use roxide_diag::{Span, SrcFile};
use roxide_gc::SexpId;
use roxide_syntax::{ParseStep, Parser};
use roxide_value::{
    BASE_ENV, CharId, DeparseOpts, KeepSource, SEXP_NIL, SexpType, ValueApi, lower,
};

use crate::{Args, Call, Ctx, EvalResult};

pub(super) fn parse(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(
        args,
        &[
            "file",
            "n",
            "text",
            "prompt",
            "keep.source",
            "srcfile",
            "encoding",
        ],
        call.err_call(),
    )?;
    let n = ctx
        .matched_arg(args, &matched, 1)
        .map(|value| ctx.integer(value, call.err_call()))
        .transpose()?
        .unwrap_or(-1);
    let keep_source = match ctx.matched_arg(args, &matched, 4) {
        Some(value) => ctx.truth(value, call.err_call())?,
        None => ctx
            .option_value("keep.source")?
            .is_some_and(|value| value == roxide_value::TRUE_SCALAR),
    };
    let mut text = ctx.matched_arg(args, &matched, 2).unwrap_or(SEXP_NIL);
    let mut source_name = String::from("<text>");
    if text == SEXP_NIL {
        let Some(file) = ctx
            .matched_arg(args, &matched, 0)
            .filter(|&file| file != SEXP_NIL)
        else {
            return Ok(ctx.session.alloc_expr_from(&[])?);
        };
        if ctx.session.type_of(file)? == SexpType::Str {
            let slice = ctx.session.str_slice(file)?;
            if slice.len() == 1 && slice[0] != CharId::NA {
                let name = String::from_utf8_lossy(ctx.session.char_bytes(slice[0])?).into_owned();
                if name.is_empty() {
                    return Ok(ctx.session.alloc_expr_from(&[])?);
                }
                source_name = name;
            }
        }
        let read_sym = ctx.session.intern("readLines")?;
        let read_fun = ctx.find_fun(read_sym, BASE_ENV)?;
        let warn_sym = ctx.session.intern("warn")?;
        text = ctx.apply(
            read_fun,
            &[(None, file), (Some(warn_sym), roxide_value::FALSE_SCALAR)],
            call.err_call(),
            call.env,
        )?;
    }
    let lines = ctx
        .session
        .str_slice(text)?
        .iter()
        .map(|&line| {
            if line == CharId::NA {
                return Ok("NA".to_owned());
            }
            Ok(String::from_utf8_lossy(ctx.session.char_bytes(line)?).into_owned())
        })
        .collect::<EvalResult<Vec<_>>>()?;
    let source = lines.join("\n");
    let file_id = ctx.sources.add_file(&source_name, source.as_str());
    let mut parser = Parser::new(&source, file_id);
    let mut expressions = Vec::new();
    let mut spans = Vec::new();
    while n < 0 || expressions.len() < n as usize {
        match parser.parse_next() {
            ParseStep::Eof => break,
            ParseStep::Incomplete => {
                return Err(ctx.stop(call.err_call(), "unexpected end of input"));
            }
            ParseStep::Error(err) => return Err(ctx.stop(call.err_call(), err.message)),
            ParseStep::Expr(node) => {
                spans.push(parser.ast().span(node));
                expressions.push(lower(parser.ast(), node, &mut ctx.session, KeepSource::No)?);
            }
        }
    }
    let expr = ctx.session.alloc_expr_from(&expressions)?;
    if keep_source {
        add_source(ctx, expr, &source, &lines, &spans)?;
    }
    Ok(expr)
}

