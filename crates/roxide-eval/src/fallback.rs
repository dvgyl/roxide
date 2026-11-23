//! Core value primitives for sessions that have no package modules loaded.

use roxide_gc::SexpId;
use roxide_value::{
    CharId, IdenticalOpts, NA_INTEGER, NA_REAL, RLogical, SEXP_MISSING, SEXP_NIL, SexpType,
    ValueApi,
};

use crate::control::logical;
use crate::{Args, Arity, Call, Ctx, Dispatch, EvalResult, GroupGeneric, Module};

const OPS: Dispatch = Dispatch::Group(GroupGeneric::Ops);
const MATH: Dispatch = Dispatch::Group(GroupGeneric::Math);
const SUMMARY: Dispatch = Dispatch::Group(GroupGeneric::Summary);

pub(crate) const MODULE: Module = Module {
    prims: &[
        primitive!("list", list, Eval, Arity::Any, On),
        primitive!("pairlist", pairlist, Eval, Arity::Any, On),
        primitive!("structure", structure, Eval, Arity::Any, On),
        primitive!("attr", attribute, Eval, Arity::Range(2, 3), On),
        primitive!("attr<-", set_attribute, Eval, Arity::Exact(3), On),
        primitive!("names", attribute, Eval, Arity::Exact(1), On),
        primitive!("names<-", set_attribute, Eval, Arity::Exact(2), On),
        primitive!("class", attribute, Eval, Arity::Exact(1), On),
        primitive!("class<-", set_attribute, Eval, Arity::Exact(2), On),
        primitive!("inherits", inherits, Eval, Arity::Range(2, 3), On),
        primitive!("identical", identical, Eval, Arity::Exact(2), On),
        primitive!(
            "as.character",
            character,
            Eval,
            Arity::Exact(1),
            On,
            Dispatch::Internal("as.character")
        ),
        primitive!(":", sequence, Eval, Arity::Exact(2), On),
        primitive!(
            "[",
            subscript,
            EvalKeepMissing,
            Arity::Range(1, 2),
            On,
            Dispatch::Internal("[")
        ),
        primitive!(
            "[[",
            subscript,
            EvalKeepMissing,
            Arity::Exact(2),
            On,
            Dispatch::Internal("[[")
        ),
        primitive!(
            "[<-",
            subassign,
            EvalKeepMissing,
            Arity::Range(2, 3),
            On,
            Dispatch::Internal("[<-")
        ),
        primitive!(
            "[[<-",
            subassign,
            EvalKeepMissing,
            Arity::Exact(3),
            On,
            Dispatch::Internal("[[<-")
        ),
        primitive!("$", dollar, Unevaluated, Arity::Exact(2), On),
        primitive!("$<-", set_dollar, Unevaluated, Arity::Exact(3), On),
        primitive!(
            "head",
            head_tail,
            Eval,
            Arity::Range(1, 2),
            On,
            Dispatch::Internal("head")
        ),
        primitive!(
            "tail",
            head_tail,
            Eval,
            Arity::Range(1, 2),
            On,
            Dispatch::Internal("tail")
        ),
        primitive!(">", compare, Eval, Arity::Exact(2), On, OPS),
        primitive!("<", compare, Eval, Arity::Exact(2), On, OPS),
        primitive!(">=", compare, Eval, Arity::Exact(2), On, OPS),
        primitive!("<=", compare, Eval, Arity::Exact(2), On, OPS),
        primitive!("==", compare, Eval, Arity::Exact(2), On, OPS),
        primitive!("!=", compare, Eval, Arity::Exact(2), On, OPS),
        primitive!("!", negate, Eval, Arity::Exact(1), On, OPS),
        primitive!("abs", math, Eval, Arity::Exact(1), On, MATH),
        primitive!("floor", math, Eval, Arity::Exact(1), On, MATH),
        primitive!("sum", summary, Eval, Arity::Any, On, SUMMARY),
        primitive!("max", summary, Eval, Arity::Any, On, SUMMARY),
    ],
    prelude: "",
};

fn list(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    ctx.list(args)
}

fn pairlist(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    ctx.pairlist(args, false)
}

fn structure(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    if args.is_empty() {
        return Err(ctx.stop(
            call.err_call(),
            "argument \".Data\" is missing, with no default",
        ));
    }
    let value = ctx.session.shallow_duplicate(args.get(0))?;
    for (tag, attr) in args.iter().skip(1) {
        let Some(tag) = tag else { continue };
        ctx.session.set_attrib(value, tag, attr)?;
    }
    Ok(value)
}

