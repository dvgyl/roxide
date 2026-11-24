//! Milestone 0 core primitives (`<-`, `=`, `(`, `{`, `+`, `-`, `*`, `/`, `^`, `c`, `print`, `dput`, `cat`, `is.null`, `length`, `invisible`).

use std::io::Write;
use std::path::Path;

use roxide_diag::msg;
use roxide_gc::SexpId;
use roxide_host::OpenMode;
use roxide_value::format::format_real_scalar;
use roxide_value::{
    BASE_ENV, CharId, DeparseOpts, EMPTY_ENV, Encoding, FormatOpts, GLOBAL_ENV, NA_INTEGER,
    NA_REAL, RLogical, SEXP_NIL, SexpType, ValueApi, deparse, encode_string, format_int_vec,
    format_lgl_vec, format_real_vec, int_add, int_mul, int_sub, nan_propagate,
};

use crate::api::{
    ArgEval, Arity, Call, Dispatch, EvalResult, GroupGeneric, Module, PrimDef, PrimKind, Visibility,
};
use crate::ctx::{Args, Ctx};

/// Core Milestone 0 primitive module registered at `Ctx::new`.
pub const CORE_MODULE: Module = Module {
    prims: &[
        PrimDef {
            name: "<-",
            kind: PrimKind::Primitive,
            args: ArgEval::Unevaluated,
            arity: Arity::Exact(2),
            dispatch: Dispatch::None,
            visibility: Visibility::Off,
            fun: crate::control::assign,
        },
        PrimDef {
            name: "=",
            kind: PrimKind::Primitive,
            args: ArgEval::Unevaluated,
            arity: Arity::Exact(2),
            dispatch: Dispatch::None,
            visibility: Visibility::Off,
            fun: crate::control::assign,
        },
        PrimDef {
            name: "(",
            kind: PrimKind::Primitive,
            args: ArgEval::Unevaluated,
            arity: Arity::Exact(1),
            dispatch: Dispatch::None,
            visibility: Visibility::On,
            fun: prim_paren,
        },
        PrimDef {
            name: "{",
            kind: PrimKind::Primitive,
            args: ArgEval::Unevaluated,
            arity: Arity::Any,
            dispatch: Dispatch::None,
            visibility: Visibility::SetByFn,
            fun: prim_brace,
        },
        PrimDef {
            name: "+",
            kind: PrimKind::Primitive,
            args: ArgEval::Eval,
            arity: Arity::Range(1, 2),
            dispatch: Dispatch::Group(GroupGeneric::Ops),
            visibility: Visibility::On,
            fun: prim_add,
        },
        PrimDef {
            name: "-",
            kind: PrimKind::Primitive,
            args: ArgEval::Eval,
            arity: Arity::Range(1, 2),
            dispatch: Dispatch::Group(GroupGeneric::Ops),
            visibility: Visibility::On,
            fun: prim_sub,
        },
        PrimDef {
            name: "*",
            kind: PrimKind::Primitive,
            args: ArgEval::Eval,
            arity: Arity::Exact(2),
            dispatch: Dispatch::Group(GroupGeneric::Ops),
            visibility: Visibility::On,
            fun: prim_mul,
        },
        PrimDef {
            name: "/",
            kind: PrimKind::Primitive,
            args: ArgEval::Eval,
            arity: Arity::Exact(2),
            dispatch: Dispatch::Group(GroupGeneric::Ops),
            visibility: Visibility::On,
            fun: prim_div,
        },
        PrimDef {
            name: "^",
            kind: PrimKind::Primitive,
            args: ArgEval::Eval,
            arity: Arity::Exact(2),
            dispatch: Dispatch::Group(GroupGeneric::Ops),
            visibility: Visibility::On,
            fun: prim_pow,
        },
        PrimDef {
            name: "c",
            kind: PrimKind::Primitive,
            args: ArgEval::Eval,
            arity: Arity::Any,
            dispatch: Dispatch::None,
            visibility: Visibility::On,
            fun: prim_c,
        },
        PrimDef {
            name: "print",
            kind: PrimKind::Primitive,
            args: ArgEval::Eval,
            arity: Arity::Range(1, 10),
            dispatch: Dispatch::Internal("print"),
            visibility: Visibility::Off,
            fun: prim_print,
        },
        PrimDef {
            name: "dput",
            kind: PrimKind::Primitive,
            args: ArgEval::Eval,
            arity: Arity::Range(1, 3),
            dispatch: Dispatch::None,
            visibility: Visibility::Off,
            fun: prim_dput,
        },
        PrimDef {
            name: "cat",
            kind: PrimKind::Primitive,
            args: ArgEval::Eval,
            arity: Arity::Any,
            dispatch: Dispatch::None,
            visibility: Visibility::Off,
            fun: prim_cat,
        },
        PrimDef {
            name: "is.null",
            kind: PrimKind::Primitive,
            args: ArgEval::Eval,
            arity: Arity::Exact(1),
            dispatch: Dispatch::None,
            visibility: Visibility::On,
            fun: prim_is_null,
        },
        PrimDef {
            name: "length",
            kind: PrimKind::Primitive,
            args: ArgEval::Eval,
            arity: Arity::Exact(1),
            dispatch: Dispatch::Internal("length"),
            visibility: Visibility::On,
            fun: prim_length,
        },
        PrimDef {
            name: "invisible",
            kind: PrimKind::Primitive,
            args: ArgEval::Eval,
            arity: Arity::Range(0, 1),
            dispatch: Dispatch::None,
            visibility: Visibility::Off,
            fun: prim_invisible,
        },
    ],
    prelude: "",
};

