//! Milestone 0 core primitives (`<-`, `=`, `(`, `{`, `+`, `-`, `*`, `/`, `^`, `c`, `print`, `dput`, `cat`, `is.null`, `length`, `invisible`).

use roxide_diag::msg;
use roxide_gc::SexpId;
use roxide_value::format::format_real_scalar;
use roxide_value::{
    CharId, DeparseOpts, Encoding, FormatOpts, NA_INTEGER, NA_REAL, RLogical, SEXP_NIL, SexpType,
    ValueApi, deparse, encode_string, format_int_vec, format_lgl_vec, format_real_vec, int_add,
    int_mul, int_sub, nan_propagate,
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
            fun: prim_assign,
        },
        PrimDef {
            name: "=",
            kind: PrimKind::Primitive,
            args: ArgEval::Unevaluated,
            arity: Arity::Exact(2),
            dispatch: Dispatch::None,
            visibility: Visibility::Off,
            fun: prim_assign,
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

fn prim_assign(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    if args.len() != 2 {
        return Err(ctx.stop(call.err_call(), "invalid assignment"));
    }
    let lhs = args.get(0);
    let rhs_expr = args.get(1);
    let sym = ctx.sym_from_sexp(lhs)?;
    let val = ctx.eval(rhs_expr, call.env)?;
    ctx.assign(sym, val, call.env)?;
    Ok(val)
}

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
    let min_len = la.min(lb);
    if !out_len.is_multiple_of(min_len) {
        ctx.warn(call.err_call(), msg::RECYCLE_MISMATCH);
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
            ctx.warn(call.err_call(), msg::INT_OVERFLOW);
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

fn prim_c(ctx: &mut Ctx, _call: &Call, args: &mut Args) -> EvalResult {
    let mut target_rank = 0u8;
    for &v in args.values() {
        target_rank = target_rank.max(type_rank(ctx.session.type_of(v)?));
    }
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
        _ => {
            let lines = deparse(&ctx.session, val, DeparseOpts::default())?;
            format!("{}\n", lines.join("\n"))
        }
    };
    let _ = ctx.host.write_out(line.as_bytes());
    Ok(())
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
    let val = args.get(0);
    let lines = deparse(&ctx.session, val, DeparseOpts::default())?;
    for line in lines {
        let _ = ctx.host.write_out(line.as_bytes());
        let _ = ctx.host.write_out(b"\n");
    }
    Ok(val)
}

fn prim_cat(ctx: &mut Ctx, _call: &Call, args: &mut Args) -> EvalResult {
    let sep = if let Some(sep_val) = args.named(&ctx.session, "sep") {
        let slice = ctx.session.str_slice(sep_val)?;
        if let Some(&cid) = slice.first() {
            String::from_utf8_lossy(ctx.session.char_bytes(cid)?).into_owned()
        } else {
            String::from(" ")
        }
    } else {
        String::from(" ")
    };

    let fmt_opts = FormatOpts {
        digits: ctx.opts.digits,
        scipen: ctx.opts.scipen,
    };
    let mut pieces = Vec::new();
    for (tag, val) in args.iter() {
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
            _ => {}
        }
    }

    let mut out = String::new();
    for (i, piece) in pieces.iter().enumerate() {
        if i > 0 {
            out.push_str(&sep);
        }
        out.push_str(piece);
    }
    // GNU R cat(): if sep contains '\n' and any pieces were printed, append trailing '\n' unless already emitted.
    if !pieces.is_empty() && sep.contains('\n') {
        out.push('\n');
    }
    let _ = ctx.host.write_out(out.as_bytes());
    Ok(SEXP_NIL)
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
