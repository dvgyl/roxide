use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{BASE_ENV, SEXP_NIL, SexpType, ValueApi};

use super::parser::{Body, Node};
use super::{attr, chars, first_text, flag, strings, tree};

struct Options {
    width: usize,
    section_indent: usize,
    min_indent: usize,
    extra_indent: usize,
    section_extra: usize,
    bullet: String,
    underline: bool,
    code_quote: bool,
    show_urls: bool,
    describe_inline: bool,
    enumeration: SexpId,
    defines: Vec<String>,
}

impl Options {
    fn read(ctx: &mut Ctx, call: &Call, value: SexpId, defines: SexpId) -> EvalResult<Self> {
        let mut options = Self {
            width: 80,
            section_indent: 5,
            min_indent: 10,
            extra_indent: 4,
            section_extra: 2,
            bullet: "• ".to_owned(),
            underline: true,
            code_quote: true,
            show_urls: false,
            describe_inline: false,
            enumeration: SEXP_NIL,
            defines: strings(ctx, defines)?.into_iter().flatten().collect(),
        };
        let names = attr(ctx, value, "names")?;
        if names == SEXP_NIL {
            return Ok(options);
        }
        for (i, name) in strings(ctx, names)?.into_iter().enumerate() {
            let item = ctx.session().list_get(value, i)?;
            match name.as_deref() {
                Some("width") => options.width = number(ctx, item)?.clamp(1, 10_000),
                Some("sectionIndent") => options.section_indent = number(ctx, item)?,
                Some("minIndent") => options.min_indent = number(ctx, item)?,
                Some("extraIndent") => options.extra_indent = number(ctx, item)?,
                Some("sectionExtra") => options.section_extra = number(ctx, item)?,
                Some("itemBullet") => options.bullet = first_text(ctx, item)?.unwrap_or_default(),
                Some("underline_titles") => options.underline = flag(ctx, call, item)?,
                Some("code_quote") => options.code_quote = flag(ctx, call, item)?,
                Some("showURLs") => options.show_urls = flag(ctx, call, item)?,
                Some("descStyle") => {
                    options.describe_inline = first_text(ctx, item)?.as_deref() != Some("linebreak")
                }
                Some("enumFormat") => options.enumeration = item,
                _ => {}
            }
        }
        Ok(options)
    }

    fn active(&self, node: &Node) -> bool {
        let target = node
            .children()
            .first()
            .and_then(|n| n.children().first())
            .map_or("", Node::text);
        self.defines.iter().any(|d| d == target.trim()) == (node.tag == "#ifdef")
    }
}

fn number(ctx: &mut Ctx, value: SexpId) -> EvalResult<usize> {
    let (value, _) = ctx.session_mut().coerce(value, SexpType::Int)?;
    Ok(ctx
        .session()
        .int(value)?
        .iter()
        .next()
        .unwrap_or(0)
        .clamp(0, 10_000) as usize)
}

fn sexpr_option<'a>(options: &'a str, key: &str, default: &'a str) -> &'a str {
    options
        .split(',')
        .filter_map(|field| field.split_once('='))
        .find(|(name, _)| name.trim() == key)
        .map_or(default, |(_, value)| value.trim().trim_matches(['"', '\'']))
}

fn evaluate_stage(
    ctx: &mut Ctx,
    call: &Call,
    node: &mut Node,
    stage: &str,
    evaluate: SexpId,
    remaining: &mut usize,
) -> EvalResult<()> {
    if node.tag == "\\Sexpr" {
        let options = node.option.as_deref().map_or("", Node::text);
        if sexpr_option(options, "stage", "install") == stage {
            let value = if sexpr_option(options, "eval", "TRUE") == "FALSE" {
                String::new()
            } else {
                ctx.scoped(|ctx| {
                    let code: String = node.children().iter().map(Node::text).collect();
                    let code = ctx.session_mut().scalar_str(&code)?;
                    ctx.protect(code);
                    let results = ctx
                        .session_mut()
                        .scalar_str(sexpr_option(options, "results", "text"))?;
                    let result = ctx.apply(
                        evaluate,
                        &[(None, code), (None, results)],
                        call.err_call(),
                        call.env,
                    )?;
                    first_text(ctx, result).map(Option::unwrap_or_default)
                })?
            };
            tree::charge(ctx, call, remaining, value.len())?;
            node.tag = "TEXT".to_owned();
            node.body = Body::Text(value);
        }
    } else if let Body::Children(children) = &mut node.body {
        for child in children {
            evaluate_stage(ctx, call, child, stage, evaluate, remaining)?;
        }
    }
    Ok(())
}

