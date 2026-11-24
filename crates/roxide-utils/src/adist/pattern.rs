use std::collections::VecDeque;
use std::rc::Rc;

use super::{Costs, fold_char};

const STATE_LIMIT: usize = 4096;
const DEPTH_LIMIT: usize = 48;

struct Edge {
    target: usize,
    atom: Atom,
}

#[derive(Clone)]
enum Atom {
    Empty,
    Start,
    End,
    Boundary(Boundary),
    Char(u32, bool),
    Any,
    Class {
        negated: bool,
        members: Rc<[Member]>,
        fold: bool,
    },
}

#[derive(Clone, Copy)]
enum Boundary {
    Word,
    BetweenWords,
    WordStart,
    WordEnd,
}

impl Boundary {
    fn accepts(self, input: &[u32], index: usize) -> bool {
        let word_at = |index| {
            input
                .get(index)
                .copied()
                .and_then(char::from_u32)
                .is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
        };
        let before = index.checked_sub(1).is_some_and(word_at);
        let after = word_at(index);
        match self {
            Self::Word => before != after,
            Self::BetweenWords => before == after,
            Self::WordStart => !before && after,
            Self::WordEnd => before && !after,
        }
    }
}

#[derive(Clone)]
enum Member {
    Range(u32, u32),
    Class(NamedClass),
}

#[derive(Clone, Copy)]
enum NamedClass {
    Alnum,
    Alpha,
    Blank,
    Control,
    Digit,
    Graph,
    Lower,
    Print,
    Punct,
    Space,
    Upper,
    Word,
    Xdigit,
}

impl NamedClass {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "alnum" => Self::Alnum,
            "alpha" => Self::Alpha,
            "blank" => Self::Blank,
            "cntrl" => Self::Control,
            "digit" => Self::Digit,
            "graph" => Self::Graph,
            "lower" => Self::Lower,
            "print" => Self::Print,
            "punct" => Self::Punct,
            "space" => Self::Space,
            "upper" => Self::Upper,
            "xdigit" => Self::Xdigit,
            _ => return None,
        })
    }

    fn matches(self, ch: char, fold: bool) -> bool {
        match self {
            Self::Alnum => ch.is_alphanumeric(),
            Self::Alpha => ch.is_alphabetic(),
            Self::Blank => matches!(ch, ' ' | '\t'),
            Self::Control => ch.is_control(),
            Self::Digit => ch.is_ascii_digit(),
            Self::Graph => !ch.is_control() && !ch.is_whitespace(),
            Self::Lower => ch.is_lowercase(),
            Self::Print => !ch.is_control(),
            Self::Punct => ch.is_ascii_punctuation(),
            Self::Space => ch.is_whitespace(),
            Self::Upper if fold => ch.is_lowercase(),
            Self::Upper => ch.is_uppercase(),
            Self::Word => ch.is_alphanumeric() || ch == '_',
            Self::Xdigit => ch.is_ascii_hexdigit(),
        }
    }
}

impl Atom {
    fn accepts(&self, value: u32, bytes: bool) -> bool {
        let fold_unit = |code, fold| {
            if bytes {
                u32::from(if fold {
                    (code as u8).to_ascii_lowercase()
                } else {
                    code as u8
                })
            } else {
                char::from_u32(code).map_or(code, |ch| fold_char(ch, fold))
            }
        };
        match self {
            Self::Any => true,
            Self::Char(ch, fold) => fold_unit(*ch, *fold) == fold_unit(value, *fold),
            Self::Class {
                negated,
                members,
                fold,
            } => {
                let folded = |code| fold_unit(code, *fold);
                *negated
                    != members.iter().any(|member| match member {
                        Member::Range(lo, hi) => {
                            (*lo..=*hi).contains(&value)
                                || (folded(*lo)..=folded(*hi)).contains(&folded(value))
                        }
                        Member::Class(class) => {
                            char::from_u32(folded(value)).is_some_and(|ch| class.matches(ch, *fold))
                        }
                    })
            }
            _ => false,
        }
    }
}

enum Expr {
    Atom(Atom),
    Sequence(Vec<Self>),
    Alternatives(Vec<Self>),
    Repeat(Box<Self>, usize, Option<usize>),
}

