//! Summarize the structure of an R object.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod atomic;

use std::collections::HashSet;

use roxide_base::scan::{base_call, r_strings};
use roxide_eval::{
    ArgEval, Args, Arity, Call, Ctx, Dispatch, EvalResult, Module, PrimDef, PrimKind, Visibility,
};
use roxide_gc::SexpId;
use roxide_value::{
    BASE_ENV, BASE_NAMESPACE, EMPTY_ENV, GLOBAL_ENV, SEXP_NIL, SexpType, ValueApi, encode_string,
};

/// Register the utils structure display functions.
pub const MODULE: Module = Module {
    prims: &[PrimDef {
        name: "C_rox_str",
        kind: PrimKind::Internal,
        args: ArgEval::Eval,
        arity: Arity::Exact(15),
        dispatch: Dispatch::None,
        visibility: Visibility::Off,
        fun: display,
    }],
    prelude: r#"
str <- function(object, ...) UseMethod("str")
str.default <- function(object, max.level = NA, vec.len = 4L, digits.d = 3L,
                        nchar.max = 128L, give.attr = TRUE, drop.deparse.attr = TRUE,
                        give.head = TRUE, give.length = give.head, width = getOption("width"),
                        nest.lev = 0L, indent.str = " ", comp.str = "$ ", no.list = FALSE,
                        envir = baseenv(), strict.width = "no", formatNum = NULL,
                        list.len = 99L, deparse.lines = NULL, ...)
    .Internal(C_rox_str(object, max.level, vec.len, digits.d, nchar.max, give.attr,
                        give.head, give.length, width, nest.lev, indent.str, comp.str,
                        no.list, list.len, strict.width))
str.data.frame <- str.POSIXt <- str.Date <- str.default
"#,
};

fn display(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let options = Options::new(ctx, call, args)?;
    let limits = ctx.host().limits();
    let bound = limits.max_vector_bytes;
    let max_depth = limits.max_expressions.min(ctx.opts().expressions).min(128);
    let mut display = Display {
        ctx,
        call,
        options,
        bound,
        max_depth,
        text: String::new(),
        path: HashSet::new(),
    };
    display.value(args.get(0), "", 0, false)?;
    display
        .ctx
        .host()
        .write_out(display.text.as_bytes())
        .map_err(|err| display.ctx.stop(call.err_call(), err.to_string()))?;
    Ok(SEXP_NIL)
}

struct Options {
    level: usize,
    vector: usize,
    digits: usize,
    nchar: usize,
    attributes: bool,
    head: bool,
    length: bool,
    width: usize,
    nest: usize,
    indent: String,
    component: String,
    no_list: bool,
    list: usize,
    strict: String,
}

impl Options {
    fn new(ctx: &mut Ctx, call: &Call, args: &Args) -> EvalResult<Self> {
        let mut number = |i, missing| -> EvalResult<usize> {
            let value = ctx.session_mut().coerce(args.get(i), SexpType::Real)?.0;
            let value = ctx.session().real(value)?.iter().next().unwrap_or(f64::NAN);
            Ok(if value.is_nan() {
                missing
            } else {
                value.max(0.0) as usize
            })
        };
        let level = number(1, usize::MAX)?;
        let vector = number(2, 4)?;
        let digits = number(3, 3)?.clamp(1, 22);
        let nchar = number(4, 128)?;
        let width = number(8, 80)?;
        let nest = number(9, 0)?.min(128);
        let list = number(13, 99)?;
        let mut flag = |i| -> EvalResult<bool> {
            let value = ctx.session_mut().coerce(args.get(i), SexpType::Lgl)?.0;
            let values = ctx.session().lgl(value)?;
            let flag = values.iter().next().and_then(|v| v.get());
            flag.ok_or_else(|| ctx.stop(call.err_call(), "missing value where TRUE/FALSE needed"))
        };
        let attributes = flag(5)?;
        let head = flag(6)?;
        let length = flag(7)?;
        let no_list = flag(12)?;
        let mut text = |i, default: &str| -> EvalResult<String> {
            Ok(r_strings(ctx, args.get(i))?
                .into_iter()
                .next()
                .flatten()
                .unwrap_or_else(|| default.into()))
        };
        let indent = text(10, " ")?;
        let component = text(11, "$ ")?;
        let strict = text(14, "no")?;
        if !matches!(strict.as_str(), "no" | "cut" | "wrap") {
            return Err(ctx.stop(call.err_call(), "invalid 'strict.width'"));
        }
        Ok(Self {
            level,
            vector,
            digits,
            nchar,
            attributes,
            head,
            length,
            width,
            nest,
            indent,
            component,
            no_list,
            list,
            strict,
        })
    }
}

struct Display<'a> {
    ctx: &'a mut Ctx,
    call: &'a Call,
    options: Options,
    bound: usize,
    max_depth: usize,
    text: String,
    path: HashSet<SexpId>,
}

