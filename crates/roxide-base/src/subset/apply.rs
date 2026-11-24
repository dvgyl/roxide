use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{CharId, SEXP_MISSING, SEXP_NIL, SexpType, Sym, ValueApi};

use crate::ops::common::{
    atomic, attr, ch, chars, concatenate, element, error, flag, formals, list, names, rank, set,
    set_names, string, tag, vector_limit,
};

use super::index::selected;

fn function(ctx: &mut Ctx, call: &Call, fun: SexpId, env: SexpId) -> EvalResult {
    let s = ctx.session();
    let ty = s.type_of(fun)?;
    if matches!(ty, SexpType::Special | SexpType::Builtin | SexpType::Clo) {
        return Ok(fun);
    }
    let name = if ty == SexpType::Sym {
        s.sym_name_of(fun).ok().map(str::to_owned)
    } else {
        string(s, fun)
    };
    let Some(name) = name else {
        return Err(error(
            ctx,
            call,
            "argument 'FUN' is not a function, character or symbol",
        ));
    };
    let sym = ctx.session_mut().intern(&name)?;
    ctx.find_fun(sym, env)
}

pub(super) fn lapply(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    apply_each(ctx, call, args, Mode::List)
}
pub(super) fn sapply(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    apply_each(ctx, call, args, Mode::Simplify)
}
pub(super) fn vapply(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    apply_each(ctx, call, args, Mode::Template)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    List,
    Simplify,
    Template,
}

fn split_args<const N: usize>(
    ctx: &Ctx,
    args: &Args,
    labels: [&str; N],
    positional: usize,
) -> ([SexpId; N], Vec<(Option<Sym>, SexpId)>) {
    let s = ctx.session();
    let mut used = [false; N];
    let mut out = [SEXP_MISSING; N];
    let mut extra = Vec::new();
    for (sym, val) in args.iter() {
        if let Some(j) = labels.iter().position(|n| tag(s, sym) == Some(*n)) {
            used[j] = true;
            out[j] = val;
        }
    }
    for (sym, val) in args.iter() {
        if labels.iter().any(|n| tag(s, sym) == Some(*n)) {
            continue;
        }
        if sym.is_none()
            && let Some(j) = used[..positional].iter().position(|&u| !u)
        {
            used[j] = true;
            out[j] = val;
        } else {
            extra.push((sym, val));
        }
    }
    (out, extra)
}

fn invoke(ctx: &mut Ctx, call: &Call, fun: SexpId, args: &[(Option<Sym>, SexpId)]) -> EvalResult {
    ctx.scoped(|ctx| {
        for &(_, value) in args {
            ctx.protect(value);
        }
        ctx.apply(fun, args, call.expr, call.env)
    })
}

