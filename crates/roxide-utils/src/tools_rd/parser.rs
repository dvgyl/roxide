use std::collections::BTreeMap;

#[derive(Clone, Copy)]
enum Mode {
    Text,
    Code,
    Verbatim,
    Equation,
}

impl Mode {
    fn tag(self) -> &'static str {
        match self {
            Self::Text => "TEXT",
            Self::Code => "RCODE",
            Self::Verbatim | Self::Equation => "VERB",
        }
    }
}

#[derive(Clone, Copy)]
enum Item {
    None,
    Marker,
    Pair,
}

#[derive(Clone, Copy)]
struct Grammar {
    mode: Mode,
    item: Item,
    sections: bool,
}

impl Grammar {
    const TEXT: Self = Self {
        mode: Mode::Text,
        item: Item::None,
        sections: false,
    };
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Position {
    line: i32,
    byte: i32,
    column: i32,
}

impl Position {
    const START: Self = Self {
        line: 1,
        byte: 1,
        column: 1,
    };
    const ZERO: Self = Self {
        line: 0,
        byte: 0,
        column: 0,
    };

    fn after(&mut self, ch: char) {
        if ch == '\n' {
            self.line = self.line.saturating_add(1);
            self.byte = 1;
            self.column = 1;
        } else {
            self.byte = self.byte.saturating_add(ch.len_utf8() as i32);
            self.column = self.column.saturating_add(if ch == '\t' {
                8 - (self.column - 1) % 8
            } else {
                1
            });
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Span(Position, Position);

impl Span {
    pub(super) fn integers(self) -> [i32; 6] {
        let Self(a, b) = self;
        [a.line, a.byte, b.line, b.byte, a.column, b.column]
    }
}

pub(super) enum Body {
    Text(String),
    Children(Vec<Node>),
    Definition {
        name: String,
        body: String,
        arity: usize,
    },
    Expansion {
        name: String,
        values: Vec<String>,
    },
}

pub(super) struct Node {
    pub(super) tag: String,
    pub(super) body: Body,
    pub(super) span: Option<Span>,
    pub(super) option: Option<Box<Node>>,
    pub(super) dynamic: i32,
}

impl Node {
    pub(super) fn new(tag: &str, body: Body, span: Option<Span>) -> Self {
        let dynamic = match &body {
            Body::Children(children) => children.iter().fold(0, |flag, child| flag | child.dynamic),
            _ => 0,
        } | match tag {
            "#ifdef" | "#ifndef" => 1,
            "\\Sexpr" => 10,
            _ => 0,
        };
        Self {
            tag: tag.to_owned(),
            body,
            span,
            option: None,
            dynamic,
        }
    }

    pub(super) fn children(&self) -> &[Self] {
        match &self.body {
            Body::Children(children) => children,
            _ => &[],
        }
    }

    pub(super) fn text(&self) -> &str {
        match &self.body {
            Body::Text(text) => text,
            _ => "",
        }
    }
}

pub(super) struct Parser {
    pub(super) macros: BTreeMap<String, (String, usize)>,
    pub(super) warnings: Vec<(i32, String)>,
    budget: usize,
    depth: usize,
    permissive: bool,
    keep_empty: bool,
}

type Result<T> = std::result::Result<T, &'static str>;

impl Parser {
    pub(super) fn new(limit: usize, depth: usize, permissive: bool, keep_empty: bool) -> Self {
        Self {
            macros: BTreeMap::new(),
            warnings: Vec::new(),
            budget: limit,
            depth,
            permissive,
            keep_empty,
        }
    }

    fn charge(&mut self, bytes: usize) -> Result<()> {
        self.budget = self
            .budget
            .checked_sub(bytes)
            .ok_or("Rd size limit exceeded")?;
        Ok(())
    }

    pub(super) fn parse(&mut self, source: &str, fragment: bool) -> Result<Node> {
        self.charge(source.len())?;
        let mut cursor = Cursor::new(source);
        let grammar = Grammar {
            sections: !fragment,
            ..Grammar::TEXT
        };
        let nodes = self.nodes(&mut cursor, grammar, End::Input, 0)?;
        Ok(Node::new(
            "",
            Body::Children(nodes),
            Some(Span(Position::ZERO, cursor.pos)),
        ))
    }

    fn warning(&mut self, pos: Position, message: String) {
        self.warnings.push((pos.line, message));
    }

    fn nodes(
        &mut self,
        c: &mut Cursor<'_>,
        grammar: Grammar,
        end: End,
        depth: usize,
    ) -> Result<Vec<Node>> {
        if depth > self.depth {
            return Err("Rd nesting depth limit exceeded");
        }
        let mut nodes = Vec::new();
        let mut code = CodeState::default();
        let mut merge = false;
        while c.peek().is_some() && !end.at(c, &code) {
            self.charge(size_of::<Node>())?;
            let start = c.pos;
            let current = c.peek();
            if current == Some('%') && !code.comment {
                let text = c.until_newline();
                nodes.push(Node::new("COMMENT", Body::Text(text), Some(c.span(start))));
            } else if c.directive("#ifdef") || c.directive("#ifndef") {
                nodes.push(self.conditional(c, grammar, depth)?);
            } else if c.macro_start(grammar.mode, &code) {
                merge = self.markup(c, grammar, depth, &mut nodes)?;
                continue;
            } else if current == Some('{') && matches!(grammar.mode, Mode::Text) {
                c.next();
                let children = self.nodes(
                    c,
                    Grammar {
                        sections: false,
                        ..grammar
                    },
                    End::Brace,
                    depth + 1,
                )?;
                self.close(c, '}');
                if grammar.sections {
                    nodes.extend(children);
                } else {
                    nodes.push(Node::new(
                        "LIST",
                        Body::Children(children),
                        Some(c.span(start)),
                    ));
                }
            } else if current == Some('}') && matches!(grammar.mode, Mode::Text) {
                c.next();
                self.warning(start, "unexpected '}'".to_owned());
                continue;
            } else {
                let text = c.leaf(grammar.mode, end, &mut code, self.depth)?;
                if merge
                    && let Some(previous) = nodes.last_mut()
                    && previous.tag == grammar.mode.tag()
                    && let Body::Text(value) = &mut previous.body
                {
                    value.push_str(&text);
                    previous.span = Some(c.span(start));
                } else {
                    nodes.push(Node::new(
                        grammar.mode.tag(),
                        Body::Text(text),
                        Some(c.span(start)),
                    ));
                }
            }
            merge = false;
        }
        Ok(nodes)
    }

    fn close(&mut self, c: &mut Cursor<'_>, delimiter: char) {
        if c.peek() == Some(delimiter) {
            c.next();
        } else {
            self.warning(c.pos, "unexpected END_OF_INPUT '\n'".to_owned());
        }
    }

    fn conditional(&mut self, c: &mut Cursor<'_>, grammar: Grammar, depth: usize) -> Result<Node> {
        let start = c.pos;
        let tag = if c.directive("#ifdef") {
            "#ifdef"
        } else {
            "#ifndef"
        };
        for _ in tag.chars() {
            c.next();
        }
        let condition_start = c.pos;
        let mut condition = c.until_newline();
        if let Some('\n') = c.next() {
            condition.push('\n');
        }
        let condition = Node::new("TEXT", Body::Text(condition), Some(c.span(condition_start)));
        let children = self.nodes(c, grammar, End::Directive, depth + 1)?;
        if c.directive("#endif") {
            c.until_newline();
            c.next();
        } else {
            self.warning(c.pos, "unexpected END_OF_INPUT '\n'".to_owned());
        }
        Ok(Node::new(
            tag,
            Body::Children(vec![group(vec![condition]), group(children)]),
            Some(c.span(start)),
        ))
    }

    fn markup(
        &mut self,
        c: &mut Cursor<'_>,
        grammar: Grammar,
        depth: usize,
        nodes: &mut Vec<Node>,
    ) -> Result<bool> {
        let start = c.pos;
        let tag = c.macro_name();
        if matches!(tag, "\\newcommand" | "\\renewcommand") {
            let mut name = self.raw_argument(c)?;
            name.truncate(name.trim_end().len());
            let body = self.raw_argument(c)?;
            let arity = macro_arity(&body);
            if tag == "\\newcommand" && self.macros.contains_key(&name) && !self.permissive {
                self.warning(start, format!("macro '{name}' previously defined"));
            }
            self.charge(name.len().saturating_add(body.len()))?;
            self.macros.insert(name.clone(), (body.clone(), arity));
            nodes.push(Node::new(
                tag,
                Body::Definition { name, body, arity },
                Some(c.span(start)),
            ));
            return Ok(false);
        }
        if let Some((body, arity)) = self.macros.get(tag).cloned() {
            let mut values = vec![body];
            for _ in 0..arity {
                values.push(self.raw_argument(c)?);
            }
            let mut replacement = String::new();
            let mut chars = values[0].chars().peekable();
            while let Some(ch) = chars.next() {
                let arg = if ch == '#' {
                    chars.peek().and_then(|ch| ch.to_digit(10))
                } else {
                    None
                };
                if let Some(value) = arg
                    .and_then(|n| values.get(n as usize))
                    .filter(|_| arg != Some(0))
                {
                    chars.next();
                    self.charge(value.len())?;
                    replacement.push_str(value);
                } else {
                    self.charge(ch.len_utf8())?;
                    replacement.push(ch);
                }
            }
            self.charge(values.iter().map(String::len).sum())?;
            nodes.push(Node::new(
                "USERMACRO",
                Body::Expansion {
                    name: tag.to_owned(),
                    values,
                },
                Some(c.span(start)),
            ));
            let span = Span(c.pos, c.last);
            let mut expanded = self.nodes(
                &mut Cursor::new(&replacement),
                grammar,
                End::Input,
                depth + 1,
            )?;
            for node in &mut expanded {
                node.span = Some(span);
            }
            let merge = !replacement.ends_with('\n')
                && expanded.last().is_some_and(|n| n.tag == grammar.mode.tag());
            nodes.extend(expanded);
            return Ok(merge);
        }
        let Some(spec) = Spec::get(tag, grammar.item) else {
            self.warning(
                start,
                if grammar.sections {
                    format!("unexpected UNKNOWN '{tag}'")
                } else {
                    format!("unknown macro '{tag}'")
                },
            );
            if !grammar.sections {
                nodes.push(Node::new(
                    "UNKNOWN",
                    Body::Text(tag.to_owned()),
                    Some(c.span(start)),
                ));
            }
            return Ok(false);
        };
        if spec.section != grammar.sections && tag != "\\Sexpr" {
            let kind = if spec.section {
                "section header"
            } else {
                "macro"
            };
            self.warning(start, format!("unexpected {kind} '{tag}'"));
            return Ok(false);
        }
        let option = if spec.option && c.peek() == Some('[') {
            c.next();
            let mut children = self.nodes(c, Grammar::TEXT, End::Bracket, depth + 1)?;
            self.close(c, ']');
            match children.len() {
                0 => None,
                1 => children.pop().map(Box::new),
                _ => Some(Box::new(group(children))),
            }
        } else {
            None
        };

        let mut children = Vec::new();
        for (i, &mode) in spec.args.iter().enumerate() {
            if c.peek() != Some('{') {
                if spec.optional_last && i == 1 {
                    break;
                }
                if self.permissive && i == 0 && !spec.section && c.peek().is_some() {
                    let arg_start = c.pos;
                    let text = c.leaf(mode, End::Input, &mut CodeState::default(), self.depth)?;
                    let arg = Node::new(mode.tag(), Body::Text(text), Some(c.span(arg_start)));
                    if spec.args.len() == 1 {
                        children.push(arg);
                    } else {
                        children.push(group(vec![arg]));
                    }
                } else {
                    self.warning(c.pos, format!("expected '{{' after '{tag}'"));
                }
                break;
            }
            c.next();
            let inner = Grammar {
                mode,
                item: spec.item,
                sections: false,
            };
            let argument = self.nodes(c, inner, End::Brace, depth + 1)?;
            self.close(c, '}');
            if spec.args.len() == 1 {
                children = argument;
            } else {
                children.push(group(argument));
            }
        }
        if !self.keep_empty && spec.section && children.is_empty() {
            return Ok(false);
        }
        let mut node = Node::new(tag, Body::Children(children), Some(c.span(start)));
        if tag == "\\Sexpr"
            && let Some(option) = &option
        {
            let stage = option
                .text()
                .split(',')
                .filter_map(|field| field.split_once('='))
                .find(|(key, _)| key.trim() == "stage")
                .map(|(_, value)| value.trim());
            node.dynamic = match stage {
                Some("build") => 6,
                Some("render") => 18,
                _ => 10,
            };
        }
        node.option = option;
        nodes.push(node);
        Ok(false)
    }

    fn raw_argument(&mut self, c: &mut Cursor<'_>) -> Result<String> {
        if c.peek() != Some('{') {
            self.warning(c.pos, "expected '{' in Rd macro".to_owned());
            return Ok(String::new());
        }
        c.next();
        let start = c.offset;
        let mut nesting = 0;
        while let Some(ch) = c.peek() {
            match ch {
                '}' if nesting == 0 => break,
                '}' => nesting -= 1,
                '{' => {
                    nesting += 1;
                    if nesting > self.depth {
                        return Err("Rd nesting depth limit exceeded");
                    }
                }
                '\\' if c.escape().is_some() => {
                    c.next();
                }
                _ => {}
            }
            c.next();
        }
        let text = c.source[start..c.offset].to_owned();
        self.close(c, '}');
        Ok(text)
    }
}

fn macro_arity(text: &str) -> usize {
    text.as_bytes()
        .windows(2)
        .filter(|pair| pair[0] == b'#' && pair[1].is_ascii_digit())
        .map(|pair| usize::from(pair[1] - b'0'))
        .max()
        .unwrap_or(0)
}

fn group(children: Vec<Node>) -> Node {
    Node::new("", Body::Children(children), None)
}

#[derive(Clone, Copy)]
enum End {
    Input,
    Brace,
    Bracket,
    Directive,
}

impl End {
    fn at(self, c: &Cursor<'_>, code: &CodeState) -> bool {
        match self {
            Self::Input => false,
            Self::Brace => {
                c.peek() == Some('}')
                    && code.braces == 0
                    && code.quote.is_none()
                    && code.raw.is_none()
            }
            Self::Bracket => c.peek() == Some(']'),
            Self::Directive => c.directive("#endif"),
        }
    }
}

#[derive(Default)]
struct CodeState {
    braces: usize,
    quote: Option<char>,
    comment: bool,
    raw: Option<RawQuote>,
}

#[derive(Clone, Copy)]
struct RawQuote {
    delimiter: char,
    quote: char,
    dashes: usize,
}

impl RawQuote {
    fn end_len(self, text: &str) -> Option<usize> {
        let tail = text.strip_prefix(self.delimiter)?;
        let padding = tail.get(..self.dashes)?;
        if padding.bytes().all(|ch| ch == b'-') && tail.get(self.dashes..)?.starts_with(self.quote)
        {
            Some(self.dashes + 2)
        } else {
            None
        }
    }
}

struct Cursor<'a> {
    source: &'a str,
    offset: usize,
    pos: Position,
    last: Position,
}

impl<'a> Cursor<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            offset: 0,
            pos: Position::START,
            last: Position::START,
        }
    }