impl Display<'_> {
    fn line(&mut self, prefix: &str, content: &str) -> EvalResult<()> {
        let size = prefix.len().saturating_add(content.len()).saturating_add(1);
        if size > self.bound.saturating_sub(self.text.len()) {
            return Err(self
                .ctx
                .stop(self.call.err_call(), "vector size limit exceeded"));
        }
        let mut line = format!("{prefix}{content}");
        let width = self.options.width;
        match self.options.strict.as_str() {
            "cut" if line.chars().count() > width => {
                let end = line
                    .char_indices()
                    .nth(width.saturating_sub(2))
                    .map_or(line.len(), |(i, _)| i);
                line.truncate(end);
                line.push_str("..");
            }
            "wrap" => {
                let mut wrapped = String::new();
                let mut used = 0;
                for word in line.split_whitespace() {
                    let len = word.chars().count();
                    if used != 0 && used + 1 + len >= width {
                        wrapped.push_str("\n   ");
                        used = 3;
                    } else if used != 0 {
                        wrapped.push(' ');
                        used += 1;
                    }
                    wrapped.push_str(word);
                    used += len;
                }
                line = wrapped;
            }
            _ => {}
        }
        if line.len().saturating_add(1) > self.bound.saturating_sub(self.text.len()) {
            return Err(self
                .ctx
                .stop(self.call.err_call(), "vector size limit exceeded"));
        }
        self.text.push_str(&line);
        self.text.push('\n');
        Ok(())
    }

    fn value(&mut self, value: SexpId, prefix: &str, depth: usize, column: bool) -> EvalResult<()> {
        if depth >= self.max_depth || !self.path.insert(value) {
            return self.line(prefix, " ...");
        }
        let result = self.render(value, prefix, depth, column);
        self.path.remove(&value);
        result
    }

    fn render(&mut self, x: SexpId, prefix: &str, depth: usize, column: bool) -> EvalResult<()> {
        let ty = self.ctx.session().type_of(x)?;
        let class = self.attr(x, "class")?;
        let first_class = if class == SEXP_NIL {
            None
        } else {
            r_strings(self.ctx, class)?.into_iter().next().flatten()
        };
        let class = first_class.as_deref();
        let frame = class == Some("data.frame");
        let datetime = matches!(class, Some("Date" | "POSIXct" | "POSIXlt"));
        let factor = matches!(class, Some("factor" | "ordered"));
        let dim = self.attr(x, "dim")?;
        let names = self.attr(x, "names")?;

        match ty {
            SexpType::Vec | SexpType::List if !datetime => {
                self.container(x, prefix, depth, frame, ty, names)?;
            }
            SexpType::Lgl
            | SexpType::Int
            | SexpType::Real
            | SexpType::Str
            | SexpType::Cplx
            | SexpType::Raw
            | SexpType::Vec
                if datetime =>
            {
                let content = self.datetime(x, class.unwrap_or("Date"), column)?;
                self.line(prefix, &content)?;
            }
            SexpType::Lgl
            | SexpType::Int
            | SexpType::Real
            | SexpType::Str
            | SexpType::Cplx
            | SexpType::Raw => {
                let content = self.atomic(x, ty, class, dim, names, column)?;
                self.line(prefix, &content)?;
            }
            SexpType::Nil => return self.line(prefix, " NULL"),
            SexpType::Env => {
                let content = self.environment(x);
                self.line(prefix, &content)?;
            }
            SexpType::Clo | SexpType::Builtin | SexpType::Special => {
                let content = self.function(x)?;
                self.line(prefix, &content)?;
            }
            _ => {
                let text = self.ctx.deparse_lines(x, Default::default())?.join(" ");
                let head = if !self.options.head {
                    String::new()
                } else if ty == SexpType::Expr {
                    "  ".into()
                } else {
                    format!(" {} ", ty.type_name(false))
                };
                self.line(prefix, &format!("{head}{text}"))?;
            }
        }
        if self.options.attributes {
            for (key, attr) in self.ctx.session().attrs(x)? {
                let key = self.ctx.session().sym_name(key)?;
                let implicit = match key {
                    "srcref" => true,
                    "dim" => !matches!(ty, SexpType::Vec | SexpType::List),
                    "names" => matches!(ty, SexpType::Vec | SexpType::List),
                    "class" => frame || ty != SexpType::Vec || datetime,
                    "row.names" => frame,
                    "levels" => factor,
                    "tzone" | "balanced" => datetime,
                    _ => false,
                };
                if !implicit {
                    let prefix = format!(
                        "{}- attr(*, {})=",
                        self.indent(depth),
                        encode_string(key.as_bytes())
                    );
                    self.value(attr, &prefix, depth + 1, false)?;
                }
            }
        }
        Ok(())
    }

    fn container(
        &mut self,
        x: SexpId,
        prefix: &str,
        depth: usize,
        frame: bool,
        ty: SexpType,
        names: SexpId,
    ) -> EvalResult<()> {
        let n = self.ctx.session().xlength(x)?;
        if n == 0 && !frame {
            return self.line(
                prefix,
                if ty == SexpType::List {
                    " Dotted pair list of 0"
                } else {
                    " list()"
                },
            );
        }
        let titled = frame || !self.options.no_list || depth != 0 || !prefix.is_empty();
        if titled {
            let title = if frame {
                let s = self.ctx.session_mut();
                let row_names = s.attr_row_names(x)?;
                let rows = s.xlength(row_names)?;
                let suffix = if n == 1 { "" } else { "s" };
                format!("'data.frame':\t{rows} obs. of  {n} variable{suffix}:")
            } else if ty == SexpType::List {
                format!("Dotted pair list of {n}")
            } else {
                format!("List of {n}")
            };
            self.line(prefix, &title)?;
        }
        if depth.saturating_add(self.options.nest) >= self.options.level {
            return Ok(());
        }
        let names = if ty == SexpType::List {
            let s = self.ctx.session();
            s.pairlist_iter(x)
                .map(|(tag, _)| {
                    tag.map(|tag| s.sym_name(tag).map(str::to_owned))
                        .transpose()
                })
                .collect::<Result<Vec<_>, _>>()?
        } else {
            r_strings(self.ctx, names)?
        };
        let width = names
            .iter()
            .flatten()
            .map(|name| name.chars().count())
            .max()
            .unwrap_or(0);
        let indent = self.indent(depth);
        let count = n.min(self.options.list);
        if count > self.bound / size_of::<SexpId>() {
            return Err(self
                .ctx
                .stop(self.call.err_call(), "vector size limit exceeded"));
        }
        let s = self.ctx.session();
        let children = if ty == SexpType::List {
            s.pairlist_iter(x)
                .take(count)
                .map(|(_, value)| value)
                .collect::<Vec<_>>()
        } else {
            s.list_slice(x)?.iter().take(count).copied().collect()
        };
        for (i, child) in children.into_iter().enumerate() {
            let name = names.get(i).and_then(Option::as_deref).unwrap_or("");
            let component = if depth == 0 {
                &self.options.component
            } else {
                "$ "
            };
            let line = format!("{indent}{component}{name:width$}:");
            self.value(child, &line, depth + 1, frame && depth == 0)?;
        }
        if count < n {
            self.line(&indent, " [list output truncated]")?;
        }
        Ok(())
    }

    fn indent(&self, depth: usize) -> String {
        format!("{}{}", self.options.indent, " ..".repeat(depth))
    }

    fn attr(&mut self, object: SexpId, name: &str) -> EvalResult {
        let key = self.ctx.session_mut().intern(name)?;
        Ok(self.ctx.session().attr(object, key))
    }

    fn environment(&self, x: SexpId) -> String {
        if !self.options.head {
            return String::new();
        }
        match x {
            BASE_ENV => "<environment: base> ".into(),
            BASE_NAMESPACE => "<environment: namespace:base> ".into(),
            GLOBAL_ENV => "<environment: R_GlobalEnv> ".into(),
            EMPTY_ENV => "<environment: R_EmptyEnv> ".into(),
            _ => format!("<environment: 0x{:x}> ", x.index()),
        }
    }

    fn function(&self, x: SexpId) -> EvalResult<String> {
        if let Some(name) = self.ctx.primitive_name(x) {
            let arguments = match name {
                "sum" | "prod" | "min" | "max" | "any" | "all" => "..., na.rm = FALSE",
                "+" | "-" | "*" | "/" | "^" | "%%" | "%/%" | "==" | "!=" | "<" | "<=" | ">"
                | ">=" | "&" | "|" => "e1, e2",
                "length" | "is.null" | "sqrt" | "abs" | "exp" => "x",
                "log" => "x, base = exp(1)",
                "c" => "...",
                _ => return Ok(format!(".Primitive(\"{name}\") ")),
            };
            return Ok(format!("function ({arguments})  "));
        }
        let lines = self.ctx.deparse_lines(x, Default::default())?;
        let count = lines
            .iter()
            .position(|line| line.trim_end().ends_with(')'))
            .map_or(1, |i| i + 1);
        let formals = lines[..count.min(lines.len())].join("\n");
        Ok(format!("{}  ", formals.trim_end()))
    }

    fn datetime(&mut self, x: SexpId, class: &str, column: bool) -> EvalResult<String> {
        let formatted = base_call(self.ctx, self.call, "format", &[x])?;
        let s = self.ctx.session();
        let len = s.xlength(formatted)?;
        if len == 0 {
            return Ok(format!(" '{class}' num(0) "));
        }
        let max = self.options.vector.saturating_mul(5) / 4;
        let length = if self.options.length && !column {
            format!("[1:{len}]")
        } else {
            String::new()
        };
        let mut content = format!(" {class}{length}, format:");
        self.characters(&mut content, formatted, max, true)?;
        Ok(content)
    }
}