pub(super) struct Pattern {
    states: Vec<Vec<Edge>>,
    bytes: bool,
}

#[derive(Clone, Copy)]
pub(super) struct Match {
    pub(super) cost: f64,
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) edits: [i32; 3],
}

impl Match {
    fn rank(&self) -> (f64, usize, i32, i32) {
        (
            self.cost,
            self.start,
            self.edits.iter().sum(),
            -self.edits[2],
        )
    }

    fn edit(mut self, op: usize, costs: Costs) -> Self {
        self.edits[op] += 1;
        self.cost += costs[op];
        self
    }
}

impl Pattern {
    pub(super) fn new(units: &[u32], fixed: bool, fold: bool, bytes: bool) -> Result<Self, String> {
        if units.len() >= STATE_LIMIT {
            return Err("edit distance pattern exceeds the size limit".into());
        }
        let mut pattern = Self {
            states: vec![Vec::new(), Vec::new()],
            bytes,
        };
        let expr = if fixed {
            Expr::Sequence(
                units
                    .iter()
                    .map(|&ch| Expr::Atom(Atom::Char(ch, fold)))
                    .collect(),
            )
        } else {
            let mut parser = Parser {
                units,
                index: 0,
                fold,
                groups: 0,
                backreference: false,
            };
            let expr = parser.alternatives(0)?;
            if parser.index != units.len() {
                return Err(regcomp("Missing '('"));
            }
            if parser.backreference {
                return Ok(pattern);
            }
            expr
        };
        pattern.compile(&expr, 0, 1)?;
        Ok(pattern)
    }

    pub(super) fn len(&self) -> usize {
        self.states.len()
    }

    fn state(&mut self) -> Result<usize, String> {
        if self.states.len() >= STATE_LIMIT {
            return Err("edit distance pattern exceeds the size limit".into());
        }
        let index = self.states.len();
        self.states.push(Vec::new());
        Ok(index)
    }

    fn edge(&mut self, from: usize, target: usize, atom: Atom) {
        self.states[from].push(Edge { target, atom });
    }

    fn compile(&mut self, expr: &Expr, start: usize, end: usize) -> Result<(), String> {
        match expr {
            Expr::Atom(atom) => self.edge(start, end, atom.clone()),
            Expr::Sequence(parts) => {
                let mut state = start;
                for (index, part) in parts.iter().enumerate() {
                    let next = if index + 1 == parts.len() {
                        end
                    } else {
                        self.state()?
                    };
                    self.compile(part, state, next)?;
                    state = next;
                }
                if parts.is_empty() {
                    self.edge(start, end, Atom::Empty);
                }
            }
            Expr::Alternatives(parts) => {
                if let [part] = parts.as_slice() {
                    self.compile(part, start, end)?;
                } else {
                    for part in parts {
                        let branch = self.state()?;
                        self.edge(start, branch, Atom::Empty);
                        self.compile(part, branch, end)?;
                    }
                }
            }
            Expr::Repeat(part, min, max) => {
                let mut state = start;
                for _ in 0..*min {
                    let next = self.state()?;
                    self.compile(part, state, next)?;
                    state = next;
                }
                self.edge(state, end, Atom::Empty);
                if let Some(max) = max {
                    for _ in *min..*max {
                        let next = self.state()?;
                        self.compile(part, state, next)?;
                        self.edge(next, end, Atom::Empty);
                        state = next;
                    }
                } else {
                    let next = self.state()?;
                    self.compile(part, state, next)?;
                    self.edge(next, state, Atom::Empty);
                }
            }
        }
        Ok(())
    }

