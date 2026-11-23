use roxide_eval::{Args, Call, Ctx, EvalResult, Unwind};
use roxide_gc::SexpId;
use roxide_value::{
    CharId, Encoding, NA_INTEGER, NA_REAL, RComplex, RLogical, RSession, SEXP_MISSING, SEXP_NIL,
    SexpType, Sym, ValueApi, ValueError,
};

pub(crate) fn error(ctx: &mut Ctx, call: &Call, text: impl Into<String>) -> Unwind {
    ctx.stop(call.err_call(), text)
}

pub(crate) fn attr(s: &RSession, x: SexpId, name: &str) -> SexpId {
    s.attrs(x)
        .ok()
        .and_then(|attrs| {
            attrs
                .into_iter()
                .find(|(sym, _)| s.sym_name(*sym).ok() == Some(name))
        })
        .map_or(SEXP_NIL, |(_, val)| val)
}

pub(crate) fn set(
    s: &mut RSession,
    x: SexpId,
    name: &str,
    value: SexpId,
) -> Result<(), ValueError> {
    let sym = s.intern(name)?;
    s.set_attrib(x, sym, value)
}

pub(crate) fn chars(s: &RSession, x: SexpId) -> &[CharId] {
    s.str_slice(x).unwrap_or_default()
}

pub(crate) fn ch(s: &mut RSession, text: &str) -> Result<CharId, ValueError> {
    s.mk_char(text.as_bytes(), Encoding::Utf8)
}

pub(crate) fn string(s: &RSession, x: SexpId) -> Option<String> {
    let &id = chars(s, x).first()?;
    (id != CharId::NA).then(|| text(s, id))
}

pub(crate) fn text(s: &RSession, id: CharId) -> String {
    String::from_utf8_lossy(s.char_bytes(id).unwrap_or_default()).into_owned()
}

pub(crate) fn flag(ctx: &mut Ctx, x: SexpId, default: bool) -> EvalResult<bool> {
    if x == SEXP_MISSING || x == SEXP_NIL {
        return Ok(default);
    }
    let (x, _) = ctx.session_mut().coerce(x, SexpType::Lgl)?;
    Ok(ctx
        .session()
        .lgl(x)?
        .iter()
        .next()
        .and_then(RLogical::get)
        .unwrap_or(default))
}

pub(crate) fn number(s: &mut RSession, x: SexpId) -> Result<f64, ValueError> {
    let (x, _) = s.coerce(x, SexpType::Real)?;
    Ok(s.real(x)?.iter().next().unwrap_or(NA_REAL))
}

pub(crate) fn vector_limit(ctx: &mut Ctx, call: &Call, len: usize, width: usize) -> EvalResult<()> {
    if len.saturating_mul(width) > ctx.host().limits().max_vector_bytes {
        return Err(error(ctx, call, "vector size limit exceeded"));
    }
    Ok(())
}

pub(crate) fn count(ctx: &mut Ctx, call: &Call, n: f64) -> EvalResult<usize> {
    if !n.is_finite() || n < 0.0 || n >= usize::MAX as f64 {
        return Err(error(ctx, call, "invalid 'length' argument"));
    }
    Ok(n as usize)
}

pub(crate) fn list(s: &mut RSession, values: &[SexpId]) -> Result<SexpId, ValueError> {
    let x = s.alloc_list(values.len())?;
    for (i, &v) in values.iter().enumerate() {
        s.list_set(x, i, v)?;
    }
    Ok(x)
}

pub(crate) fn names(s: &RSession, x: SexpId) -> &[CharId] {
    chars(s, attr(s, x, "names"))
}

pub(crate) fn set_names(s: &mut RSession, x: SexpId, vals: &[CharId]) -> Result<(), ValueError> {
    let n = s.alloc_str_from(vals)?;
    set(s, x, "names", n)
}

pub(crate) fn tag(s: &RSession, name: Option<Sym>) -> Option<&str> {
    name.and_then(|sym| s.sym_name(sym).ok())
}

// Match exact names, then unique prefixes, then unnamed arguments.
pub(crate) fn formals<const N: usize>(s: &RSession, args: &Args, names: [&str; N]) -> [SexpId; N] {
    let mut matched = [SEXP_MISSING; N];
    let mut used = [false; N];
    for (sym, value) in args.iter().filter(|(sym, _)| sym.is_some()) {
        if let Some(i) = names.iter().position(|n| Some(*n) == tag(s, sym)) {
            matched[i] = value;
            used[i] = true;
        }
    }
    for (sym, value) in args.iter() {
        let Some(prefix) = tag(s, sym) else { continue };
        if names.contains(&prefix) {
            continue;
        }
        let mut possible = names
            .iter()
            .enumerate()
            .filter(|(_, n)| n.starts_with(prefix));
        if let Some((i, _)) = possible.next()
            && possible.next().is_none()
            && !used[i]
        {
            matched[i] = value;
            used[i] = true;
        }
    }
    for ((_, value), (i, _)) in args
        .iter()
        .filter(|(sym, _)| sym.is_none())
        .zip(used.iter().enumerate().filter(|(_, used)| !**used))
    {
        matched[i] = value;
    }
    matched
}