fn prim_paren(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    if args.len() != 1 {
        return Err(ctx.stop(call.err_call(), "incorrect number of arguments to '('"));
    }
    ctx.eval(args.get(0), call.env)
}

fn prim_brace(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    if args.is_empty() {
        ctx.set_visible(true);
        return Ok(SEXP_NIL);
    }
    let mut last = SEXP_NIL;
    for &expr in args.values() {
        last = ctx.eval(expr, call.env)?;
    }
    Ok(last)
}

#[derive(Clone, Copy)]
enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
}

fn prim_add(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    if args.len() == 1 {
        return unary_sign(ctx, call, args.get(0), false);
    }
    binary_arith(ctx, call, args.get(0), args.get(1), BinOp::Add)
}

fn prim_sub(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    if args.len() == 1 {
        return unary_sign(ctx, call, args.get(0), true);
    }
    binary_arith(ctx, call, args.get(0), args.get(1), BinOp::Sub)
}

fn prim_mul(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    binary_arith(ctx, call, args.get(0), args.get(1), BinOp::Mul)
}

fn prim_div(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    binary_arith(ctx, call, args.get(0), args.get(1), BinOp::Div)
}

fn prim_pow(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    binary_arith(ctx, call, args.get(0), args.get(1), BinOp::Pow)
}

fn unary_sign(ctx: &mut Ctx, call: &Call, x: SexpId, negate: bool) -> EvalResult {
    ctx.check_vector_len(ctx.session.xlength(x)?, size_of::<f64>(), call.err_call())?;
    match ctx.session.type_of(x)? {
        SexpType::Int => {
            let vals: Vec<i32> = ctx
                .session
                .int(x)?
                .iter()
                .map(|n| if n == NA_INTEGER || !negate { n } else { -n })
                .collect();
            Ok(ctx.session.alloc_int_from(&vals)?)
        }
        SexpType::Lgl => {
            let vals: Vec<i32> = ctx
                .session
                .lgl(x)?
                .iter()
                .map(|b| match b.get() {
                    Some(true) => {
                        if negate {
                            -1
                        } else {
                            1
                        }
                    }
                    Some(false) => 0,
                    None => NA_INTEGER,
                })
                .collect();
            Ok(ctx.session.alloc_int_from(&vals)?)
        }
        SexpType::Real => {
            let vals: Vec<f64> = ctx
                .session
                .real(x)?
                .iter()
                .map(|v| if !negate || v.is_nan() { v } else { -v })
                .collect();
            Ok(ctx.session.alloc_real_from(&vals)?)
        }
        _ => Err(ctx.stop(call.err_call(), msg::NON_NUMERIC_UNOP)),
    }
}