pub(super) fn rd_to_text(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let fragment = flag(ctx, call, args.get(1))?;
    let options = Options::read(ctx, call, args.get(2), args.get(3))?;
    let encoding = first_text(ctx, args.get(5))?.unwrap_or_default();
    let mut root = tree::decode(ctx, call, args.get(0))?;
    if !fragment {
        let selected = strings(ctx, args.get(4))?;
        let evaluate = args.get(6);
        let mut remaining = ctx.host().limits().max_vector_bytes;
        for stage in ["build", "install", "render"] {
            if selected.iter().flatten().any(|name| name == stage) {
                evaluate_stage(ctx, call, &mut root, stage, evaluate, &mut remaining)?;
            }
        }
    }
    let remaining = ctx.host().limits().max_vector_bytes;
    let mut render = Renderer {
        ctx,
        call,
        options,
        lines: Vec::new(),
        remaining,
    };
    if fragment {
        render.prose(root.children().iter(), 0, 0, "")?;
        while render.lines.last().is_some_and(String::is_empty) {
            render.lines.pop();
        }
    } else {
        render.page(root.children())?;
    }

    if matches!(encoding.as_str(), "latin1" | "ISO-8859-1" | "ASCII") {
        let latin1 = encoding != "ASCII";
        let mut lines = Vec::new();
        for line in &render.lines {
            let bytes = line
                .chars()
                .map(|ch| {
                    if ch.is_ascii() || (latin1 && (ch as u32) <= 255) {
                        ch as u8
                    } else {
                        b'?'
                    }
                })
                .collect::<Vec<_>>();
            lines.push(
                render
                    .ctx
                    .session_mut()
                    .mk_char(&bytes, roxide_value::Encoding::Latin1)?,
            );
        }
        return Ok(render.ctx.session_mut().alloc_str_from(&lines)?);
    }
    chars(render.ctx, render.lines.iter().map(String::as_str))
}

struct Renderer<'a> {
    ctx: &'a mut Ctx,
    call: &'a Call,
    options: Options,
    lines: Vec<String>,
    remaining: usize,
}

