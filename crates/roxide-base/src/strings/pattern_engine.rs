//! Compile bounded matchers and advance through their byte offsets.

use fancy_regex::{BytesMode, Expr, Regex, RegexBuilder, RegexInput};
use regex::bytes::{Regex as Regular, RegexBuilder as RegularBuilder};

use super::{Mode, Options};

const PROGRAM_LIMIT: usize = 1 << 20;
const PATTERN_LIMIT: usize = 1 << 15;
const BACKTRACK_LIMIT: usize = 100_000;

pub(super) struct Match {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) groups: Vec<Option<(usize, usize)>>,
}

impl Match {
    pub(super) fn new(start: usize, end: usize) -> Self {
        Self {
            start,
            end,
            groups: Vec::new(),
        }
    }
}

pub(super) struct Matcher {
    engine: Engine,
    longest: Option<Longest>,
}

enum Engine {
    Regular(Regular),
    Backtracking(Box<Regex>),
}

struct Longest {
    at_start: Regex,
    max_width: usize,
    bytes: bool,
}

impl Matcher {
    pub(super) fn compile(pattern: &[u8], options: Options) -> Result<Self, String> {
        if pattern.len() > PATTERN_LIMIT {
            return Err("regular expression is too long".into());
        }
        let source = if options.bytes {
            let mut source = String::new();
            use std::fmt::Write;
            for &b in pattern {
                if b.is_ascii() && options.mode != Mode::Fixed {
                    source.push(char::from(b));
                } else {
                    let _ = write!(source, "\\x{b:02x}");
                }
            }
            if options.mode == Mode::Perl {
                translate(&source, options.mode, true)
            } else {
                source
            }
        } else {
            let text = std::str::from_utf8(pattern)
                .map_err(|_| "input string 1 is invalid in this locale")?;
            if options.mode == Mode::Fixed {
                regex::escape(text)
            } else {
                translate(text, options.mode, false)
            }
        };
        let invalid = || {
            format!(
                "invalid regular expression '{}'",
                String::from_utf8_lossy(pattern)
            )
        };
        let longest = if options.mode == Mode::Posix && source.contains('|') {
            let tree = Expr::parse_tree(&source).map_err(|_| invalid())?;
            let (max_width, alternatives) = extent(&tree.expr);
            if alternatives {
                // The lookbehind checks the same match start after each candidate end.
                // The greedy prefix tries the farthest end first.
                let at_start = backtracking(&format!(r"(?s:\G.*)(?<=\G(?:{source}))"), options)
                    .map_err(|_| invalid())?;
                Some(Longest {
                    at_start,
                    max_width,
                    bytes: options.bytes,
                })
            } else {
                None
            }
        } else {
            None
        };
        let regular = (options.mode != Mode::Perl).then(|| {
            RegularBuilder::new(&source)
                .unicode(!options.bytes)
                .dot_matches_new_line(true)
                .case_insensitive(options.ignore_case)
                .size_limit(PROGRAM_LIMIT)
                .dfa_size_limit(PROGRAM_LIMIT)
                .nest_limit(100)
                .build()
        });
        let engine = if let Some(Ok(re)) = regular {
            Engine::Regular(re)
        } else {
            let re = backtracking(&source, options).map_err(|_| invalid())?;
            Engine::Backtracking(Box::new(re))
        };
        Ok(Self { engine, longest })
    }

    pub(super) fn capture_names(&self) -> Vec<&str> {
        match &self.engine {
            Engine::Regular(re) => re
                .capture_names()
                .skip(1)
                .map(|n| n.unwrap_or(""))
                .collect(),
            Engine::Backtracking(re) => re
                .capture_names()
                .skip(1)
                .map(|n| n.unwrap_or(""))
                .collect(),
        }
    }