fn apply_each(ctx: &mut Ctx, call: &Call, args: &Args, mode: Mode) -> EvalResult {
    let (x, fun, template, simplify, use_names, mut more) = match mode {
        Mode::List => {
            let ([x, fun], more) = split_args(ctx, args, ["X", "FUN"], 2);
            (x, fun, SEXP_MISSING, SEXP_MISSING, SEXP_MISSING, more)
        }
        Mode::Simplify => {
            let ([x, fun, simplify, use_names], more) =
                split_args(ctx, args, ["X", "FUN", "simplify", "USE.NAMES"], 2);
            (x, fun, SEXP_MISSING, simplify, use_names, more)
        }
        Mode::Template => {
            let ([x, fun, template, use_names], more) =
                split_args(ctx, args, ["X", "FUN", "FUN.VALUE", "USE.NAMES"], 3);
            (x, fun, template, SEXP_MISSING, use_names, more)
        }
    };
    let use_names = flag(ctx, use_names, true)?;
    let simplify =
        string(ctx.session(), simplify).as_deref() == Some("array") || flag(ctx, simplify, true)?;
    let f = function(ctx, call, fun, call.env)?;
    ctx.protect(f);
    let s = ctx.session();
    let n = s.xlength(x)?;
    let mut nms = names(s, x).to_vec();
    if nms.is_empty() && s.type_of(x)? == SexpType::List {
        let source: Vec<_> = s
            .pairlist_iter(x)
            .map(|(n, _)| tag(s, n).map(str::to_owned))
            .collect();
        if source.iter().any(Option::is_some) {
            for label in source {
                nms.push(ch(ctx.session_mut(), label.as_deref().unwrap_or(""))?);
            }
        }
    }
    if mode == Mode::Template && !use_names {
        nms.clear();
    }
    if use_names
        && mode != Mode::List
        && nms.is_empty()
        && ctx.session().type_of(x)? == SexpType::Str
    {
        nms = chars(ctx.session(), x).to_vec();
    }
    vector_limit(ctx, call, n, size_of::<SexpId>())?;
    let standard = if mode == Mode::Template {
        Some((
            ctx.session().type_of(template)?,
            ctx.session().xlength(template)?,
        ))
    } else {
        None
    };
    more.insert(0, (None, SEXP_NIL));
    let mut values = Vec::with_capacity(n);
    for i in 0..n {
        let v = element(ctx.session_mut(), x, i)?;
        more[0].1 = v;
        let result = invoke(ctx, call, f, &more)?;
        ctx.protect(result);
        if let Some((ty, len)) = standard {
            let s = ctx.session();
            let size = s.xlength(result)?;
            if size != len {
                return Err(error(
                    ctx,
                    call,
                    format!(
                        "values must be length {len},\n but FUN(X[[{}]]) result is length {size}",
                        i + 1
                    ),
                ));
            }
            let result_ty = s.type_of(result)?;
            let widen = matches!(
                ty,
                SexpType::Lgl | SexpType::Int | SexpType::Real | SexpType::Cplx
            ) && matches!(
                result_ty,
                SexpType::Lgl | SexpType::Int | SexpType::Real | SexpType::Cplx
            ) && rank(result_ty) <= rank(ty);
            if ty != result_ty && !widen {
                return Err(error(
                    ctx,
                    call,
                    format!(
                        "values must be type '{}',\n but FUN(X[[{}]]) result is type '{}'",
                        ty.type_name(false),
                        i + 1,
                        result_ty.type_name(false)
                    ),
                ));
            }
        }
        values.push(result);
    }
    let s = ctx.session();
    let info = if let Some((ty, len)) = standard {
        Some((template, ty, len))
    } else if mode == Mode::Simplify && simplify {
        if let Some(&first) = values.first() {
            let len = s.xlength(first)?;
            let mut ty = SexpType::Nil;
            let mut equal = len != 0;
            for &v in &values {
                let result_ty = s.type_of(v)?;
                if rank(result_ty) > rank(ty) {
                    ty = result_ty;
                }
                equal &= s.xlength(v)? == len
                    && (atomic(result_ty) || matches!(result_ty, SexpType::Vec | SexpType::Expr));
            }
            equal.then_some((first, ty, len))
        } else {
            None
        }
    } else {
        None
    };
    if let Some((shape, ty, len)) = info {
        if mode == Mode::Simplify && len == 1 {
            let s = ctx.session_mut();
            let mut labels = Vec::with_capacity(values.len());
            for (i, &v) in values.iter().enumerate() {
                let outer = nms.get(i).copied().unwrap_or(CharId::BLANK);
                let inner = names(s, v).first().copied().unwrap_or(CharId::BLANK);
                labels.push(match (outer, inner) {
                    (CharId::BLANK, _) => inner,
                    (_, CharId::BLANK) => outer,
                    _ => ch(
                        s,
                        &format!(
                            "{}.{}",
                            crate::ops::common::text(s, outer),
                            crate::ops::common::text(s, inner)
                        ),
                    )?,
                });
            }
            if labels.iter().any(|&v| v != CharId::BLANK) {
                nms = labels;
            }
        }
        return simplify_values(ctx, call, &values, &nms, shape, ty, len);
    }
    let s = ctx.session_mut();
    let out = list(s, &values)?;
    if !nms.is_empty() {
        set_names(s, out, &nms)?;
    }
    Ok(out)
}

fn simplify_values(
    ctx: &mut Ctx,
    call: &Call,
    values: &[SexpId],
    nms: &[CharId],
    shape: SexpId,
    ty: SexpType,
    len: usize,
) -> EvalResult {
    vector_limit(
        ctx,
        call,
        len.saturating_mul(values.len()),
        size_of::<SexpId>() * 2,
    )?;
    let s = ctx.session_mut();
    let out = if atomic(ty) {
        concatenate(s, values, ty)?
    } else {
        let mut data = Vec::new();
        for &v in values {
            for i in 0..len {
                data.push(element(s, v, i)?);
            }
        }
        concatenate(s, &data, ty)?
    };
    let old_dim = attr(s, shape, "dim");
    if len == 1 && old_dim == SEXP_NIL {
        if !nms.is_empty() {
            set_names(s, out, nms)?;
        }
    } else {
        let mut dims = if old_dim == SEXP_NIL {
            vec![len as i32]
        } else {
            s.int(old_dim)?.to_vec()
        };
        let mut labels = attr(s, shape, "dimnames");
        if labels == SEXP_NIL {
            labels = values.first().map_or(SEXP_NIL, |&v| attr(s, v, "dimnames"));
        }
        let mut axes = if old_dim == SEXP_NIL {
            vec![{
                let labels = attr(s, shape, "names");
                if labels == SEXP_NIL {
                    values.first().map_or(SEXP_NIL, |&v| attr(s, v, "names"))
                } else {
                    labels
                }
            }]
        } else {
            (0..dims.len())
                .map(|i| s.list_get(labels, i).unwrap_or(SEXP_NIL))
                .collect()
        };
        dims.push(values.len() as i32);
        axes.push(if nms.is_empty() {
            SEXP_NIL
        } else {
            s.alloc_str_from(nms)?
        });
        let dim = s.alloc_int_from(&dims)?;
        set(s, out, "dim", dim)?;
        if axes.iter().any(|&v| v != SEXP_NIL) {
            let dn = list(s, &axes)?;
            set(s, out, "dimnames", dn)?;
        }
    }
    Ok(out)
}