    fn rest(&self) -> &'a str {
        &self.source[self.offset..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn next(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.offset += ch.len_utf8();
        self.last = self.pos;
        self.last.byte = self.last.byte.saturating_add(ch.len_utf8() as i32 - 1);
        self.pos.after(ch);
        Some(ch)
    }

    fn span(&self, start: Position) -> Span {
        Span(start, self.last)
    }

    fn until_newline(&mut self) -> String {
        let start = self.offset;
        while self.peek().is_some_and(|ch| ch != '\n') {
            self.next();
        }
        self.source[start..self.offset].to_owned()
    }

    fn directive(&self, tag: &str) -> bool {
        self.pos.column == 1
            && self
                .rest()
                .strip_prefix(tag)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
    }

    fn macro_start(&self, mode: Mode, code: &CodeState) -> bool {
        let Some(rest) = self.rest().strip_prefix('\\') else {
            return false;
        };
        match mode {
            Mode::Text => rest.starts_with(|ch: char| ch.is_ascii_alphabetic()),
            Mode::Code if !code.comment && code.raw.is_none() => {
                rest.starts_with(|ch: char| ch.is_ascii_alphabetic())
                    && (code.quote.is_none() || rest.starts_with("link") || rest.starts_with("var"))
            }
            _ => false,
        }
    }

    fn raw_open(&self) -> Option<(RawQuote, usize)> {
        let rest = self.rest().strip_prefix(['r', 'R'])?;
        if self.source[..self.offset]
            .ends_with(|ch: char| ch.is_alphanumeric() || ch == '_' || ch == '.')
        {
            return None;
        }
        let quote = rest.chars().next().filter(|ch| matches!(ch, '"' | '\''))?;
        let tail = &rest[1..];
        let dashes = tail.bytes().take_while(|&ch| ch == b'-').count();
        let delimiter = match tail.as_bytes().get(dashes)? {
            b'(' => ')',
            b'[' => ']',
            b'{' => '}',
            _ => return None,
        };
        Some((
            RawQuote {
                delimiter,
                quote,
                dashes,
            },
            3 + dashes,
        ))
    }

    fn copy_ascii(&mut self, len: usize, output: &mut String) {
        let start = self.offset;
        for _ in 0..len {
            self.next();
        }
        output.push_str(&self.source[start..self.offset]);
    }

    fn macro_name(&mut self) -> &'a str {
        let start = self.offset;
        self.next();
        while self.peek().is_some_and(|ch| ch.is_ascii_alphanumeric()) {
            self.next();
        }
        &self.source[start..self.offset]
    }