    pub(super) fn find(
        &self,
        text: &[u8],
        offset: usize,
        captures: bool,
    ) -> Result<Option<Match>, String> {
        if offset > text.len() {
            return Ok(None);
        }
        let found = self.engine.find(text, offset, captures)?;
        if let (Some(found), Some(longest)) = (&found, &self.longest) {
            let start = found.start;
            let max_bytes = longest
                .max_width
                .saturating_mul(if longest.bytes { 1 } else { 4 });
            let mut end = start.saturating_add(max_bytes).min(text.len());
            if !longest.bytes {
                while text.get(end).is_some_and(|b| b & 0xc0 == 0x80) {
                    end -= 1;
                }
            }
            if end > found.end {
                let input = RegexInput::new(text).range(start..end).anchored(true);
                if let Some(matched) = find_fancy(&longest.at_start, input, captures)? {
                    return Ok(Some(matched));
                }
            }
        }
        Ok(found)
    }

    pub(super) fn exists(&self, text: &[u8]) -> Result<bool, String> {
        self.engine.find(text, 0, false).map(|m| m.is_some())
    }

    pub(super) fn matches<'a>(
        &'a self,
        text: &'a [u8],
        options: Options,
        captures: bool,
    ) -> impl Iterator<Item = Result<Match, String>> + 'a {
        let mut position = 0;
        let mut previous_end = None;
        std::iter::from_fn(move || {
            while position <= text.len() {
                let found = match self.find(text, position, captures) {
                    Ok(Some(found)) => found,
                    Ok(None) => return None,
                    Err(err) => {
                        position = text.len() + 1;
                        return Some(Err(err));
                    }
                };
                position = if found.start == found.end {
                    next_unit(text, found.end, options.bytes)
                } else {
                    found.end
                };
                if found.start != found.end || previous_end != Some(found.end) {
                    previous_end = Some(found.end);
                    return Some(Ok(found));
                }
            }
            None
        })
    }
}

impl Engine {
    fn find(&self, text: &[u8], offset: usize, captures: bool) -> Result<Option<Match>, String> {
        match self {
            Self::Regular(re) if captures => Ok(re.captures_at(text, offset).and_then(|caps| {
                let whole = caps.get(0)?;
                let mut found = Match::new(whole.start(), whole.end());
                found.groups = caps
                    .iter()
                    .map(|m| m.map(|m| (m.start(), m.end())))
                    .collect();
                Some(found)
            })),
            Self::Regular(re) => Ok(re
                .find_at(text, offset)
                .map(|m| Match::new(m.start(), m.end()))),
            Self::Backtracking(re) => {
                find_fancy(re, RegexInput::new(text).from_pos(offset), captures)
            }
        }
    }
}

fn find_fancy(
    re: &Regex,
    input: RegexInput<'_, [u8]>,
    captures: bool,
) -> Result<Option<Match>, String> {
    if captures {
        return re
            .captures_input(input)
            .map_err(|err| err.to_string())
            .map(|caps| {
                let caps = caps?;
                let whole = caps.get(0)?;
                let mut found = Match::new(whole.start(), whole.end());
                found.groups = (0..caps.len())
                    .map(|i| caps.get(i).map(|m| (m.start(), m.end())))
                    .collect();
                Some(found)
            });
    }
    re.find_input(input)
        .map_err(|err| err.to_string())
        .map(|m| m.map(|m| Match::new(m.start(), m.end())))
}

fn backtracking(source: &str, options: Options) -> fancy_regex::Result<Regex> {
    RegexBuilder::new(source)
        .bytes_mode(if options.bytes {
            BytesMode::Ascii
        } else {
            BytesMode::Unicode
        })
        .dot_matches_new_line(options.mode != Mode::Perl)
        .case_insensitive(options.ignore_case)
        .backtrack_limit(BACKTRACK_LIMIT)
        .delegate_size_limit(PROGRAM_LIMIT)
        .delegate_dfa_size_limit(PROGRAM_LIMIT)
        .delegate_dfa_skip_cache_capacity_check(false)
        .max_prog_size(PROGRAM_LIMIT)
        .build()
}