impl Renderer<'_> {
    fn line(&mut self, line: String) -> EvalResult<()> {
        tree::charge(
            self.ctx,
            self.call,
            &mut self.remaining,
            line.len().saturating_add(size_of::<String>()),
        )?;
        self.lines.push(line);
        Ok(())
    }

    fn blank(&mut self) -> EvalResult<()> {
        if self
            .lines
            .last()
            .is_some_and(|line| !line.trim().is_empty())
        {
            self.line(String::new())?;
        }
        Ok(())
    }

    fn heading(&mut self, heading: &str, colon: bool) -> EvalResult<()> {
        let heading = heading.replace(['‘', '’'], "'").replace(['“', '”'], "\"");
        let heading = if self.options.underline {
            heading
                .chars()
                .map(|ch| {
                    if !ch.is_alphanumeric() {
                        ch.to_string()
                    } else {
                        format!("_\u{8}{ch}")
                    }
                })
                .collect()
        } else {
            heading
        };
        self.line(if colon {
            format!("{heading}:")
        } else {
            heading
        })?;
        self.blank()
    }

    fn page(&mut self, nodes: &[Node]) -> EvalResult<()> {
        let mut sections = Vec::new();
        gather_sections(nodes, &self.options, &mut sections);
        let names = sections
            .iter()
            .filter(|n| n.tag == "\\name")
            .collect::<Vec<_>>();
        let titles = sections
            .iter()
            .filter(|n| n.tag == "\\title")
            .collect::<Vec<_>>();
        if names.len() != 1 || titles.len() != 1 {
            let name = names
                .first()
                .map_or_else(|| "x".to_owned(), |node| self.inline(node.children()));
            return Err(self.ctx.stop(
                self.call.err_call(),
                format!(
                    "{name}: Sections \\title, and \\name must exist and be unique in Rd files"
                ),
            ));
        }
        let title = self.inline(titles[0].children());
        self.heading(&fold(&title), false)?;
        let indent = self.options.section_indent;
        for (tag, heading) in [
            ("\\description", "Description"),
            ("\\usage", "Usage"),
            ("\\arguments", "Arguments"),
            ("\\format", "Format"),
            ("\\details", "Details"),
            ("\\value", "Value"),
            ("\\section", ""),
            ("\\note", "Note"),
            ("\\author", "Author(s)"),
            ("\\source", "Source"),
            ("\\references", "References"),
            ("\\seealso", "See Also"),
            ("\\examples", "Examples"),
        ] {
            for node in sections
                .iter()
                .filter(|n| n.tag == tag)
                .take(if tag == "\\section" { usize::MAX } else { 1 })
            {
                if self.inline(node.children()).trim().is_empty()
                    && !node.children().iter().any(is_block)
                {
                    continue;
                }
                self.blank()?;
                if tag == "\\section" {
                    let title = fold(&self.inline(argument(node, 0)));
                    self.heading(&title, true)?;
                    self.prose(argument(node, 1).iter(), indent, indent, "")?;
                } else {
                    self.heading(heading, true)?;
                    match tag {
                        "\\usage" | "\\examples" => {
                            let code = self.code(node.children());
                            self.verbatim(&code, indent, true)?;
                        }
                        "\\arguments" | "\\value" => {
                            self.description(node.children(), indent, true)?
                        }
                        _ => self.prose(node.children().iter(), indent, indent, "")?,
                    }
                }
            }
        }
        self.blank()
    }

    fn inline(&self, nodes: &[Node]) -> String {
        let mut output = String::new();
        append_inline(nodes.iter(), &self.options, &mut output);
        output
    }

    fn prose<'a>(
        &mut self,
        nodes: impl IntoIterator<Item = &'a Node>,
        indent: usize,
        continuation: usize,
        prefix: &str,
    ) -> EvalResult<()> {
        let mut buffer = String::new();
        let mut prefix = prefix;
        let mut spacing = InlineSpacing::default();
        for node in nodes {
            if is_block(node) {
                self.paragraphs(&buffer, indent, continuation, prefix)?;
                buffer.clear();
                prefix = "";
                self.blank()?;
                self.block(node, indent)?;
                if node.tag != "\\tabular" {
                    self.blank()?;
                }
                continue;
            }
            spacing.append(node, &self.options, &mut buffer);
        }
        self.paragraphs(&buffer, indent, continuation, prefix)
    }

    fn paragraphs(
        &mut self,
        text: &str,
        indent: usize,
        continuation: usize,
        prefix: &str,
    ) -> EvalResult<()> {
        let mut paragraph = String::new();
        let mut initial = true;
        for line in text.split('\n') {
            if line.trim().is_empty() {
                if !paragraph.is_empty() {
                    self.wrap(
                        &paragraph,
                        indent,
                        continuation,
                        if initial { prefix } else { "" },
                    )?;
                    initial = false;
                    paragraph.clear();
                    self.blank()?;
                }
                continue;
            }
            if !paragraph.is_empty() {
                paragraph.push('\n');
            }
            paragraph.push_str(line);
        }
        if !paragraph.is_empty() {
            self.wrap(
                &paragraph,
                indent,
                continuation,
                if initial { prefix } else { "" },
            )?;
        }
        Ok(())
    }

    fn wrap(
        &mut self,
        text: &str,
        indent: usize,
        continuation: usize,
        prefix: &str,
    ) -> EvalResult<()> {
        let mut padding = indent;
        for (index, part) in text.split('\0').enumerate() {
            let prefix = if index == 0 { prefix } else { "" };
            let mut line = format!("{}{prefix}", " ".repeat(padding));
            let mut width = padding + prefix.chars().count();
            let mut started = false;
            for (word, spaces) in words(part) {
                let size = word.chars().count();
                if started && width + spaces + size >= self.options.width * 9 / 10 {
                    self.line(line)?;
                    line = " ".repeat(continuation);
                    width = continuation;
                    started = false;
                }
                if started {
                    line.push_str(if spaces == 2 { "  " } else { " " });
                    width += spaces;
                }
                line.push_str(word);
                width += size;
                started = true;
            }
            if started || index > 0 || !prefix.is_empty() {
                self.line(line)?;
            }
            padding = indent;
        }
        Ok(())
    }

    fn block(&mut self, node: &Node, indent: usize) -> EvalResult<()> {
        match node.tag.as_str() {
            "\\deqn" => {
                let equation = self.inline(argument(node, usize::from(node.children().len() > 1)));
                let equation = math_text(&equation);
                let width = self.options.width * 9 / 10;
                for line in equation.trim().lines() {
                    let space = width.saturating_sub(line.chars().count());
                    let left = space / 2 + indent.div_ceil(2);
                    let left = left.min(space);
                    self.line(format!(
                        "{}{line}{}",
                        " ".repeat(left),
                        " ".repeat(space - left)
                    ))?;
                }
                self.line(" ".repeat(indent))
            }
            "\\preformatted" => self.verbatim(&self.inline(node.children()), indent, true),
            "\\itemize" | "\\enumerate" => self.items(node, indent),
            "\\describe" => self.description(node.children(), indent, self.options.describe_inline),
            "\\tabular" => self.table(node, indent),
            _ => Ok(()),
        }
    }

    fn items(&mut self, node: &Node, indent: usize) -> EvalResult<()> {
        let mut groups: Vec<&[Node]> = Vec::new();
        let mut start = 0;
        for (i, child) in node.children().iter().enumerate() {
            if child.tag == "\\item" {
                if i != start {
                    groups.push(&node.children()[start..i]);
                }
                start = i + 1;
            }
        }
        if start < node.children().len() {
            groups.push(&node.children()[start..]);
        }
        let body_indent = self
            .options
            .min_indent
            .max(indent + self.options.extra_indent);
        for (index, group) in groups.into_iter().enumerate() {
            self.blank()?;
            let label = if node.tag == "\\itemize" {
                self.options.bullet.clone()
            } else if self.options.enumeration != SEXP_NIL {
                self.ctx.scoped(|ctx| {
                    let n = ctx.session_mut().scalar_int((index + 1) as i32)?;
                    let value =
                        ctx.apply(self.options.enumeration, &[(None, n)], SEXP_NIL, BASE_ENV)?;
                    Ok(first_text(ctx, value)?.unwrap_or_default())
                })?
            } else {
                format!("{}. ", index + 1)
            };
            self.prose(
                group.iter(),
                body_indent.saturating_sub(label.chars().count()),
                body_indent,
                &label,
            )?;
        }
        Ok(())
    }

    fn description(&mut self, nodes: &[Node], indent: usize, inline: bool) -> EvalResult<()> {
        let mut start = 0;
        let body_indent = self
            .options
            .min_indent
            .max(indent + self.options.section_extra);
        for (i, node) in nodes.iter().enumerate() {
            if node.tag != "\\item" {
                continue;
            }
            self.prose(nodes[start..i].iter(), indent, indent, "")?;
            self.blank()?;
            let label = fold(&self.inline(argument(node, 0)));
            if inline {
                let label = format!("{label}: ");
                self.prose(
                    argument(node, 1).iter(),
                    body_indent.saturating_sub(label.chars().count()),
                    body_indent,
                    &label,
                )?;
            } else {
                self.line(format!("{}{label}", " ".repeat(indent)))?;
                self.prose(argument(node, 1).iter(), body_indent, body_indent, "")?;
            }
            start = i + 1;
        }
        self.prose(nodes[start..].iter(), indent, indent, "")
    }

    fn table(&mut self, node: &Node, indent: usize) -> EvalResult<()> {
        let layout = self.inline(argument(node, 0));
        let mut rows = Vec::new();
        let mut row = Vec::new();
        let mut cell = String::new();
        for child in argument(node, 1) {
            match child.tag.as_str() {
                "\\tab" | "\\cr" => {
                    row.push(fold(&cell));
                    cell.clear();
                    if child.tag == "\\cr" {
                        rows.push(std::mem::take(&mut row));
                    }
                }
                _ => append_node(child, &self.options, &mut cell),
            }
        }
        if !row.is_empty() || !cell.trim().is_empty() {
            row.push(fold(&cell));
            rows.push(row);
        }
        let columns = layout.chars().collect::<Vec<_>>();
        if rows.iter().any(|row| row.len() > columns.len()) {
            let line = node.span.map_or(1, |span| span.integers()[0]);
            return Err(self.ctx.stop(
                self.call.err_call(),
                format!("Rd:{line}: too many columns for format '{layout}'"),
            ));
        }
        let mut widths = vec![0usize; columns.len()];
        for row in &rows {
            for (cell, width) in row.iter().zip(&mut widths) {
                *width = (*width).max(cell.chars().count());
            }
        }
        for row in rows {
            let mut line = " ".repeat(indent + self.options.section_extra);
            for (i, (&column, &width)) in columns.iter().zip(&widths).enumerate() {
                let cell = row.get(i).map_or("", String::as_str);
                let pad = width.saturating_sub(cell.chars().count());
                let left = match column {
                    'r' => pad,
                    'c' => pad / 2,
                    _ => 0,
                };
                line.push_str(&" ".repeat(left));
                line.push_str(cell);
                line.push_str(&" ".repeat(pad - left + if i + 1 == columns.len() { 1 } else { 2 }));
            }
            self.line(line)?;
        }
        self.line(" ".repeat(indent + 1))
    }

    fn verbatim(&mut self, text: &str, indent: usize, trailing_blank: bool) -> EvalResult<()> {
        let lines = text.split('\n').collect::<Vec<_>>();
        let first = lines.iter().position(|line| !line.trim().is_empty());
        let last = lines.iter().rposition(|line| !line.trim().is_empty());
        if let (Some(first), Some(last)) = (first, last) {
            let padding = " ".repeat(indent);
            for line in &lines[first..=last] {
                self.line(format!("{padding}{line}"))?;
            }
            if trailing_blank {
                self.line(padding)?;
            }
        }
        Ok(())
    }

    fn code(&self, nodes: &[Node]) -> String {
        let mut text = String::new();
        for node in nodes {
            match node.tag.as_str() {
                "\\dontshow" | "\\testonly" => {}
                "\\dontrun" => {
                    let content = self.inline(node.children());
                    if content.trim().contains('\n') || content.starts_with('\n') {
                        text.push_str("## Not run:\n");
                        text.push_str(content.trim_matches('\n'));
                        text.push_str("\n## End(Not run)\n");
                    } else {
                        text.push_str("## Not run: ");
                        text.push_str(&content);
                    }
                }
                "\\method" | "\\S3method" | "\\S4method" => {
                    let class = self.inline(argument(node, 1));
                    let func = self.inline(argument(node, 0));
                    let method = if node.tag == "\\S4method" {
                        "S4 method for signature"
                    } else {
                        "S3 method for class"
                    };
                    text.push_str(&format!("## {method} '{class}'\n{func}"));
                }
                "COMMENT" => {}
                _ => append_node(node, &self.options, &mut text),
            }
        }
        text
    }
}