    fn escape(&self) -> Option<char> {
        self.rest()
            .strip_prefix('\\')?
            .chars()
            .next()
            .filter(|ch| matches!(ch, '%' | '{' | '}' | '\\'))
    }

    fn leaf(
        &mut self,
        mode: Mode,
        end: End,
        state: &mut CodeState,
        depth: usize,
    ) -> Result<String> {
        let mut text = String::new();
        while let Some(ch) = self.peek() {
            if end.at(self, state)
                || (ch == '%' && !state.comment)
                || self.macro_start(mode, state)
                || (matches!(mode, Mode::Text) && matches!(ch, '{' | '}'))
            {
                break;
            }
            if matches!(mode, Mode::Code) && state.quote.is_none() && !state.comment {
                if let Some(raw) = state.raw {
                    if let Some(len) = raw.end_len(self.rest()) {
                        self.copy_ascii(len, &mut text);
                        state.raw = None;
                        continue;
                    }
                } else if let Some((raw, len)) = self.raw_open() {
                    self.copy_ascii(len, &mut text);
                    state.raw = Some(raw);
                    continue;
                }
            }
            let escape = self.escape();
            self.next();
            if let Some(escaped) = escape {
                self.next();
                if matches!(mode, Mode::Equation) {
                    text.push('\\');
                }
                text.push(escaped);
                continue;
            }
            text.push(ch);
            if ch == '\n' {
                state.comment = false;
                break;
            }
            if state.raw.is_some() {
                continue;
            }
            if matches!(mode, Mode::Code) && !state.comment {
                if let Some(quote) = state.quote {
                    if ch == quote {
                        state.quote = None;
                    } else if ch == '\\'
                        && self.peek() != Some('\n')
                        && let Some(escaped) = self.next()
                    {
                        text.push(escaped);
                    }
                    continue;
                }
                match ch {
                    '\'' | '"' | '`' => state.quote = Some(ch),
                    '#' => state.comment = true,
                    _ => {}
                }
            }
            if !matches!(mode, Mode::Text) {
                if ch == '{' {
                    state.braces += 1;
                    if state.braces > depth {
                        return Err("Rd nesting depth limit exceeded");
                    }
                } else if ch == '}' {
                    state.braces = state.braces.saturating_sub(1);
                }
            }
        }
        Ok(text)
    }
}