pub(crate) fn rank(t: SexpType) -> u8 {
    use SexpType::*;
    match t {
        Nil => 0,
        Raw => 1,
        Lgl => 2,
        Int => 3,
        Real => 4,
        Cplx => 5,
        Str => 6,
        Expr => 7,
        _ => 8,
    }
}

pub(crate) fn atomic(t: SexpType) -> bool {
    (1..=6).contains(&rank(t))
}

pub(crate) fn gather(
    s: &mut RSession,
    x: SexpId,
    indices: &[Option<usize>],
) -> Result<SexpId, ValueError> {
    macro_rules! gather {
        ($read:ident, $alloc:ident, $missing:expr) => {{
            let src = s.$read(x)?;
            let out: Vec<_> = indices
                .iter()
                .map(|i| {
                    i.filter(|&j| j < src.len())
                        .map_or($missing, |j| src.get(j))
                })
                .collect();
            s.$alloc(&out)
        }};
    }
    match s.type_of(x)? {
        SexpType::Nil => Ok(SEXP_NIL),
        SexpType::Raw => gather!(raw, alloc_raw_from, 0),
        SexpType::Lgl => gather!(lgl, alloc_lgl_from, RLogical::NA),
        SexpType::Int => gather!(int, alloc_int_from, NA_INTEGER),
        SexpType::Real => gather!(real, alloc_real_from, NA_REAL),
        SexpType::Cplx => gather!(cplx, alloc_cplx_from, RComplex::NA),
        SexpType::Str => {
            let src = s.str_slice(x)?;
            let out: Vec<_> = indices
                .iter()
                .map(|i| i.and_then(|j| src.get(j)).copied().unwrap_or(CharId::NA))
                .collect();
            s.alloc_str_from(&out)
        }
        ty => {
            let src = if matches!(ty, SexpType::List | SexpType::Lang) {
                s.pairlist_iter(x).map(|(_, v)| v).collect()
            } else {
                s.list_slice(x)?.to_vec()
            };
            let out: Vec<_> = indices
                .iter()
                .map(|i| i.and_then(|j| src.get(j)).copied().unwrap_or(SEXP_NIL))
                .collect();
            match ty {
                SexpType::Expr => s.alloc_expr_from(&out),
                SexpType::List | SexpType::Lang => {
                    let tags: Vec<_> = s.pairlist_iter(x).map(|(tag, _)| tag).collect();
                    let mut result = SEXP_NIL;
                    for (&value, i) in out.iter().zip(indices).rev() {
                        let tag = i.and_then(|j| tags.get(j)).copied().flatten();
                        result = s.cons(value, result, tag.map_or(SEXP_NIL, Sym::as_sexp))?;
                    }
                    Ok(result)
                }
                _ => list(s, &out),
            }
        }
    }
}

pub(crate) fn element(s: &mut RSession, x: SexpId, i: usize) -> Result<SexpId, ValueError> {
    match s.type_of(x)? {
        SexpType::Vec | SexpType::Expr => s.list_get(x, i),
        SexpType::List | SexpType::Lang => {
            Ok(s.pairlist_iter(x).nth(i).map_or(SEXP_NIL, |(_, v)| v))
        }
        _ => gather(s, x, &[Some(i)]),
    }
}

pub(crate) fn concatenate(
    s: &mut RSession,
    values: &[SexpId],
    ty: SexpType,
) -> Result<SexpId, ValueError> {
    macro_rules! join {
        ($read:ident, $alloc:ident) => {{
            let mut out = Vec::new();
            for &x in values {
                let (x, _) = s.coerce(x, ty)?;
                out.extend(s.$read(x)?.iter());
            }
            s.$alloc(&out)
        }};
    }
    match ty {
        SexpType::Nil => Ok(SEXP_NIL),
        SexpType::Raw => join!(raw, alloc_raw_from),
        SexpType::Lgl => join!(lgl, alloc_lgl_from),
        SexpType::Int => join!(int, alloc_int_from),
        SexpType::Real => join!(real, alloc_real_from),
        SexpType::Cplx => join!(cplx, alloc_cplx_from),
        SexpType::Str => {
            let mut out = Vec::new();
            for &x in values {
                let (x, _) = s.coerce(x, ty)?;
                out.extend_from_slice(s.str_slice(x)?);
            }
            s.alloc_str_from(&out)
        }
        SexpType::Expr => s.alloc_expr_from(values),
        _ => list(s, values),
    }
}

pub(crate) fn copy_attrs(
    s: &mut RSession,
    src: SexpId,
    out: SexpId,
    keep: impl Fn(&str) -> bool,
) -> Result<(), ValueError> {
    for (sym, value) in s.attrs(src)? {
        if keep(s.sym_name(sym)?) && s.attr(out, sym) == SEXP_NIL {
            s.set_attrib(out, sym, value)?;
        }
    }
    Ok(())
}