fn gather_sections<'a>(nodes: &'a [Node], options: &Options, out: &mut Vec<&'a Node>) {
    for node in nodes {
        if matches!(node.tag.as_str(), "#ifdef" | "#ifndef") {
            if options.active(node) {
                gather_sections(argument(node, 1), options, out);
            }
        } else if node.tag.starts_with('\\') {
            out.push(node);
        }
    }
}

fn is_block(node: &Node) -> bool {
    matches!(
        node.tag.as_str(),
        "\\deqn" | "\\preformatted" | "\\itemize" | "\\enumerate" | "\\describe" | "\\tabular"
    )
}

fn argument(node: &Node, index: usize) -> &[Node] {
    node.children().get(index).map_or(&[], Node::children)
}

fn append_inline<'a>(
    nodes: impl IntoIterator<Item = &'a Node>,
    options: &Options,
    out: &mut String,
) {
    let mut spacing = InlineSpacing {
        start: out.len(),
        ..InlineSpacing::default()
    };
    for node in nodes {
        spacing.append(node, options, out);
    }
}

#[derive(Default)]
struct InlineSpacing {
    start: usize,
    skip_newline: bool,
}

impl InlineSpacing {
    fn append(&mut self, node: &Node, options: &Options, out: &mut String) {
        if node.tag == "COMMENT" {
            let line = out[self.start..].rsplit('\n').next().unwrap_or_default();
            if line.trim().is_empty() {
                out.truncate(out.len() - line.len());
                self.skip_newline = true;
            }
            return;
        }
        let offset = out.len();
        append_node(node, options, out);
        if self.skip_newline && out[offset..].starts_with('\n') {
            out.remove(offset);
        }
        self.skip_newline = false;
    }
}