    pub(super) fn find(&self, input: &[u32], costs: Costs) -> Option<Match> {
        let mut best: Option<Match> = None;
        let mut current = vec![None; self.len()];
        let mut next = vec![None; self.len()];
        let mut queue = VecDeque::with_capacity(self.len());
        for end in 0..=input.len() {
            let initial = Match {
                cost: 0.0,
                start: end,
                end,
                edits: [0; 3],
            };
            update(&mut current[0], initial);
            queue.extend(
                current
                    .iter()
                    .enumerate()
                    .filter_map(|(i, entry)| entry.map(|_| i)),
            );
            while let Some(node) = queue.pop_front() {
                let Some(found) = current[node] else { continue };
                for edge in &self.states[node] {
                    let candidate = match edge.atom {
                        Atom::Empty => found,
                        Atom::Start if end == 0 => found,
                        Atom::End if end == input.len() => found,
                        Atom::Boundary(boundary) if boundary.accepts(input, end) => found,
                        Atom::Start | Atom::End | Atom::Boundary(_) => continue,
                        _ => found.edit(1, costs),
                    };
                    if update(&mut current[edge.target], candidate) {
                        queue.push_back(edge.target);
                    }
                }
            }
            if let Some(mut found) = current[1] {
                found.end = end;
                if best.is_none_or(|best| found.rank() <= best.rank()) {
                    best = Some(found);
                }
            }
            let Some(&unit) = input.get(end) else { break };
            for (node, found) in current.iter().copied().enumerate() {
                if let Some(found) = found {
                    for edge in &self.states[node] {
                        if matches!(
                            edge.atom,
                            Atom::Empty | Atom::Start | Atom::End | Atom::Boundary(_)
                        ) {
                            continue;
                        }
                        let candidate = if edge.atom.accepts(unit, self.bytes) {
                            found
                        } else {
                            found.edit(2, costs)
                        };
                        update(&mut next[edge.target], candidate);
                    }
                    update(&mut next[node], found.edit(0, costs));
                }
            }
            std::mem::swap(&mut current, &mut next);
            next.fill(None);
        }
        best
    }
}

fn update(slot: &mut Option<Match>, candidate: Match) -> bool {
    if slot.is_none_or(|old| candidate.rank() < old.rank()) {
        *slot = Some(candidate);
        true
    } else {
        false
    }
}

struct Parser<'a> {
    units: &'a [u32],
    index: usize,
    fold: bool,
    groups: u32,
    backreference: bool,
}

fn regcomp(message: &str) -> String {
    format!("regcomp error:  '{message}'")
}

