//! C entry points for parsing R expressions.

#![allow(non_snake_case)]

use std::ffi::{CStr, c_char, c_int};

use roxide_eval::{Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_syntax::{ParseStep, Parser};
use roxide_value::{
    CharId, KeepSource, SEXP_NIL, SYM_CLASS, SYM_SRCFILE, SYM_SRCREF, SexpType, ValueApi, lower,
};

use crate::abi::Sexp;

use super::env::{api, root};
use super::eval::eval;

const PARSE_OK: c_int = 1;
const PARSE_INCOMPLETE: c_int = 2;
const PARSE_ERROR: c_int = 3;

fn parse(
    ctx: &mut Ctx,
    source: &str,
    count: c_int,
    srcfile: SexpId,
) -> EvalResult<(SexpId, c_int)> {
    let limit = ctx.host().limits().max_vector_bytes / size_of::<SexpId>();
    let mut parser = Parser::new(source, Default::default());
    let mut expressions = Vec::new();
    let mut spans = Vec::new();
    let keep_source = ctx.session().type_of(srcfile)? == SexpType::Env;
    let count = usize::try_from(count).unwrap_or(usize::MAX);
    while expressions.len() < count {
        match parser.parse_next() {
            ParseStep::Expr(node) => {
                if expressions.len() == limit {
                    return Err(ctx.stop(SEXP_NIL, "expression vector exceeds the session limit"));
                }
                if keep_source {
                    let span = parser.ast().span(node);
                    spans.push((span.lo, span.hi));
                }
                expressions.push(lower(
                    parser.ast(),
                    node,
                    ctx.session_mut(),
                    KeepSource::No,
                )?);
            }
            ParseStep::Eof => break,
            ParseStep::Incomplete => return Ok((SEXP_NIL, PARSE_INCOMPLETE)),
            ParseStep::Error(_) => return Ok((SEXP_NIL, PARSE_ERROR)),
        }
    }
    let result = ctx.session_mut().alloc_expr_from(&expressions)?;
    ctx.protect(result);
    if keep_source {
        source_refs(ctx, result, srcfile, source, &spans)?;
    }
    for warning in parser.warnings() {
        ctx.warn_now(SEXP_NIL, &warning.message)?;
    }
    Ok((result, PARSE_OK))
}

fn source_refs(
    ctx: &mut Ctx,
    result: SexpId,
    srcfile: SexpId,
    source: &str,
    spans: &[(u32, u32)],
) -> EvalResult<()> {
    let starts: Vec<_> = std::iter::once(0)
        .chain(source.match_indices('\n').map(|(i, _)| i + 1))
        .collect();
    let position = |offset: u32| {
        let offset = offset as usize;
        let line = starts.partition_point(|&start| start <= offset) - 1;
        let start = starts[line];
        let col = source.as_bytes()[start..offset]
            .iter()
            .filter(|&&byte| byte & 0xc0 != 0x80)
            .count();
        // Offset can point to the last byte of a multibyte character.
        let col = col + usize::from(source.is_char_boundary(offset));
        ((line + 1) as i32, (offset - start + 1) as i32, col as i32)
    };

    let session = ctx.session_mut();
    let class = session.scalar_str("srcref")?;
    let mut reference = |lo, hi: u32| -> EvalResult {
        let (first, first_byte, first_col) = position(lo);
        let (last, last_byte, last_col) = position(hi.saturating_sub(1).max(lo));
        let reference = session.alloc_int_from(&[
            first, first_byte, last, last_byte, first_col, last_col, first, last,
        ])?;
        session.set_attrib(reference, SYM_CLASS, class)?;
        session.set_attrib(reference, SYM_SRCFILE, srcfile)?;
        Ok(reference)
    };
    let refs = spans
        .iter()
        .map(|&(lo, hi)| reference(lo, hi))
        .collect::<EvalResult<Vec<_>>>()?;
    let whole = reference(0, source.len() as u32)?;
    let list = session.alloc_list(refs.len())?;
    for (i, reference) in refs.into_iter().enumerate() {
        session.list_set(list, i, reference)?;
    }
    let whole_sym = session.intern("wholeSrcref")?;
    session.set_attrib(result, SYM_SRCREF, list)?;
    session.set_attrib(result, SYM_SRCFILE, srcfile)?;
    session.set_attrib(result, whole_sym, whole)?;
    Ok(())
}

/// Parse a character vector into an expression vector and store its parse status.
///
/// # Safety
/// Call inside an active native trampoline. `status` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ParseVector(
    text: Sexp,
    count: c_int,
    status: *mut c_int,
    srcfile: Sexp,
) -> Sexp {
    unsafe {
        api(Sexp::NULL, |ctx| {
            if status.is_null() {
                return Err(ctx.stop(SEXP_NIL, "invalid NULL parse status pointer"));
            }
            let text = root(ctx, text)?;
            let srcfile = root(ctx, srcfile)?;
            ctx.safepoint();
            let limit = ctx.host().limits().max_vector_bytes.min(i32::MAX as usize);
            let session = ctx.session();
            let mut source = String::new();
            let lines = session.str_slice(text)?;
            if lines.is_empty() && count != 0 {
                return Err(ctx.stop(
                    SEXP_NIL,
                    "'Rf_translateChar' must be called on a CHARSXP, but got 'NULL'",
                ));
            }
            for (i, &line) in lines.iter().enumerate() {
                let line = if line == CharId::NA {
                    "NA".into()
                } else {
                    let bytes = session.char_bytes(line)?;
                    match session.char_encoding(line)? {
                        roxide_value::Encoding::Latin1 => std::borrow::Cow::Owned(
                            bytes.iter().map(|&byte| char::from(byte)).collect(),
                        ),
                        _ => String::from_utf8_lossy(bytes),
                    }
                };
                if source
                    .len()
                    .saturating_add(line.len())
                    .saturating_add(usize::from(i > 0))
                    > limit
                {
                    return Err(ctx.stop(SEXP_NIL, "parse input exceeds the session limit"));
                }
                if i > 0 {
                    source.push('\n');
                }
                source.push_str(&line);
            }
            let (result, parsed) = parse(ctx, &source, count, srcfile)?;
            status.write(parsed);
            Ok(Sexp::from_id(result))
        })
    }
}

