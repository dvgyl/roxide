//! Base text input and shared table field parsing.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod fields;
mod input;
mod names;
/// Numeric field parsing for scanners and type conversion.
pub mod number;

use roxide_eval::{
    ArgEval, Args, Arity, Call, Ctx, Dispatch, EvalResult, Module, PrimDef, PrimFn, PrimKind,
    Visibility,
};
use roxide_gc::SexpId;
use roxide_value::{
    CharId, Encoding, NA_INTEGER, NA_REAL, RComplex, RLogical, SEXP_NIL, SexpType, ValueApi,
};

pub use fields::{Dialect, Fields, Position, Record};
pub use input::{InputOptions, base_call, r_strings};

use crate::ops::common::{flag, list, number as arg_number, set_names, vector_limit};

/// Register the base scanner and syntactic name repair.
pub const MODULE: Module = Module {
    prims: &[
        internal("C_rox_scan", 18, scan),
        internal("C_rox_make_names", 3, names::make_names),
    ],
    prelude: r#"
scan <- function(file = "", what = double(), nmax = -1L, n = -1L, sep = "",
                 quote = if (identical(sep, "\n")) "" else "'\"", dec = ".",
                 skip = 0L, nlines = 0L, na.strings = "NA", flush = FALSE,
                 fill = FALSE, strip.white = FALSE, quiet = FALSE, blank.lines.skip = TRUE,
                 multi.line = TRUE, comment.char = "", allowEscapes = FALSE,
                 fileEncoding = "", encoding = "unknown", text, skipNul = FALSE) {
    if (!missing(nmax) && !missing(n)) stop("either specify 'nmax' or 'n', but not both.")
    if (missing(nmax)) nmax <- n
    if (missing(text) || !identical(file, "")) text <- NULL
    if (is.null(text) && (identical(file, "") || identical(file, "stdin"))) file <- stdin()
    if (fileEncoding != "") encoding <- fileEncoding
    .Internal(C_rox_scan(file, what, nmax, sep, quote, dec, skip, nlines, na.strings,
                         flush, fill, strip.white, quiet, blank.lines.skip, multi.line,
                         comment.char, allowEscapes, list(text, encoding, skipNul)))
}
make.names <- function(names, unique = FALSE, allow_ = TRUE)
    .Internal(C_rox_make_names(names, unique, allow_))
"#,
};

const fn internal(name: &'static str, n: usize, fun: PrimFn) -> PrimDef {
    PrimDef {
        name,
        kind: PrimKind::Internal,
        args: ArgEval::Eval,
        arity: Arity::Exact(n),
        dispatch: Dispatch::None,
        visibility: Visibility::On,
        fun,
    }
}

/// Parse separator, quote, comment, escape, and blank-line options.
pub fn dialect(ctx: &mut Ctx, call: &Call, args: [SexpId; 5]) -> EvalResult<Dialect> {
    let [sep, quote, comment, escapes, blank] = args;
    Ok(Dialect {
        separator: mark(ctx, call, sep, "sep")?,
        quotes: if quote == SEXP_NIL {
            String::new()
        } else {
            input::string(ctx, call, quote, "quote")?
        },
        comment: mark(ctx, call, comment, "comment.char")?,
        escapes: flag(ctx, escapes, false)?,
        skip_blank: flag(ctx, blank, true)?,
    })
}

/// Parse a decimal mark.
pub fn decimal(ctx: &mut Ctx, call: &Call, value: SexpId) -> EvalResult<char> {
    let (value, _) = ctx.session_mut().coerce(value, SexpType::Str)?;
    let text = crate::ops::common::string(ctx.session(), value);
    if text.as_deref() == Some("") {
        return Err(ctx.stop(call.err_call(), "invalid 'dec' argument"));
    }
    Ok(text.and_then(|s| s.chars().next()).unwrap_or('.'))
}

fn mark(ctx: &mut Ctx, call: &Call, value: SexpId, name: &str) -> EvalResult<Option<char>> {
    let text = input::string(ctx, call, value, name)?;
    let mut chars = text.chars();
    let ch = chars.next();
    if chars.next().is_some() {
        return Err(ctx.stop(call.err_call(), format!("invalid '{name}' argument")));
    }
    Ok(ch)
}

fn positive(ctx: &mut Ctx, value: SexpId) -> EvalResult<usize> {
    Ok(arg_number(ctx.session_mut(), value)?.max(0.0) as usize)
}