fn binary_arith(ctx: &mut Ctx, call: &Call, a: SexpId, b: SexpId, op: BinOp) -> EvalResult {
    let ta = ctx.session.type_of(a)?;
    let tb = ctx.session.type_of(b)?;
    let is_num = |t: SexpType| matches!(t, SexpType::Lgl | SexpType::Int | SexpType::Real);
    if !is_num(ta) || !is_num(tb) {
        return Err(ctx.stop(call.err_call(), msg::NON_NUMERIC_BINOP));
    }
    let la = ctx.session.xlength(a)?;
    let lb = ctx.session.xlength(b)?;
    if la == 0 || lb == 0 {
        let int_res = matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul)
            && ta != SexpType::Real
            && tb != SexpType::Real;
        return if int_res {
            Ok(ctx.session.alloc_int_from(&[])?)
        } else {
            Ok(ctx.session.alloc_real_from(&[])?)
        };
    }
    let out_len = la.max(lb);
    ctx.check_vector_len(out_len, size_of::<f64>(), call.err_call())?;
    let min_len = la.min(lb);
    if !out_len.is_multiple_of(min_len) {
        ctx.warn_now(call.err_call(), msg::RECYCLE_MISMATCH)?;
    }

    // Integer path for +, -, * when neither operand is Real.
    if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul)
        && ta != SexpType::Real
        && tb != SexpType::Real
    {
        let va = as_int_vec(&ctx.session, a)?;
        let vb = as_int_vec(&ctx.session, b)?;
        let mut out = Vec::with_capacity(out_len);
        let mut overflow = false;
        for i in 0..out_len {
            let x = va[i % la];
            let y = vb[i % lb];
            let (r, ovf) = match op {
                BinOp::Add => int_add(x, y),
                BinOp::Sub => int_sub(x, y),
                BinOp::Mul => int_mul(x, y),
                _ => unreachable!(),
            };
            overflow |= ovf;
            out.push(r);
        }
        if overflow {
            ctx.warn_now(call.err_call(), msg::INT_OVERFLOW)?;
        }
        return Ok(ctx.session.alloc_int_from(&out)?);
    }

    // Double path.
    let va = as_real_vec(&ctx.session, a)?;
    let vb = as_real_vec(&ctx.session, b)?;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let x = va[i % la];
        let y = vb[i % lb];
        let r = match op {
            BinOp::Add => {
                let s = x + y;
                if s.is_nan() { nan_propagate(x, y) } else { s }
            }
            BinOp::Sub => {
                let s = x - y;
                if s.is_nan() { nan_propagate(x, y) } else { s }
            }
            BinOp::Mul => {
                let s = x * y;
                if s.is_nan() { nan_propagate(x, y) } else { s }
            }
            BinOp::Div => {
                let s = x / y;
                if s.is_nan() { nan_propagate(x, y) } else { s }
            }
            BinOp::Pow => {
                // ADR-0005: power identities apply BEFORE missing-value propagation.
                if y == 0.0 || x == 1.0 {
                    1.0
                } else {
                    let s = x.powf(y);
                    if s.is_nan() { nan_propagate(x, y) } else { s }
                }
            }
        };
        out.push(r);
    }
    Ok(ctx.session.alloc_real_from(&out)?)
}

fn as_int_vec(
    session: &roxide_value::RSession,
    x: SexpId,
) -> Result<Vec<i32>, roxide_value::ValueError> {
    match session.type_of(x)? {
        SexpType::Int => Ok(session.int(x)?.to_vec()),
        SexpType::Lgl => Ok(session
            .lgl(x)?
            .iter()
            .map(|b| match b.get() {
                Some(true) => 1,
                Some(false) => 0,
                None => NA_INTEGER,
            })
            .collect()),
        other => Err(roxide_value::ValueError::TypeMismatch {
            expected: "integer",
            actual: other.type_name(false),
        }),
    }
}