unsafe fn parse_string(ctx: &mut Ctx, text: *const c_char) -> EvalResult {
    if text.is_null() {
        return Err(ctx.stop(SEXP_NIL, "invalid NULL parse input"));
    }
    let source = unsafe { CStr::from_ptr(text) }.to_string_lossy();
    if source.len() > ctx.host().limits().max_vector_bytes.min(i32::MAX as usize) {
        return Err(ctx.stop(SEXP_NIL, "parse input exceeds the session limit"));
    }
    let (parsed, status) = parse(ctx, &source, 2, SEXP_NIL)?;
    if status != PARSE_OK {
        return Err(ctx.stop(SEXP_NIL, "parse error"));
    }
    match ctx.session().xlength(parsed)? {
        1 => Ok(ctx.session().list_get(parsed, 0)?),
        _ => Err(ctx.stop(SEXP_NIL, "parse error")),
    }
}

/// Parse a single expression from a C string.
///
/// # Safety
/// Call inside an active native trampoline. `text` must point to a C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ParseString(text: *const c_char) -> Sexp {
    unsafe {
        api(Sexp::NULL, |ctx| {
            ctx.safepoint();
            parse_string(ctx, text).map(Sexp::from_id)
        })
    }
}

/// Parse a single expression and evaluate it in an environment.
///
/// # Safety
/// Call inside an active native trampoline. `text` must point to a C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_ParseEvalString(text: *const c_char, rho: Sexp) -> Sexp {
    unsafe {
        api(Sexp::NULL, |ctx| {
            let env = root(ctx, rho)?;
            ctx.safepoint();
            let expr = parse_string(ctx, text)?;
            eval(ctx, expr, env).map(Sexp::from_id)
        })
    }
}