fn attribute(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let op = ctx.prims[usize::from(call.prim.0)].name;
    let value = args.get(0);
    if op == "class" {
        let mut classes = ctx.classes(value, false)?;
        if classes.is_empty() {
            classes = ctx.classes(value, true)?;
            if classes.as_slice() == ["integer", "numeric"] {
                classes.truncate(1);
            }
            if classes.as_slice() == ["double", "numeric"] {
                classes.remove(0);
            }
        }
        return ctx.strings(classes.iter().map(String::as_str));
    }
    let name = if op == "attr" {
        ctx.string(args.get(1), call.err_call())?
    } else {
        op.to_owned()
    };
    let tag = ctx.session.intern(&name)?;
    Ok(ctx.session.attr(value, tag))
}

fn set_attribute(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let op = ctx.prims[usize::from(call.prim.0)].name;
    let (name, attr) = match op {
        "attr<-" => (ctx.string(args.get(1), call.err_call())?, args.get(2)),
        "names<-" => ("names".to_owned(), args.get(1)),
        _ => ("class".to_owned(), args.get(1)),
    };
    let tag = ctx.session.intern(&name)?;
    let value = ctx.session.shallow_duplicate(args.get(0))?;
    ctx.session.set_attrib(value, tag, attr)?;
    Ok(value)
}

fn inherits(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let classes = ctx.classes(args.get(0), true)?;
    let queries = ctx.session.str_slice(args.get(1))?;
    let mut indices = Vec::with_capacity(queries.len());
    for &query in queries {
        let query = ctx.session.char_bytes(query)?;
        let index = classes.iter().position(|class| class.as_bytes() == query);
        indices.push(index.map_or(0, |i| i as i32 + 1));
    }
    if args.len() == 3 && ctx.truth(args.get(2), call.err_call())? {
        return Ok(ctx.session.alloc_int_from(&indices)?);
    }
    Ok(ctx
        .session
        .scalar_lgl(logical(indices.into_iter().any(|i| i != 0)))?)
}

fn identical(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let value = roxide_value::identical(
        &ctx.session,
        args.get(0),
        args.get(1),
        IdenticalOpts::default(),
    )?;
    Ok(ctx.session.scalar_lgl(logical(value))?)
}

fn character(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    Ok(ctx.session.coerce(args.get(0), SexpType::Str)?.0)
}

fn sequence(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let from = ctx.integer(args.get(0), call.err_call())?;
    let to = ctx.integer(args.get(1), call.err_call())?;
    let count = (i64::from(to) - i64::from(from)).unsigned_abs() as usize + 1;
    Ok(ctx
        .session
        .alloc_compact_intseq(from, count, if from <= to { 1 } else { -1 })?)
}

fn indices(ctx: &mut Ctx, value: SexpId, len: usize, call: &Call) -> EvalResult<Vec<usize>> {
    if value == SEXP_MISSING {
        ctx.check_vector_len(len, size_of::<usize>(), call.err_call())?;
        return Ok((0..len).collect());
    }
    if value == SEXP_NIL {
        return Ok(Vec::new());
    }
    ctx.check_vector_len(
        ctx.session.xlength(value)?,
        size_of::<usize>(),
        call.err_call(),
    )?;
    if ctx.session.type_of(value)? == SexpType::Lgl {
        ctx.check_vector_len(len, size_of::<usize>(), call.err_call())?;
        let mask = ctx.session.lgl(value)?;
        if mask.is_empty() {
            return Ok(Vec::new());
        }
        return Ok((0..len.max(mask.len()))
            .filter_map(|i| match mask.get(i % mask.len()).get() {
                Some(true) => Some(i),
                Some(false) => None,
                None => Some(usize::MAX),
            })
            .collect());
    }
    let (value, _) = ctx.session.coerce(value, SexpType::Int)?;
    let input = ctx.session.int(value)?;
    let mut positive = false;
    let mut negative = false;
    for i in input.iter() {
        positive |= i > 0 || i == NA_INTEGER;
        negative |= i < 0 && i != NA_INTEGER;
    }
    if positive && negative {
        return Err(ctx.stop(
            call.err_call(),
            "only 0's may be mixed with negative subscripts",
        ));
    }
    if negative {
        let omitted: std::collections::HashSet<_> = input
            .iter()
            .filter(|i| *i != 0)
            .map(|i| i.unsigned_abs() as usize - 1)
            .collect();
        ctx.check_vector_len(len, size_of::<usize>(), call.err_call())?;
        return Ok((0..len).filter(|i| !omitted.contains(i)).collect());
    }
    Ok(input
        .iter()
        .filter(|i| *i != 0)
        .map(|i| {
            if i == NA_INTEGER {
                usize::MAX
            } else {
                i as usize - 1
            }
        })
        .collect())
}