fn append_node(node: &Node, options: &Options, out: &mut String) {
    let children = node.children();
    match node.tag.as_str() {
        "TEXT" | "RCODE" | "VERB" | "UNKNOWN" => out.push_str(node.text()),
        "COMMENT" | "USERMACRO" | "\\newcommand" | "\\renewcommand" | "\\dontshow"
        | "\\testonly" => {}
        "\\R" => out.push('R'),
        "\\dots" | "\\ldots" => out.push_str("..."),
        "\\cr" => out.push('\0'),
        "\\tab" => out.push(' '),
        "\\code" if options.code_quote => surrounded(children, options, out, '‘', '’'),
        "\\sQuote" | "\\pkg" | "\\file" | "\\env" | "\\option" | "\\command" | "\\cite" => {
            surrounded(children, options, out, '‘', '’')
        }
        "\\dQuote" => surrounded(children, options, out, '“', '”'),
        "\\emph" => surrounded(children, options, out, '_', '_'),
        "\\bold" | "\\strong" => surrounded(children, options, out, '*', '*'),
        "\\enc" => append_inline(argument(node, 0), options, out),
        "\\eqn" | "\\deqn" => {
            let mut content = String::new();
            append_inline(
                argument(node, usize::from(children.len() > 1)),
                options,
                &mut content,
            );
            out.push_str(math_text(content.trim()).as_str());
        }
        "\\verb" => {
            let mut text = String::new();
            append_inline(children, options, &mut text);
            out.extend(text.chars().map(|c| if c == '\n' { '\0' } else { c }));
        }
        "\\url" | "\\email" => url(children, options, out, node.tag == "\\email"),
        "\\href" => {
            append_inline(argument(node, 1).iter(), options, out);
            if options.show_urls {
                out.push(' ');
                url(argument(node, 0), options, out, false);
            }
        }
        "\\if" | "\\ifelse" => {
            let mut target = String::new();
            append_inline(argument(node, 0), options, &mut target);
            let index = if target
                .split(|ch: char| ch == ',' || ch.is_whitespace())
                .any(|x| x == "text")
            {
                1
            } else {
                2
            };
            append_inline(argument(node, index), options, out);
        }
        "#ifdef" | "#ifndef" => {
            if options.active(node) {
                append_inline(argument(node, 1), options, out);
            }
        }
        "\\Sexpr" => {
            out.push_str("\\Sexpr");
            if let Some(option) = &node.option {
                out.push('[');
                out.push_str(option.text());
                out.push(']');
            }
            out.push('{');
            append_inline(children, options, out);
            out.push('}');
            out.push('\0');
        }
        _ => append_inline(children, options, out),
    }
}