fn add_source(
    ctx: &mut Ctx,
    expr: SexpId,
    source: &str,
    lines: &[String],
    spans: &[Span],
) -> EvalResult<()> {
    let file = SrcFile::new("<text>", source);
    let srcfile = ctx.session.alloc_env(roxide_value::EMPTY_ENV)?;
    let class_sym = ctx.session.intern("class")?;
    let srcfile_sym = ctx.session.intern("srcfile")?;
    let srcref_sym = ctx.session.intern("srcref")?;
    let whole_sym = ctx.session.intern("wholeSrcref")?;
    let classes = ctx.strings(["srcfilecopy", "srcfile"])?;
    ctx.session.set_attrib(srcfile, class_sym, classes)?;
    let lines_value = ctx.strings(lines.iter().map(String::as_str))?;
    let lines_sym = ctx.session.intern("lines")?;
    let filename_sym = ctx.session.intern("filename")?;
    let filename = ctx.session.scalar_str("<text>")?;
    ctx.assign(lines_sym, lines_value, srcfile)?;
    ctx.assign(filename_sym, filename, srcfile)?;
    let class = ctx.session.scalar_str("srcref")?;
    let mut reference = |lo, hi| -> EvalResult {
        let srcref = ctx.session.alloc_int_from(&file.srcref8(lo, hi))?;
        ctx.session.set_attrib(srcref, class_sym, class)?;
        ctx.session.set_attrib(srcref, srcfile_sym, srcfile)?;
        Ok(srcref)
    };
    let pairs = spans
        .iter()
        .map(|span| Ok((None, reference(span.lo, span.hi)?)))
        .collect::<EvalResult<Vec<_>>>()?;
    let whole = reference(0, source.len() as u32)?;
    let list = ctx.list(&Args::from_pairs(pairs))?;
    ctx.session.set_attrib(expr, srcref_sym, list)?;
    ctx.session.set_attrib(expr, srcfile_sym, srcfile)?;
    ctx.session.set_attrib(expr, whole_sym, whole)?;
    Ok(())
}

pub(super) fn deparse(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let one_line = ctx.prims[usize::from(call.prim.0)].name == "deparse1";
    let extra = if one_line { "collapse" } else { "nlines" };
    let matched = ctx.match_names(
        args,
        &[
            "expr",
            if one_line { extra } else { "width.cutoff" },
            if one_line { "width.cutoff" } else { "backtick" },
            "control",
            if one_line { "backtick" } else { extra },
        ],
        call.err_call(),
    )?;
    let expr = ctx.matched_arg(args, &matched, 0).unwrap_or(SEXP_NIL);
    let mut opts = DeparseOpts {
        width_cutoff: if one_line { 500 } else { 60 },
        ..DeparseOpts::default()
    };
    if let Some(value) = ctx.matched_arg(args, &matched, if one_line { 2 } else { 1 }) {
        opts.width_cutoff = ctx.integer(value, call.err_call())?.clamp(20, 500) as usize;
    }
    if let Some(value) = ctx.matched_arg(args, &matched, 3) {
        let control = ctx
            .session
            .str_slice(value)?
            .iter()
            .map(|&value| ctx.session.char_bytes(value))
            .collect::<Result<Vec<_>, _>>()?;
        opts.keep_integer =
            control.contains(&b"keepInteger".as_slice()) || control.contains(&b"all".as_slice());
        opts.keep_na =
            control.contains(&b"keepNA".as_slice()) || control.contains(&b"all".as_slice());
        opts.digits17 = control.contains(&b"digits17".as_slice());
    }
    let mut lines = ctx.deparse_lines(expr, opts)?;
    if one_line {
        let separator = ctx
            .matched_arg(args, &matched, 1)
            .map(|value| ctx.string(value, call.err_call()))
            .transpose()?;
        return Ok(ctx
            .session
            .scalar_str(&lines.join(separator.as_deref().unwrap_or(" ")))?);
    }
    if let Some(value) = ctx.matched_arg(args, &matched, 4) {
        let n = ctx.integer(value, call.err_call())?;
        if n >= 0 {
            lines.truncate(n as usize);
        }
    }
    ctx.strings(lines.iter().map(String::as_str))
}

pub(super) fn str_to_language(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let single = ctx.prims[usize::from(call.prim.0)].name == "str2lang";
    if ctx.session.type_of(args.get(0))? != SexpType::Str {
        return Err(ctx.stop(call.err_call(), "argument must be character"));
    }
    if single && ctx.session.xlength(args.get(0))? != 1 {
        return Err(ctx.stop(call.err_call(), "argument must be a character string"));
    }
    let text = ctx.session.intern("text")?;
    let source = ctx.session.intern("keep.source")?;
    let expr = parse(
        ctx,
        call,
        &mut Args::from_pairs(vec![
            (Some(text), args.get(0)),
            (Some(source), roxide_value::FALSE_SCALAR),
        ]),
    )?;
    if !single {
        return Ok(expr);
    }
    if ctx.session.xlength(expr)? != 1 {
        return Err(ctx.stop(
            call.err_call(),
            "parsing result not of length one, but 0 or > 1",
        ));
    }
    Ok(ctx.session.list_get(expr, 0)?)
}