pub(super) fn reduce(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [f, x, init, right, accumulate] = formals(
        ctx.session(),
        args,
        ["f", "x", "init", "right", "accumulate"],
    );
    let f = function(ctx, call, f, call.env)?;
    ctx.protect(f);
    let right = flag(ctx, right, false)?;
    let accumulate = flag(ctx, accumulate, false)?;
    let n = ctx.session().xlength(x)?;
    if n == 0 && init == SEXP_MISSING {
        return Ok(SEXP_NIL);
    }
    if accumulate {
        vector_limit(ctx, call, n.saturating_add(1), size_of::<SexpId>())?;
    }
    let mut iter = (0..n).map(|i| if right { n - i - 1 } else { i });
    let mut acc = if init != SEXP_MISSING {
        init
    } else if let Some(i) = iter.next() {
        element(ctx.session_mut(), x, i)?
    } else {
        SEXP_NIL
    };
    let mut history = Vec::new();
    if accumulate {
        ctx.protect(acc);
        history.push(acc);
    }
    for i in iter {
        acc = ctx.scoped(|ctx| {
            let v = element(ctx.session_mut(), x, i)?;
            let (a, b) = if right { (v, acc) } else { (acc, v) };
            invoke(ctx, call, f, &[(None, a), (None, b)])
        })?;
        if accumulate {
            ctx.protect(acc);
            history.push(acc);
        }
    }
    if !accumulate {
        return Ok(acc);
    }
    if right {
        history.reverse();
    }
    let s = ctx.session_mut();
    let kind = history.iter().try_fold(SexpType::Nil, |current, &v| {
        let ty = s.type_of(v)?;
        let ty = if atomic(ty) && s.xlength(v)? == 1 {
            ty
        } else {
            SexpType::Vec
        };
        Ok::<_, roxide_value::ValueError>(if rank(ty) > rank(current) {
            ty
        } else {
            current
        })
    })?;
    Ok(concatenate(s, &history, kind)?)
}

pub(super) fn filter(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [f, x] = formals(ctx.session(), args, ["f", "x"]);
    let f = function(ctx, call, f, call.env)?;
    ctx.protect(f);
    let n = ctx.session().xlength(x)?;
    vector_limit(ctx, call, n, size_of::<Option<usize>>())?;
    let mut keep = Vec::new();
    for i in 0..n {
        let yes = ctx.scoped(|ctx| {
            let v = element(ctx.session_mut(), x, i)?;
            let r = invoke(ctx, call, f, &[(None, v)])?;
            flag(ctx, r, false)
        })?;
        if yes {
            keep.push(Some(i));
        }
    }
    selected(ctx, x, &keep)
}

pub(super) fn do_call(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [what, values, quote, env] =
        formals(ctx.session(), args, ["what", "args", "quote", "envir"]);
    let env = if env == SEXP_MISSING { call.env } else { env };
    if ctx.session().type_of(env)? != SexpType::Env {
        return Err(error(ctx, call, "'envir' must be an environment"));
    }
    let head = if let Some(name) = string(ctx.session(), what) {
        ctx.session_mut().intern(&name)?.as_sexp()
    } else if ctx.session().type_of(what)? == SexpType::Sym {
        what
    } else {
        function(ctx, call, what, env)?
    };
    ctx.protect(head);
    let quote = flag(ctx, quote, false)?;
    let s = ctx.session_mut();
    let len = match s.type_of(values)? {
        SexpType::Nil => 0,
        SexpType::Vec => s.xlength(values)?,
        _ => return Err(error(ctx, call, "second argument must be a list")),
    };
    let mut tail = SEXP_NIL;
    let quote = if quote {
        let namespace = s.intern("::")?.as_sexp();
        let base = s.intern("base")?.as_sexp();
        let quote = s.intern("quote")?.as_sexp();
        let args = s.cons(quote, SEXP_NIL, SEXP_NIL)?;
        let args = s.cons(base, args, SEXP_NIL)?;
        Some(s.lcons(namespace, args, SEXP_NIL)?)
    } else {
        None
    };
    for i in (0..len).rev() {
        let mut value = s.list_get(values, i)?;
        let tag = match names(s, values).get(i).copied() {
            Some(id) if id != CharId::NA && id != CharId::BLANK => {
                let name = crate::ops::common::text(s, id);
                s.intern(&name)?.as_sexp()
            }
            _ => SEXP_NIL,
        };
        if let Some(quote) = quote {
            let arg = s.cons(value, SEXP_NIL, SEXP_NIL)?;
            value = s.lcons(quote, arg, SEXP_NIL)?;
        }
        tail = s.cons(value, tail, tag)?;
    }
    let inner = s.lcons(head, tail, SEXP_NIL)?;
    ctx.eval(inner, env)
}