fn scan(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let what = args.get(1);
    let is_list = ctx.session().type_of(what)? == SexpType::Vec;
    let types = if is_list {
        let s = ctx.session();
        (0..s.xlength(what)?)
            .map(|i| s.type_of(s.list_get(what, i)?))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        vec![ctx.session().type_of(what)?]
    };
    if types.iter().any(|ty| {
        !matches!(
            ty,
            SexpType::Lgl
                | SexpType::Int
                | SexpType::Real
                | SexpType::Cplx
                | SexpType::Str
                | SexpType::Raw
                | SexpType::Nil
        )
    }) || (!is_list && types.first() == Some(&SexpType::Nil))
    {
        return Err(ctx.stop(call.err_call(), "invalid 'what' argument"));
    }
    if types.is_empty() {
        return Err(ctx.stop(call.err_call(), "empty 'what' specified"));
    }
    let strip_len = ctx.session().xlength(args.get(11))?;
    if strip_len > 1 && strip_len != types.len() {
        return Err(ctx.stop(call.err_call(), "invalid 'strip.white' length"));
    }
    let strip = strips(ctx, args.get(11), types.len())?;
    let dialect = dialect(
        ctx,
        call,
        [
            args.get(3),
            args.get(4),
            args.get(15),
            args.get(16),
            args.get(13),
        ],
    )?;
    let dec = decimal(ctx, call, args.get(5))?;
    let skip = positive(ctx, args.get(6))?;
    let nlines = positive(ctx, args.get(7))?;
    let limit = positive(ctx, args.get(2))?;
    let missing = r_strings(ctx, args.get(8))?;
    let quiet = flag(ctx, args.get(12), false)?;
    let settings = RowOptions {
        max_rows: limit,
        nlines,
        flush: flag(ctx, args.get(9), false)?,
        fill: !is_list || flag(ctx, args.get(10), false)?,
        multi_line: flag(ctx, args.get(14), true)?,
        flush_at_limit: is_list,
    };
    let options = args.get(17);
    let text = ctx.session().list_get(options, 0)?;
    let enc_value = ctx.session().list_get(options, 1)?;
    let nul_value = ctx.session().list_get(options, 2)?;
    let encoding = input::string(ctx, call, enc_value, "encoding")?;
    let skip_nul = flag(ctx, nul_value, false)?;
    let options = InputOptions {
        skip,
        encoding: &encoding,
        skip_nul,
    };
    let mut lexer = Fields::open(ctx, call, args.get(0), text, options, &dialect)?;
    let columns = read_columns(ctx, call, &mut lexer, &types, &strip, settings);
    lexer.finish(ctx);
    let columns = columns?;
    let count = columns.first().map_or(0, Vec::len);
    let result = ctx.scoped(|ctx| {
        let mut values = Vec::new();
        for (&ty, fields) in types.iter().zip(columns) {
            let value = scan_column(ctx, call, fields, ty, &missing, dec)?;
            ctx.protect(value);
            values.push(value);
        }
        if !is_list {
            return Ok(values.first().copied().unwrap_or(SEXP_NIL));
        }
        let value = list(ctx.session_mut(), &values)?;
        let names = crate::ops::common::names(ctx.session(), what).to_vec();
        if !names.is_empty() {
            set_names(ctx.session_mut(), value, &names)?;
        }
        Ok(value)
    })?;
    if !quiet {
        let unit = if is_list { "record" } else { "item" };
        let suffix = if count == 1 { "" } else { "s" };
        ctx.host()
            .write_err(format!("Read {count} {unit}{suffix}\n").as_bytes())
            .map_err(|e| ctx.stop(call.err_call(), e.to_string()))?;
    }
    Ok(result)
}

/// Expand one whitespace flag or recycle flags across the columns.
pub fn strips(ctx: &mut Ctx, value: SexpId, width: usize) -> EvalResult<Vec<bool>> {
    let (value, _) = ctx.session_mut().coerce(value, SexpType::Lgl)?;
    let values = ctx.session().lgl(value)?;
    Ok((0..width)
        .map(|i| !values.is_empty() && values.get(i % values.len()).get() == Some(true))
        .collect())
}

/// Record assembly options for a scan with a fixed width.
pub struct RowOptions {
    /// Maximum number of result records. Zero removes the limit.
    pub max_rows: usize,
    /// Maximum number of physical input lines. Zero removes the limit.
    pub nlines: usize,
    /// Discard extra fields after a complete record on a line.
    pub flush: bool,
    /// Complete short records with empty fields.
    pub fill: bool,
    /// Permit a record to continue on the next line.
    pub multi_line: bool,
    /// Flush the final record when the record limit is reached.
    pub flush_at_limit: bool,
}