impl Parser<'_> {
    fn at(&self, ascii: u8) -> bool {
        self.units.get(self.index) == Some(&u32::from(ascii))
    }

    fn take(&mut self, ascii: u8) -> bool {
        if !self.at(ascii) {
            return false;
        }
        self.index += 1;
        true
    }

    fn next(&mut self) -> Option<u32> {
        let unit = self.units.get(self.index).copied()?;
        self.index += 1;
        Some(unit)
    }

    fn alternatives(&mut self, depth: usize) -> Result<Expr, String> {
        let mut branches = Vec::new();
        loop {
            let mut items = Vec::new();
            while self.index < self.units.len() && !self.at(b')') && !self.at(b'|') {
                let item = self.atom(depth)?;
                let repetition = if self.take(b'*') {
                    Some((0, None))
                } else if self.take(b'+') {
                    Some((1, None))
                } else if self.take(b'?') {
                    Some((0, Some(1)))
                } else if self.take(b'{') {
                    Some(self.interval()?)
                } else {
                    None
                };
                items.push(if let Some((min, max)) = repetition {
                    Expr::Repeat(Box::new(item), min, max)
                } else {
                    item
                });
            }
            branches.push(Expr::Sequence(items));
            if !self.take(b'|') {
                return Ok(Expr::Alternatives(branches));
            }
        }
    }

    fn atom(&mut self, depth: usize) -> Result<Expr, String> {
        let atom = match self.next().and_then(char::from_u32) {
            Some('(') => return self.group(depth),
            Some('[') => self.class()?,
            Some('^') => Atom::Start,
            Some('$') => Atom::End,
            Some('.') => Atom::Any,
            Some('*' | '+' | '?') => return Err(regcomp("Invalid use of repetition operators")),
            Some('\\') => self.escaped()?,
            Some(ch) => self.literal(u32::from(ch)),
            None => return Err(regcomp("Invalid character")),
        };
        Ok(Expr::Atom(atom))
    }

    fn group(&mut self, depth: usize) -> Result<Expr, String> {
        if depth >= DEPTH_LIMIT {
            return Err("edit distance pattern exceeds the nesting limit".into());
        }
        let previous_fold = self.fold;
        if !self.take(b'?') {
            self.groups += 1;
        } else if !self.take(b':') {
            let fold = !self.take(b'-');
            if !self.take(b'i') {
                return Err(regcomp("Invalid regexp"));
            }
            self.fold = fold;
            if self.take(b')') {
                return Ok(Expr::Atom(Atom::Empty));
            }
            if !self.take(b':') {
                return Err(regcomp("Invalid regexp"));
            }
        }
        let expr = self.alternatives(depth + 1)?;
        self.fold = previous_fold;
        if !self.take(b')') {
            return Err(regcomp("Missing ')'"));
        }
        Ok(expr)
    }

    fn escaped(&mut self) -> Result<Atom, String> {
        let unit = self.next().ok_or_else(|| regcomp("Trailing backslash"))?;
        let ch = char::from_u32(unit).ok_or_else(|| regcomp("Invalid character"))?;
        let class = match ch.to_ascii_lowercase() {
            'd' => Some(NamedClass::Digit),
            's' => Some(NamedClass::Space),
            'w' => Some(NamedClass::Word),
            _ => None,
        };
        if let Some(class) = class {
            return Ok(Atom::Class {
                negated: ch.is_ascii_uppercase(),
                members: Rc::new([Member::Class(class)]),
                fold: self.fold,
            });
        }
        Ok(match ch {
            'b' => Atom::Boundary(Boundary::Word),
            'B' => Atom::Boundary(Boundary::BetweenWords),
            '<' => Atom::Boundary(Boundary::WordStart),
            '>' => Atom::Boundary(Boundary::WordEnd),
            'n' => self.literal(u32::from(b'\n')),
            'r' => self.literal(u32::from(b'\r')),
            't' => self.literal(u32::from(b'\t')),
            '1'..='9' => {
                if unit - u32::from(b'0') > self.groups {
                    return Err(regcomp("Invalid back reference"));
                }
                self.backreference = true;
                Atom::Empty
            }
            _ => self.literal(unit),
        })
    }

    fn literal(&self, unit: u32) -> Atom {
        Atom::Char(unit, self.fold)
    }

    fn interval(&mut self) -> Result<(usize, Option<usize>), String> {
        let invalid = || regcomp("Invalid contents of {}");
        let min = self.number().ok_or_else(invalid)?;
        let max = if self.take(b',') {
            self.number()
        } else {
            Some(min)
        };
        if !self.take(b'}')
            || min >= STATE_LIMIT
            || max.is_some_and(|max| max < min || max >= STATE_LIMIT)
        {
            return Err(invalid());
        }
        Ok((min, max))
    }

    fn number(&mut self) -> Option<usize> {
        let start = self.index;
        let mut value: usize = 0;
        while let Some(digit) = self
            .units
            .get(self.index)
            .and_then(|&unit| char::from_u32(unit))
            .and_then(|ch| ch.to_digit(10))
        {
            self.index += 1;
            value = value.saturating_mul(10).saturating_add(digit as usize);
        }
        (self.index != start).then_some(value)
    }

    fn class(&mut self) -> Result<Atom, String> {
        let negated = self.take(b'^');
        let mut members = Vec::new();
        if self.take(b']') {
            members.push(Member::Range(u32::from(b']'), u32::from(b']')));
        }
        while !self.take(b']') {
            let lo = self.next().ok_or_else(|| regcomp("Missing ']'"))?;
            if lo == u32::from(b'[') && self.take(b':') {
                let mut name = String::new();
                while !self.take(b':') {
                    name.push(
                        self.next()
                            .and_then(char::from_u32)
                            .ok_or_else(|| regcomp("Unknown character class name"))?,
                    );
                }
                let class = NamedClass::parse(&name)
                    .filter(|_| self.take(b']'))
                    .ok_or_else(|| regcomp("Unknown character class name"))?;
                members.push(Member::Class(class));
            } else {
                let hi =
                    if self.at(b'-') && self.units.get(self.index + 1) != Some(&u32::from(b']')) {
                        self.index += 1;
                        self.next().ok_or_else(|| regcomp("Missing ']'"))?
                    } else {
                        lo
                    };
                if lo > hi {
                    return Err(regcomp("Invalid character range"));
                }
                members.push(Member::Range(lo, hi));
            }
        }
        Ok(Atom::Class {
            negated,
            members: members.into(),
            fold: self.fold,
        })
    }
}