fn as_real_vec(
    session: &roxide_value::RSession,
    x: SexpId,
) -> Result<Vec<f64>, roxide_value::ValueError> {
    match session.type_of(x)? {
        SexpType::Real => Ok(session.real(x)?.to_vec()),
        SexpType::Int => Ok(session
            .int(x)?
            .iter()
            .map(|n| {
                if n == NA_INTEGER {
                    NA_REAL
                } else {
                    f64::from(n)
                }
            })
            .collect()),
        SexpType::Lgl => Ok(session
            .lgl(x)?
            .iter()
            .map(|b| match b.get() {
                Some(true) => 1.0,
                Some(false) => 0.0,
                None => NA_REAL,
            })
            .collect()),
        other => Err(roxide_value::ValueError::TypeMismatch {
            expected: "double",
            actual: other.type_name(false),
        }),
    }
}

fn type_rank(t: SexpType) -> u8 {
    match t {
        SexpType::Nil => 0,
        SexpType::Raw => 1,
        SexpType::Lgl => 2,
        SexpType::Int => 3,
        SexpType::Real => 4,
        SexpType::Cplx => 5,
        SexpType::Str => 6,
        _ => 7,
    }
}

fn prim_c(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let mut target_rank = 0u8;
    let mut len = 0usize;
    for &v in args.values() {
        target_rank = target_rank.max(type_rank(ctx.session.type_of(v)?));
        len = len.saturating_add(ctx.session.xlength(v)?);
    }
    ctx.check_vector_len(len, size_of::<SexpId>(), call.err_call())?;
    match target_rank {
        0 => Ok(SEXP_NIL),
        2 => {
            let mut out = Vec::new();
            for &v in args.values() {
                if v != SEXP_NIL {
                    out.extend(ctx.session.lgl(v)?.iter());
                }
            }
            Ok(ctx.session.alloc_lgl_from(&out)?)
        }
        3 => {
            let mut out = Vec::new();
            for &v in args.values() {
                if v != SEXP_NIL {
                    out.extend(as_int_vec(&ctx.session, v)?);
                }
            }
            Ok(ctx.session.alloc_int_from(&out)?)
        }
        4 => {
            let mut out = Vec::new();
            for &v in args.values() {
                if v != SEXP_NIL {
                    out.extend(as_real_vec(&ctx.session, v)?);
                }
            }
            Ok(ctx.session.alloc_real_from(&out)?)
        }
        _ => {
            let mut out = Vec::new();
            let fmt_opts = FormatOpts {
                digits: ctx.opts.digits,
                scipen: ctx.opts.scipen,
            };
            for &v in args.values() {
                if v == SEXP_NIL {
                    continue;
                }
                match ctx.session.type_of(v)? {
                    SexpType::Str => out.extend_from_slice(ctx.session.str_slice(v)?),
                    SexpType::Lgl => {
                        for b in ctx.session.lgl(v)?.to_vec() {
                            match b.get() {
                                Some(true) => {
                                    out.push(ctx.session.mk_char(b"TRUE", Encoding::Native)?)
                                }
                                Some(false) => {
                                    out.push(ctx.session.mk_char(b"FALSE", Encoding::Native)?)
                                }
                                None => out.push(CharId::NA),
                            }
                        }
                    }
                    SexpType::Int => {
                        for n in ctx.session.int(v)?.to_vec() {
                            if n == NA_INTEGER {
                                out.push(CharId::NA);
                            } else {
                                let s = n.to_string();
                                out.push(ctx.session.mk_char(s.as_bytes(), Encoding::Native)?);
                            }
                        }
                    }
                    SexpType::Real => {
                        for f in ctx.session.real(v)?.to_vec() {
                            if roxide_value::is_na_real(f) {
                                out.push(CharId::NA);
                            } else {
                                let s = format_real_scalar(f, fmt_opts);
                                out.push(ctx.session.mk_char(s.as_bytes(), Encoding::Native)?);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(ctx.session.alloc_str_from(&out)?)
        }
    }
}

/// Core default printer used by both `print()` and top-level auto-printing.
pub(crate) fn print_value(ctx: &mut Ctx, val: SexpId, digits: usize) -> EvalResult<()> {
    let ty = ctx.session.type_of(val)?;
    if matches!(
        ty,
        SexpType::Lgl | SexpType::Int | SexpType::Real | SexpType::Str
    ) {
        ctx.check_vector_len(ctx.session.xlength(val)?, size_of::<String>(), SEXP_NIL)?;
    }
    let fmt_opts = FormatOpts {
        digits,
        scipen: ctx.opts.scipen,
    };
    let line = match ty {
        SexpType::Nil => String::from("NULL\n"),
        SexpType::Lgl => {
            let v = ctx.session.lgl(val)?.to_vec();
            if v.is_empty() {
                String::from("logical(0)\n")
            } else {
                format_vector_output(&format_lgl_vec(&v), true)
            }
        }
        SexpType::Int => {
            let v = ctx.session.int(val)?.to_vec();
            if v.is_empty() {
                String::from("integer(0)\n")
            } else {
                format_vector_output(&format_int_vec(&v), true)
            }
        }
        SexpType::Real => {
            let v = ctx.session.real(val)?.to_vec();
            if v.is_empty() {
                String::from("numeric(0)\n")
            } else {
                format_vector_output(&format_real_vec(&v, fmt_opts), true)
            }
        }
        SexpType::Str => {
            let slice = ctx.session.str_slice(val)?;
            if slice.is_empty() {
                String::from("character(0)\n")
            } else {
                let mut items = Vec::with_capacity(slice.len());
                for &cid in slice {
                    if cid == CharId::NA {
                        items.push(String::from("NA"));
                    } else {
                        items.push(encode_string(ctx.session.char_bytes(cid)?));
                    }
                }
                format_vector_output(&items, false)
            }
        }
        SexpType::Env => format!("{}\n", ctx.environment_label(val)?),
        _ => {
            let lines = deparse(&ctx.session, val, DeparseOpts::default())?;
            format!("{}\n", lines.join("\n"))
        }
    };
    let _ = ctx.host.write_out(line.as_bytes());
    Ok(())
}

impl Ctx {
    /// Return the label used to print an environment.
    pub fn environment_label(&mut self, env: SexpId) -> EvalResult<String> {
        let label = match env {
            GLOBAL_ENV => String::from("R_GlobalEnv"),
            BASE_ENV => String::from("base"),
            EMPTY_ENV => String::from("R_EmptyEnv"),
            _ => {
                if let Some(name) = self.namespace_name(env) {
                    format!("namespace:{name}")
                } else {
                    let name = self.search_entry_name(env)?;
                    if name.starts_with("package:") {
                        name
                    } else {
                        format!("0x{:x}", env.to_bits())
                    }
                }
            }
        };
        Ok(format!("<environment: {label}>"))
    }
}

fn format_vector_output(items: &[String], right_align: bool) -> String {
    let width = items.iter().map(String::len).max().unwrap_or(0);
    let mut out = String::from("[1]");
    for item in items {
        out.push(' ');
        if right_align {
            for _ in item.len()..width {
                out.push(' ');
            }
            out.push_str(item);
        } else {
            out.push_str(item);
        }
    }
    out.push('\n');
    out
}

fn prim_print(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    if args.is_empty() {
        return Err(ctx.stop(
            call.err_call(),
            "argument \"x\" is missing, with no default",
        ));
    }
    let val = args.get(0);
    let mut digits = ctx.opts.digits;
    if let Some(d_val) = args.named(&ctx.session, "digits") {
        let d = match ctx.session.type_of(d_val)? {
            SexpType::Int => ctx.session.int(d_val)?.get(0) as usize,
            SexpType::Real => ctx.session.real(d_val)?.get(0) as usize,
            _ => digits,
        };
        digits = d;
    }
    if ctx.session.is_s4(val)? && ctx.show_s4(val, call.env)? {
        return Ok(val);
    }
    print_value(ctx, val, digits)?;
    Ok(val)
}

fn prim_dput(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    if args.is_empty() {
        return Err(ctx.stop(
            call.err_call(),
            "argument \"x\" is missing, with no default",
        ));
    }
    let val = args.named(&ctx.session, "x").unwrap_or_else(|| args.get(0));
    let file = args.named(&ctx.session, "file").or_else(|| {
        let mut pos = 0usize;
        for (tag, v) in args.iter() {
            if tag.is_none() {
                if pos == 1 {
                    return Some(v);
                }
                pos += 1;
            }
        }
        None
    });
    let lines = roxide_value::dput(&ctx.session, val, DeparseOpts::default())?;
    let write_to_stdout = match file {
        None => true,
        Some(f) if ctx.session.type_of(f)? == SexpType::Str => {
            let slice = ctx.session.str_slice(f)?;
            slice.len() == 1
                && slice[0] != roxide_value::CharId::NA
                && ctx.session.char_bytes(slice[0])?.is_empty()
        }
        Some(_) => false,
    };
    if write_to_stdout {
        for line in lines {
            let _ = ctx.host.write_out(line.as_bytes());
            let _ = ctx.host.write_out(b"\n");
        }
        return Ok(val);
    }
    let dest = file.unwrap_or(SEXP_NIL);
    ctx.scoped(|ctx| {
        ctx.protect(val);
        ctx.protect(dest);
        let mut ids = Vec::with_capacity(lines.len());
        for line in &lines {
            ids.push(ctx.session.mk_char(line.as_bytes(), Encoding::Native)?);
        }
        let text = ctx.session.alloc_str_from(&ids)?;
        ctx.protect(text);
        let sym = ctx.session.intern("writeLines")?;
        let fun = ctx.find_fun(sym, BASE_ENV)?;
        ctx.apply(
            fun,
            &[(None, text), (None, dest)],
            call.err_call(),
            call.env,
        )?;
        Ok(val)
    })
}

fn prim_cat(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let separators = cat_strings(ctx, args.named(&ctx.session, "sep"), call)?;
    let separators = if args.named(&ctx.session, "sep").is_none() {
        vec![" ".to_owned()]
    } else if separators.is_empty() {
        vec![String::new()]
    } else {
        separators
    };
    let labels = cat_strings(ctx, args.named(&ctx.session, "labels"), call)?;
    let width = match args.named(&ctx.session, "fill") {
        Some(fill) if ctx.session.type_of(fill)? == SexpType::Lgl => {
            if ctx.truth(fill, call.err_call())? {
                ctx.opts.width
            } else {
                0
            }
        }
        Some(fill) => ctx.integer(fill, call.err_call())?.max(0) as usize,
        None => 0,
    };
    let append = match args.named(&ctx.session, "append") {
        Some(value) => ctx.truth(value, call.err_call())?,
        None => false,
    };

    let fmt_opts = FormatOpts {
        digits: ctx.opts.digits,
        scipen: ctx.opts.scipen,
    };
    let mut pieces = Vec::new();
    for (i, (tag, val)) in args.iter().enumerate() {
        if let Some(sym) = tag
            && matches!(
                ctx.session.sym_name(sym).ok(),
                Some("sep" | "file" | "fill" | "labels" | "append")
            )
        {
            continue;
        }
        match ctx.session.type_of(val)? {
            SexpType::Nil => {}
            SexpType::Str => {
                for &cid in ctx.session.str_slice(val)? {
                    if cid == CharId::NA {
                        pieces.push(String::from("NA"));
                    } else {
                        pieces.push(
                            String::from_utf8_lossy(ctx.session.char_bytes(cid)?).into_owned(),
                        );
                    }
                }
            }
            SexpType::Lgl => pieces.extend(format_lgl_vec(&ctx.session.lgl(val)?.to_vec())),
            SexpType::Int => pieces.extend(format_int_vec(&ctx.session.int(val)?.to_vec())),
            SexpType::Real => {
                for x in ctx.session.real(val)?.iter() {
                    pieces.push(format_real_scalar(x, fmt_opts));
                }
            }
            SexpType::Sym => pieces.push(ctx.session.sym_name_of(val)?.to_owned()),
            SexpType::Raw => {
                pieces.extend(ctx.session.raw(val)?.iter().map(|x| format!("{x:02x}")))
            }
            SexpType::Cplx => {
                for x in ctx.session.cplx(val)?.iter() {
                    let real = format_real_scalar(x.re, fmt_opts);
                    let imaginary = format_real_scalar(x.im.abs(), fmt_opts);
                    let sign = if x.im < 0.0 { '-' } else { '+' };
                    pieces.push(format!("{real}{sign}{imaginary}i"));
                }
            }
            ty => {
                return Err(ctx.stop(
                    call.err_call(),
                    format!(
                        "argument {} (type '{}') cannot be handled by 'cat'",
                        i + 1,
                        ty.type_name(false)
                    ),
                ));
            }
        }
    }

    let mut out = String::new();
    let mut column = 0;
    let mut line = 0;
    for (i, piece) in pieces.iter().enumerate() {
        if i > 0 {
            let sep = &separators[(i - 1) % separators.len()];
            out.push_str(sep);
            column = cat_column(column, sep);
            if width > 0 && column > 0 && column + piece.chars().count() >= width {
                out.push('\n');
                column = 0;
            }
        }
        if column == 0 && width > 0 && !labels.is_empty() {
            let label = &labels[line % labels.len()];
            out.push_str(label);
            out.push(' ');
            column = label.chars().count() + 1;
            line += 1;
        }
        out.push_str(piece);
        column = cat_column(column, piece);
        ctx.check_vector_len(out.len(), 1, call.err_call())?;
    }
    if !pieces.is_empty() && (width > 0 || separators.iter().any(|sep| sep.contains('\n'))) {
        out.push('\n');
    }
    if let Some(file) = args.named(&ctx.session, "file") {
        if ctx.session.type_of(file)? == SexpType::Str {
            let path = ctx.string(file, call.err_call())?;
            if !path.is_empty() {
                let mode = OpenMode {
                    truncate: !append,
                    append,
                    ..OpenMode::write_truncate()
                };
                let result = ctx
                    .host()
                    .open(Path::new(&path), mode)
                    .and_then(|mut file| {
                        file.write_all(out.as_bytes())
                            .map_err(roxide_host::HostError::from_io)
                    });
                result.map_err(|error| ctx.stop(call.err_call(), error.to_string()))?;
                return Ok(SEXP_NIL);
            }
        } else {
            let text = ctx.session.scalar_str(&out)?;
            let sep = ctx.session.scalar_str("")?;
            ctx.call_function(
                "writeLines",
                &[(None, text), (None, file), (None, sep)],
                BASE_ENV,
            )?;
            return Ok(SEXP_NIL);
        }
    }
    ctx.host
        .write_out(out.as_bytes())
        .map_err(|error| ctx.stop(call.err_call(), error.to_string()))?;
    Ok(SEXP_NIL)
}

fn cat_column(column: usize, text: &str) -> usize {
    text.rsplit_once('\n').map_or_else(
        || column + text.chars().count(),
        |(_, last)| last.chars().count(),
    )
}

fn cat_strings(ctx: &mut Ctx, value: Option<SexpId>, call: &Call) -> EvalResult<Vec<String>> {
    let Some(value) = value.filter(|&value| value != SEXP_NIL) else {
        return Ok(Vec::new());
    };
    if ctx.session.type_of(value)? != SexpType::Str {
        return Err(ctx.stop(call.err_call(), "invalid separator or labels"));
    }
    ctx.session
        .str_slice(value)?
        .iter()
        .map(|&id| {
            if id == CharId::NA {
                Ok("NA".to_owned())
            } else {
                Ok(String::from_utf8_lossy(ctx.session.char_bytes(id)?).into_owned())
            }
        })
        .collect()
}

fn prim_is_null(ctx: &mut Ctx, _call: &Call, args: &mut Args) -> EvalResult {
    let is_nil = args.get(0) == SEXP_NIL;
    Ok(ctx.session.scalar_lgl(if is_nil {
        RLogical::TRUE
    } else {
        RLogical::FALSE
    })?)
}

fn prim_length(ctx: &mut Ctx, _call: &Call, args: &mut Args) -> EvalResult {
    let len = ctx.session.xlength(args.get(0))?;
    Ok(ctx.session.scalar_int(len as i32)?)
}

fn prim_invisible(_ctx: &mut Ctx, _call: &Call, args: &mut Args) -> EvalResult {
    Ok(if args.is_empty() {
        SEXP_NIL
    } else {
        args.get(0)
    })
}