fn extent(expr: &Expr) -> (usize, bool) {
    match expr {
        Expr::Empty | Expr::Assertion(_) | Expr::LookAround(..) => (0, false),
        Expr::Any { .. } | Expr::Delegate { .. } => (1, false),
        Expr::GeneralNewline { .. } => (2, false),
        Expr::Literal { val, .. } => (val.chars().count(), false),
        Expr::LiteralBytes { bytes } => (bytes.len(), false),
        Expr::Group(child) => extent(child),
        Expr::Repeat { child, hi, .. } => {
            let (width, alternatives) = extent(child);
            (width.saturating_mul(*hi), alternatives)
        }
        Expr::Concat(children) => children
            .iter()
            .map(extent)
            .fold((0usize, false), |(n, a), (m, b)| {
                (n.saturating_add(m), a || b)
            }),
        Expr::Alt(children) => (
            children.iter().map(|c| extent(c).0).max().unwrap_or(0),
            true,
        ),
        _ => (usize::MAX, false),
    }
}

pub(super) fn next_unit(text: &[u8], offset: usize, bytes: bool) -> usize {
    let mut next = offset + 1;
    if !bytes {
        while text.get(next).is_some_and(|b| b & 0xc0 == 0x80) {
            next += 1;
        }
    }
    next
}

fn translate(pattern: &str, mode: Mode, bytewise: bool) -> String {
    let mut output = String::new();
    let mut source = pattern;
    let mut in_class = false;
    let mut class_start = false;
    while let Some(c) = source.chars().next() {
        source = &source[c.len_utf8()..];
        match c {
            '\\' => {
                if let Some(c) = source.chars().next() {
                    source = &source[c.len_utf8()..];
                    let ascii = if mode == Mode::Perl && !bytewise {
                        match c {
                            'd' => Some("[0-9]"),
                            'D' => Some("[^0-9]"),
                            's' => Some("[[:space:]]"),
                            'S' => Some("[^[:space:]]"),
                            'w' => Some("[[:word:]]"),
                            'W' => Some("[^[:word:]]"),
                            'b' if !in_class => Some(
                                r"(?:(?<![[:word:]])(?=[[:word:]])|(?<=[[:word:]])(?![[:word:]]))",
                            ),
                            'B' if !in_class => Some(
                                r"(?!(?<![[:word:]])(?=[[:word:]])|(?<=[[:word:]])(?![[:word:]]))",
                            ),
                            _ => None,
                        }
                    } else {
                        None
                    };
                    if let Some(ascii) = ascii {
                        output.push_str(ascii);
                    } else {
                        output.push('\\');
                        output.push(c);
                    }
                } else {
                    output.push('\\');
                }
                class_start = false;
            }
            '[' if in_class && source.starts_with(':') => {
                if let Some(end) = source.find(":]") {
                    let name = &source[1..end];
                    let property = match (mode, name) {
                        (Mode::Posix, "alpha") => Some(r"\p{Alphabetic}"),
                        (Mode::Posix, "lower") => Some(r"\p{Lowercase}"),
                        (Mode::Posix, "upper") => Some(r"\p{Uppercase}"),
                        (Mode::Posix, "alnum") => Some(r"\p{Alphabetic}0-9"),
                        (Mode::Posix, "space") => Some(r"\p{White_Space}"),
                        _ => None,
                    };
                    if let Some(property) = property {
                        output.push_str(property);
                    } else {
                        output.push('[');
                        output.push_str(&source[..end + 2]);
                    }
                    source = &source[end + 2..];
                } else {
                    output.push('[');
                }
                class_start = false;
            }
            '[' if !in_class => {
                in_class = true;
                class_start = true;
                output.push(c);
            }
            ']' if !class_start => {
                in_class = false;
                output.push(c);
            }
            '^' if class_start => output.push(c),
            '$' if !in_class && mode == Mode::Perl => output.push_str(r"(?:$|(?=\n\z))"),
            _ => {
                if in_class && matches!(c, '[' | ']') {
                    output.push('\\');
                }
                class_start = false;
                output.push(c);
            }
        }
    }
    output
}