fn math_text(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some((prefix, suffix)) = rest.split_once('\\') {
        out.push_str(prefix);
        let length = suffix.bytes().take_while(u8::is_ascii_alphabetic).count();
        let command = &suffix[..length];
        match command {
            "alpha" | "beta" | "gamma" | "delta" | "epsilon" | "zeta" | "eta" | "theta"
            | "iota" | "kappa" | "lambda" | "mu" | "nu" | "xi" | "omicron" | "pi" | "rho"
            | "sigma" | "tau" | "upsilon" | "phi" | "chi" | "psi" | "omega" | "Gamma" | "Delta"
            | "Theta" | "Lambda" | "Xi" | "Pi" | "Sigma" | "Upsilon" | "Phi" | "Psi" | "Omega"
            | "sqrt" => out.push_str(command),
            "infty" => out.push_str("Inf"),
            "left" | "right" => {}
            _ => {
                out.push('\\');
                out.push_str(command);
            }
        }
        rest = &suffix[length..];
    }
    out.push_str(rest);
    out
}

fn surrounded(nodes: &[Node], options: &Options, out: &mut String, left: char, right: char) {
    out.push(left);
    append_inline(nodes, options, out);
    out.push(right);
}

fn url(nodes: &[Node], options: &Options, out: &mut String, email: bool) {
    let mut text = String::new();
    append_inline(nodes, options, &mut text);
    out.push('<');
    if email {
        out.push_str("mailto:");
    }
    for ch in text.trim().chars() {
        match ch {
            ' ' => out.push_str("%20"),
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            _ => out.push(ch),
        }
    }
    out.push('>');
}

fn fold(text: &str) -> String {
    let mut result = String::new();
    for (word, spaces) in words(text) {
        if !result.is_empty() {
            result.push_str(if spaces == 2 { "  " } else { " " });
        }
        result.push_str(word);
    }
    result
}

fn words(text: &str) -> impl Iterator<Item = (&str, usize)> {
    let mut rest = text;
    let mut sentence = false;
    std::iter::from_fn(move || {
        let trimmed = rest.trim_start_matches(char::is_whitespace);
        if trimmed.is_empty() {
            return None;
        }
        let gap = &rest[..rest.len() - trimmed.len()];
        let spaces = if sentence && (gap.chars().count() > 1 || gap.contains('\t')) {
            2
        } else {
            1
        };
        let end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
        let word = &trimmed[..end];
        sentence = word.ends_with(['.', '!', '?']);
        rest = &trimmed[end..];
        Some((word, spaces))
    })
}