/// Read a scan into columns. The types control quoting and whitespace.
pub fn read_columns(
    ctx: &mut Ctx,
    call: &Call,
    lexer: &mut Fields<'_>,
    types: &[SexpType],
    strip: &[bool],
    options: RowOptions,
) -> EvalResult<Vec<Vec<String>>> {
    let width = types.len();
    vector_limit(ctx, call, width, size_of::<Vec<String>>())?;
    let mut columns = vec![Vec::new(); width];
    if width == 0 {
        return Ok(columns);
    }
    let bound = ctx.host().limits().max_vector_bytes;
    let mut used = 0usize;
    let mut column = 0;
    let mut rows = 0;
    while (options.max_rows == 0 || rows < options.max_rows)
        && (options.nlines == 0 || lexer.scan_line() <= options.nlines)
    {
        let Some(record) = lexer.record_until(ctx, call, width - column, options.nlines, |i| {
            let j = column + i;
            (
                matches!(types[j], SexpType::Str | SexpType::Nil),
                strip.get(j) == Some(&true),
            )
        })?
        else {
            break;
        };
        if record.unclosed_quote {
            ctx.warn_now(call.err_call(), "EOF within quoted string")?;
        }
        for field in record.fields {
            used = used.saturating_add(field.len() + size_of::<String>());
            columns[column].push(field);
            column += 1;
        }
        if column == width {
            rows += 1;
            column = 0;
            if options.flush
                && !record.line_ended
                && (options.flush_at_limit || options.max_rows == 0 || rows < options.max_rows)
            {
                lexer.flush_line();
            }
        } else if options.fill {
            used = used.saturating_add((width - column) * size_of::<String>());
            for fields in &mut columns[column..] {
                fields.push(String::new());
            }
            rows += 1;
            column = 0;
        } else if !options.multi_line {
            return Err(ctx.stop(
                call.err_call(),
                format!("line {} did not have {width} elements", record.end_line),
            ));
        }
        if used > bound {
            return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
        }
    }
    if column != 0 {
        vector_limit(
            ctx,
            call,
            used.saturating_add((width - column) * size_of::<String>()),
            1,
        )?;
        for fields in &mut columns[column..] {
            fields.push(String::new());
        }
    }
    Ok(columns)
}

/// Convert a column with the explicit scanner rules for its atomic type.
pub fn scan_column(
    ctx: &mut Ctx,
    call: &Call,
    fields: Vec<String>,
    ty: SexpType,
    missing: &[Option<String>],
    decimal: char,
) -> EvalResult {
    let is_missing = |text: &str| missing.iter().any(|m| m.as_deref() == Some(text));
    macro_rules! parse {
        ($alloc:ident, $na:expr, $name:literal, $parser:expr) => {{
            let mut values = Vec::with_capacity(fields.len());
            for field in fields {
                let field = field.split('\0').next().unwrap_or_default();
                let value = if field.is_empty() || is_missing(field) {
                    $na
                } else {
                    $parser(field).ok_or_else(|| {
                        ctx.stop(
                            call.err_call(),
                            format!("scan() expected '{}', got '{}'", $name, field),
                        )
                    })?
                };
                values.push(value);
            }
            Ok(ctx.session_mut().$alloc(&values)?)
        }};
    }
    match ty {
        SexpType::Str => {
            let s = ctx.session_mut();
            let chars = fields
                .iter()
                .map(|field| {
                    let field = field.split('\0').next().unwrap_or_default();
                    if is_missing(field) {
                        Ok(CharId::NA)
                    } else {
                        s.mk_char(field.as_bytes(), Encoding::Utf8)
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(s.alloc_str_from(&chars)?)
        }
        SexpType::Int => parse!(alloc_int_from, NA_INTEGER, "an integer", number::integer),
        SexpType::Real => parse!(alloc_real_from, NA_REAL, "a real", |f| number::real(
            f, decimal
        )
        .map(|x| x.0)),
        SexpType::Lgl => parse!(alloc_lgl_from, RLogical::NA, "a logical", |f| {
            number::logical(f, true).map(|x| RLogical::from_option(Some(x)))
        }),
        SexpType::Cplx => parse!(alloc_cplx_from, RComplex::NA, "a complex", |f| {
            number::complex(f, decimal).map(|x| x.0)
        }),
        SexpType::Raw => parse!(alloc_raw_from, 0, "a raw", |f: &str| {
            let bytes = f.as_bytes();
            match bytes {
                [first, second] if first.is_ascii_hexdigit() && second.is_ascii_hexdigit() => {
                    u8::from_str_radix(f, 16).ok()
                }
                [first, ..] if first.is_ascii_hexdigit() && f[1..].trim_ascii().is_empty() => {
                    Some(0)
                }
                _ => None,
            }
        }),
        _ => Ok(SEXP_NIL),
    }
}