struct Spec {
    args: &'static [Mode],
    item: Item,
    section: bool,
    option: bool,
    optional_last: bool,
}

impl Spec {
    fn get(tag: &str, item: Item) -> Option<Self> {
        use Mode::{Code, Equation, Text, Verbatim};
        let (args, section): (&'static [Mode], bool) = match tag {
            "\\name" | "\\alias" | "\\Rdversion" | "\\synopsis" => (&[Verbatim], true),
            "\\title" | "\\description" | "\\arguments" | "\\value" | "\\details" | "\\note"
            | "\\author" | "\\references" | "\\seealso" | "\\keyword" | "\\concept"
            | "\\docType" | "\\encoding" | "\\format" | "\\source" => (&[Text], true),
            "\\usage" | "\\examples" => (&[Code], true),
            "\\section" => (&[Text, Text], true),
            "\\code" | "\\special" | "\\donttest" | "\\dontshow" | "\\testonly" | "\\Sexpr" => {
                (&[Code], false)
            }
            "\\preformatted" | "\\verb" | "\\url" | "\\env" | "\\out" | "\\dontrun" | "\\kbd" => {
                (&[Verbatim], false)
            }
            "\\emph" | "\\strong" | "\\bold" | "\\sQuote" | "\\dQuote" | "\\pkg" | "\\file"
            | "\\option" | "\\command" | "\\dfn" | "\\cite" | "\\email" | "\\link"
            | "\\linkS4class" | "\\itemize" | "\\enumerate" | "\\describe" | "\\acronym"
            | "\\var" | "\\abbr" => (&[Text], false),
            "\\href" => (&[Verbatim, Text], false),
            "\\eqn" | "\\deqn" => (&[Equation, Verbatim], false),
            "\\enc" | "\\tabular" | "\\method" | "\\S3method" | "\\S4method" | "\\if" => {
                (&[Text, Text], false)
            }
            "\\ifelse" => (&[Text, Text, Text], false),
            "\\R" | "\\dots" | "\\ldots" | "\\cr" | "\\tab" => (&[], false),
            "\\item" => match item {
                Item::None => return None,
                Item::Marker => (&[], false),
                Item::Pair => (&[Text, Text], false),
            },
            _ => return None,
        };
        Some(Self {
            args,
            item: match tag {
                "\\arguments" | "\\value" | "\\describe" => Item::Pair,
                "\\itemize" | "\\enumerate" => Item::Marker,
                _ => Item::None,
            },
            section,
            option: matches!(tag, "\\link" | "\\linkS4class" | "\\Sexpr"),
            optional_last: matches!(tag, "\\eqn" | "\\deqn"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Position;

    #[test]
    fn source_positions_saturate_at_the_integer_limit() {
        let mut position = Position {
            line: i32::MAX,
            byte: i32::MAX - 1,
            column: i32::MAX - 1,
        };
        position.after('é');
        position.after('\t');
        assert_eq!(position.byte, i32::MAX);
        assert_eq!(position.column, i32::MAX);
        position.after('\n');
        assert_eq!(position.line, i32::MAX);
        assert_eq!(position.column, 1);
    }
}