fn subset(ctx: &mut Ctx, value: SexpId, indices: &[usize]) -> EvalResult {
    let len = ctx.session.xlength(value)?;
    let limit = |i: usize| if i < len { Some(i) } else { None };
    let result = match ctx.session.type_of(value)? {
        SexpType::Nil => SEXP_NIL,
        SexpType::Vec => {
            let list = ctx.session.alloc_list(indices.len())?;
            for (j, &i) in indices.iter().enumerate() {
                let value = if i < len {
                    ctx.session.list_get(value, i)?
                } else {
                    SEXP_NIL
                };
                ctx.session.list_set(list, j, value)?;
            }
            list
        }
        SexpType::Expr => {
            let values = indices
                .iter()
                .map(|&i| {
                    if i < len {
                        ctx.session.list_get(value, i)
                    } else {
                        Ok(SEXP_NIL)
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;
            ctx.session.alloc_expr_from(&values)?
        }
        SexpType::Str => {
            let values = ctx.session.str_slice(value)?;
            let selected = indices
                .iter()
                .map(|&i| values.get(i).copied().unwrap_or(CharId::NA))
                .collect::<Vec<_>>();
            ctx.session.alloc_str_from(&selected)?
        }
        SexpType::Int => {
            let values = ctx.session.int(value)?;
            let selected = indices
                .iter()
                .map(|&i| limit(i).map_or(NA_INTEGER, |i| values.get(i)))
                .collect::<Vec<_>>();
            ctx.session.alloc_int_from(&selected)?
        }
        SexpType::Real => {
            let values = ctx.session.real(value)?;
            let selected = indices
                .iter()
                .map(|&i| limit(i).map_or(NA_REAL, |i| values.get(i)))
                .collect::<Vec<_>>();
            ctx.session.alloc_real_from(&selected)?
        }
        SexpType::Lgl => {
            let values = ctx.session.lgl(value)?;
            let selected = indices
                .iter()
                .map(|&i| limit(i).map_or(RLogical::NA, |i| values.get(i)))
                .collect::<Vec<_>>();
            ctx.session.alloc_lgl_from(&selected)?
        }
        SexpType::Cplx => {
            let values = ctx.session.cplx(value)?;
            let missing = roxide_value::RComplex {
                re: NA_REAL,
                im: NA_REAL,
            };
            let selected = indices
                .iter()
                .map(|&i| limit(i).map_or(missing, |i| values.get(i)))
                .collect::<Vec<_>>();
            ctx.session.alloc_cplx_from(&selected)?
        }
        SexpType::Raw => {
            let values = ctx.session.raw(value)?;
            let selected = indices
                .iter()
                .map(|&i| limit(i).map_or(0, |i| values.get(i)))
                .collect::<Vec<_>>();
            ctx.session.alloc_raw_from(&selected)?
        }
        _ => return Err(ctx.stop(SEXP_NIL, "object is not subsettable")),
    };
    let tag = ctx.session.intern("names")?;
    let names = ctx.session.attr(value, tag);
    if names != SEXP_NIL {
        let names = ctx.session.str_slice(names)?;
        let selected = indices
            .iter()
            .map(|&i| names.get(i).copied().unwrap_or(CharId::NA))
            .collect::<Vec<_>>();
        let names = ctx.session.alloc_str_from(&selected)?;
        ctx.session.set_attrib(result, tag, names)?;
    }
    Ok(result)
}

fn subscript(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let value = args.get(0);
    let index = if args.len() == 1 {
        SEXP_MISSING
    } else {
        args.get(1)
    };
    let len = ctx.session.xlength(value)?;
    let scalar = ctx.prims[usize::from(call.prim.0)].name == "[[";
    if ctx.session.type_of(index)? == SexpType::Str {
        let tag = ctx.session.intern("names")?;
        let names = ctx.session.attr(value, tag);
        let names = if names == SEXP_NIL {
            &[]
        } else {
            ctx.session.str_slice(names)?
        };
        let positions = ctx
            .session
            .str_slice(index)?
            .iter()
            .map(|id| names.iter().position(|n| n == id).unwrap_or(usize::MAX))
            .collect::<Vec<_>>();
        if !scalar {
            return subset(ctx, value, &positions);
        }
        if let [i] = positions.as_slice() {
            if *i < len {
                return ctx.element(value, *i);
            }
            if ctx.session.type_of(value)? == SexpType::Vec {
                return Ok(SEXP_NIL);
            }
        }
        return Err(ctx.stop(call.err_call(), "subscript out of bounds"));
    }
    let indices = indices(ctx, index, len, call)?;
    if scalar {
        if let [i] = indices.as_slice()
            && *i < len
        {
            return ctx.element(value, *i);
        }
        return Err(ctx.stop(call.err_call(), "subscript out of bounds"));
    }
    subset(ctx, value, &indices)
}

fn subassign(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let original = args.get(0);
    let len = ctx.session.xlength(original)?;
    let index = if args.len() == 2 {
        SEXP_MISSING
    } else {
        args.get(1)
    };
    let replacement = args.get(args.len() - 1);
    let scalar = ctx.prims[usize::from(call.prim.0)].name == "[[<-";
    let indices = indices(ctx, index, len, call)?;
    if indices.is_empty() {
        return Ok(original);
    }
    if indices.iter().any(|&i| i >= len) || (scalar && indices.len() != 1) {
        return Err(ctx.stop(call.err_call(), "subscript out of bounds"));
    }
    if ctx.session.type_of(original)? == SexpType::Vec {
        let copy = ctx.session.shallow_duplicate(original)?;
        let n = ctx.session.xlength(replacement)?;
        if scalar {
            if replacement == SEXP_NIL {
                let remaining: Vec<_> = (0..len).filter(|i| *i != indices[0]).collect();
                return subset(ctx, original, &remaining);
            }
            ctx.session.list_set(copy, indices[0], replacement)?;
        } else {
            if n == 0 {
                return Err(ctx.stop(call.err_call(), "replacement has length zero"));
            }
            for (j, &i) in indices.iter().enumerate() {
                let value = ctx.element(replacement, j % n)?;
                ctx.session.list_set(copy, i, value)?;
            }
        }
        return Ok(copy);
    }
    let ty = ctx.session.type_of(original)?;
    let rty = ctx.session.type_of(replacement)?;
    let target = if matches!(
        ty,
        SexpType::Raw
            | SexpType::Lgl
            | SexpType::Int
            | SexpType::Real
            | SexpType::Cplx
            | SexpType::Str
    ) && matches!(
        rty,
        SexpType::Raw
            | SexpType::Lgl
            | SexpType::Int
            | SexpType::Real
            | SexpType::Cplx
            | SexpType::Str
    ) {
        if ty == SexpType::Raw {
            rty
        } else if rty == SexpType::Raw {
            ty
        } else {
            [ty, rty]
                .into_iter()
                .max_by_key(|ty| *ty as u8)
                .unwrap_or(ty)
        }
    } else {
        return Err(ctx.stop(call.err_call(), "incompatible types in subassignment"));
    };
    let (from, _) = ctx.session.coerce(original, target)?;
    let (to, _) = ctx.session.coerce(replacement, target)?;
    let n = ctx.session.xlength(to)?;
    if n == 0 {
        return Err(ctx.stop(call.err_call(), "replacement has length zero"));
    }
    if scalar && n != 1 {
        return Err(ctx.stop(
            call.err_call(),
            "more elements supplied than there are to replace",
        ));
    }
    macro_rules! replace {
        ($get:ident, $new:ident) => {{
            let mut values = ctx.session.$get(from)?.to_vec();
            let replacement = ctx.session.$get(to)?;
            for (j, &i) in indices.iter().enumerate() {
                values[i] = replacement.get(j % n);
            }
            ctx.session.$new(&values)?
        }};
    }
    let copy = match target {
        SexpType::Lgl => replace!(lgl, alloc_lgl_from),
        SexpType::Int => replace!(int, alloc_int_from),
        SexpType::Real => replace!(real, alloc_real_from),
        SexpType::Cplx => replace!(cplx, alloc_cplx_from),
        SexpType::Raw => replace!(raw, alloc_raw_from),
        SexpType::Str => {
            let mut values = ctx.session.str_slice(from)?.to_vec();
            let replacement = ctx.session.str_slice(to)?;
            for (j, &i) in indices.iter().enumerate() {
                values[i] = replacement[j % n];
            }
            ctx.session.alloc_str_from(&values)?
        }
        _ => return Err(ctx.stop(call.err_call(), "incompatible types in subassignment")),
    };
    for (tag, value) in ctx.session.attrs(original)? {
        ctx.session.set_attrib(copy, tag, value)?;
    }
    Ok(copy)
}

fn dollar(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let value = ctx.eval(args.get(0), call.env)?;
    ctx.protect(value);
    let name = ctx.name(args.get(1), call.err_call())?;
    let tag = ctx.session.intern(&name)?;
    if ctx.session.type_of(value)? == SexpType::Env {
        return if ctx.session.env_get(value, tag).is_some() {
            Ok(ctx.get_var(tag, value)?.unwrap_or(SEXP_NIL))
        } else {
            Ok(SEXP_NIL)
        };
    }
    if matches!(
        ctx.session.type_of(value)?,
        SexpType::List | SexpType::Vec | SexpType::Nil
    ) {
        let args = ctx.list_args(value)?;
        return Ok(args.named(&ctx.session, &name).unwrap_or(SEXP_NIL));
    }
    Err(ctx.stop(call.err_call(), "$ operator is invalid for atomic vectors"))
}

fn set_dollar(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let value = ctx.eval(args.get(0), call.env)?;
    ctx.protect(value);
    let name = ctx.name(args.get(1), call.err_call())?;
    let tag = ctx.session.intern(&name)?;
    let replacement = ctx.eval(args.get(2), call.env)?;
    if ctx.session.type_of(value)? == SexpType::Env {
        ctx.assign(tag, replacement, value)?;
        return Ok(value);
    }
    let mut pairs = match ctx.session.type_of(value)? {
        SexpType::Vec | SexpType::List | SexpType::Nil => {
            ctx.list_args(value)?.iter().collect::<Vec<_>>()
        }
        _ => {
            ctx.protect(replacement);
            ctx.warn_now(call.err_call(), "Coercing LHS to a list")?;
            let len = ctx.session.xlength(value)?;
            ctx.check_vector_len(
                len,
                size_of::<(Option<roxide_value::Sym>, SexpId)>(),
                call.err_call(),
            )?;
            (0..len)
                .map(|i| Ok((None, ctx.element(value, i)?)))
                .collect::<EvalResult<Vec<_>>>()?
        }
    };
    if let Some(index) = pairs.iter().position(|(name, _)| *name == Some(tag)) {
        if replacement == SEXP_NIL {
            pairs.remove(index);
        } else {
            pairs[index].1 = replacement;
        }
    } else if replacement != SEXP_NIL {
        pairs.push((Some(tag), replacement));
    }
    ctx.list(&Args::from_pairs(pairs))
}

fn head_tail(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let value = args.get(0);
    let len = ctx.session.xlength(value)?;
    let n = if args.len() == 1 {
        6
    } else {
        ctx.integer(args.get(1), call.err_call())?
    };
    let count = if n < 0 {
        len.saturating_sub(n.unsigned_abs() as usize)
    } else {
        len.min(n as usize)
    };
    let start = if ctx.prims[usize::from(call.prim.0)].name == "head" {
        0
    } else {
        len - count
    };
    ctx.check_vector_len(count, size_of::<usize>(), call.err_call())?;
    subset(ctx, value, &(start..start + count).collect::<Vec<_>>())
}

fn compare(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    use std::cmp::Ordering::{Equal, Greater, Less};
    let (a, b) = (args.get(0), args.get(1));
    let (na, nb) = (ctx.session.xlength(a)?, ctx.session.xlength(b)?);
    if na == 0 || nb == 0 {
        return Ok(ctx.session.alloc_lgl_from(&[])?);
    }
    if !na.max(nb).is_multiple_of(na.min(nb)) {
        ctx.warn_now(call.err_call(), roxide_diag::msg::RECYCLE_MISMATCH)?;
    }
    let op = ctx.prims[usize::from(call.prim.0)].name;
    let ordered = |order| {
        logical(match op {
            ">" => order == Greater,
            "<" => order == Less,
            ">=" => order != Less,
            "<=" => order != Greater,
            "==" => order == Equal,
            _ => order != Equal,
        })
    };
    let count = na.max(nb);
    ctx.check_vector_len(count, size_of::<SexpId>(), call.err_call())?;
    let char_compare =
        ctx.session.type_of(a)? == SexpType::Str || ctx.session.type_of(b)? == SexpType::Str;
    let result = if char_compare {
        let (a, _) = ctx.session.coerce(a, SexpType::Str)?;
        let (b, _) = ctx.session.coerce(b, SexpType::Str)?;
        let (a, b) = (ctx.session.str_slice(a)?, ctx.session.str_slice(b)?);
        (0..count)
            .map(|i| {
                let (x, y) = (a[i % na], b[i % nb]);
                if x == CharId::NA || y == CharId::NA {
                    return Ok(RLogical::NA);
                }
                Ok(ordered(
                    ctx.session.char_bytes(x)?.cmp(ctx.session.char_bytes(y)?),
                ))
            })
            .collect::<EvalResult<Vec<_>>>()?
    } else {
        let (a, _) = ctx.session.coerce(a, SexpType::Real)?;
        let (b, _) = ctx.session.coerce(b, SexpType::Real)?;
        let (a, b) = (ctx.session.real(a)?, ctx.session.real(b)?);
        (0..count)
            .map(|i| {
                a.get(i % na)
                    .partial_cmp(&b.get(i % nb))
                    .map_or(RLogical::NA, ordered)
            })
            .collect()
    };
    Ok(ctx.session.alloc_lgl_from(&result)?)
}

fn negate(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let (value, _) = ctx.session.coerce(args.get(0), SexpType::Lgl)?;
    let result = ctx
        .session
        .lgl(value)?
        .iter()
        .map(|v| RLogical::from_option(v.get().map(|v| !v)))
        .collect::<Vec<_>>();
    Ok(ctx.session.alloc_lgl_from(&result)?)
}

fn math(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let value = args.get(0);
    let abs = ctx.prims[usize::from(call.prim.0)].name == "abs";
    if abs && ctx.session.type_of(value)? == SexpType::Int {
        let result = ctx
            .session
            .int(value)?
            .iter()
            .map(i32::saturating_abs)
            .zip(ctx.session.int(value)?.iter())
            .map(|(abs, original)| {
                if original == NA_INTEGER {
                    original
                } else {
                    abs
                }
            })
            .collect::<Vec<_>>();
        return Ok(ctx.session.alloc_int_from(&result)?);
    }
    let (value, _) = ctx.session.coerce(value, SexpType::Real)?;
    let result = ctx
        .session
        .real(value)?
        .iter()
        .map(|x| {
            if x.is_nan() {
                x
            } else if abs {
                x.abs()
            } else {
                x.floor()
            }
        })
        .collect::<Vec<_>>();
    Ok(ctx.session.alloc_real_from(&result)?)
}

fn summary(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let na_tag = ctx.session.intern("na.rm")?;
    let na_rm = args
        .named(&ctx.session, "na.rm")
        .map(|value| ctx.truth(value, call.err_call()))
        .transpose()?
        .unwrap_or(false);
    let sum = ctx.prims[usize::from(call.prim.0)].name == "sum";
    let mut integer = true;
    let mut missing = false;
    let mut nan = false;
    let mut count = 0;
    let mut result = if sum { 0.0 } else { f64::NEG_INFINITY };
    for (name, arg) in args.iter() {
        if name == Some(na_tag) || arg == SEXP_NIL {
            continue;
        }
        integer &= matches!(ctx.session.type_of(arg)?, SexpType::Int | SexpType::Lgl);
        let (data, _) = ctx.session.coerce(arg, SexpType::Real)?;
        for value in ctx.session.real(data)?.iter() {
            if value.is_nan() {
                if !na_rm {
                    missing |= roxide_value::is_na_real(value);
                    nan = true;
                }
                continue;
            }
            count += 1;
            result = if sum {
                result + value
            } else {
                result.max(value)
            };
        }
    }
    if missing {
        return if integer {
            Ok(ctx.session.scalar_int(NA_INTEGER)?)
        } else {
            Ok(ctx.session.scalar_real(NA_REAL)?)
        };
    }
    if nan {
        return Ok(ctx.session.scalar_real(f64::NAN)?);
    }
    if !sum && count == 0 {
        ctx.warn_now(
            call.err_call(),
            "no non-missing arguments to max; returning -Inf",
        )?;
    }
    if integer && (-f64::from(i32::MAX)..=f64::from(i32::MAX)).contains(&result) {
        Ok(ctx.session.scalar_int(result as i32)?)
    } else {
        Ok(ctx.session.scalar_real(result)?)
    }
}
