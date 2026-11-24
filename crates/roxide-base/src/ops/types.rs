use std::borrow::Cow;

use roxide_eval::{
    AltOp, AltReply, ArgEval, Args, Arity, Call, Ctx, Dispatch, EvalResult, InternalError, Module,
    PrimDef, PrimKind, Unwind, Visibility,
};
use roxide_gc::SexpId;
use roxide_value::{
    CharId, Encoding, IdenticalOpts, NA_INTEGER, RComplex, RLogical, RSession, SEXP_MISSING,
    SEXP_NIL, SexpType, Sortedness, Sym as Symbol, ValueApi, ValueError, identical as same,
    is_nan_not_na,
};

use super::common::{
    self, atomic, attr as get, ch, chars, concatenate, count, element, error, flag, formals, list,
    number, rank, set, string, tag, text, vector_limit,
};

macro_rules! predicate {
    ($($name:ident: $test:expr),* $(,)?) => {$(
        pub(super) fn $name(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
            let s = ctx.session_mut();
            let x = args.get(0);
            let ty = s.type_of(x)?;
            let result = ($test)(&*s, x, ty);
            Ok(s.scalar_lgl(RLogical::from_option(Some(result)))?)
        }
    )*};
}

use super::attributes::has_class;

/// Primitives with default implementations that read ALTREP metadata.
pub(crate) const ALTREP_MODULE: Module = Module {
    prims: &[
        super::primitive!(
            "length",
            length,
            Eval,
            Arity::Exact(1),
            Dispatch::Internal("length"),
            On
        ),
        super::primitive!(
            "is.unsorted",
            is_unsorted,
            Eval,
            Arity::Range(1, 3),
            Dispatch::None,
            On
        ),
    ],
    prelude: "",
};

fn length(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let n = ctx.altrep_len(args.get(0))?;
    let s = ctx.session_mut();
    Ok(match i32::try_from(n) {
        Ok(n) => s.scalar_int(n)?,
        Err(_) => s.scalar_real(n as f64)?,
    })
}

fn is_unsorted(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, na_rm, strictly] = formals(ctx.session(), args, ["x", "na.rm", "strictly"]);
    if x == SEXP_MISSING {
        return Err(error(
            ctx,
            call,
            "argument \"x\" is missing, with no default",
        ));
    }
    let len = ctx.altrep_len(x)?;
    if len < 2 {
        return Ok(roxide_value::FALSE_SCALAR);
    }
    ctx.resolve_altrep(na_rm)?;
    let rm = if na_rm == SEXP_MISSING {
        Some(false)
    } else {
        let s = ctx.session_mut();
        let n = s.xlength(na_rm)?;
        if n > 1 {
            return Err(error(
                ctx,
                call,
                format!("'length = {n}' in coercion to 'logical(1)'"),
            ));
        }
        if n == 0 || !matches!(s.type_of(na_rm)?, Lgl | Int | Real | Cplx) {
            return Err(error(ctx, call, "invalid argument type"));
        }
        let (value, _) = s.coerce(na_rm, Lgl)?;
        s.lgl(value)?.get(0).get()
    };
    let ty = ctx.session().type_of(x)?;
    let metadata = atomic(ty) && !ctx.session().is_object(x)? && !ctx.session().is_s4(x)?;
    let sorted = if metadata {
        sortedness(ctx, x)?
    } else {
        Sortedness::Unknown
    };
    let increasing = matches!(
        sorted,
        Sortedness::Increasing | Sortedness::IncreasingNaFirst
    );
    let no_na = if metadata && !(increasing && rm == Some(true)) {
        ctx.altrep_no_na(x)?
    } else {
        false
    };
    if rm != Some(true) && !no_na {
        ctx.resolve_altrep(x)?;
        if any_na_value(ctx, call, x, false, 1)? {
            if rm.is_none() {
                return Err(error(ctx, call, "missing value where TRUE/FALSE needed"));
            }
            return Ok(roxide_value::NA_LGL_SCALAR);
        }
    }
    ctx.resolve_altrep(strictly)?;
    let strict = if strictly == SEXP_MISSING {
        false
    } else {
        let (value, _) = ctx.session_mut().coerce(strictly, Lgl)?;
        let value = ctx
            .session()
            .lgl(value)?
            .iter()
            .next()
            .and_then(RLogical::get);
        value.ok_or_else(|| error(ctx, call, "invalid 'strictly' argument"))?
    };
    let known = match sorted {
        Sortedness::Increasing | Sortedness::IncreasingNaFirst if !strict => Some(false),
        Sortedness::Unsorted if no_na || rm != Some(true) => Some(true),
        Sortedness::Decreasing | Sortedness::DecreasingNaFirst
            if strict && (no_na || rm != Some(true)) =>
        {
            Some(true)
        }
        _ => None,
    };
    let unsorted = if let Some(known) = known {
        known
    } else {
        ctx.resolve_altrep(x)?;
        let s = ctx.session();
        match ty {
            Lgl => scan_order(s.lgl(x)?.iter().filter_map(RLogical::get), strict),
            Int => scan_order(s.int(x)?.iter().filter(|&v| v != NA_INTEGER), strict),
            Real => scan_order(s.real(x)?.iter().filter(|v| !v.is_nan()), strict),
            Cplx => scan_order(
                s.cplx(x)?
                    .iter()
                    .filter(|v| !v.re.is_nan() && !v.im.is_nan())
                    .map(|v| (v.re, v.im)),
                strict,
            ),
            Raw => scan_order(s.raw(x)?.iter(), strict),
            Str => scan_order(
                s.str_slice(x)?
                    .iter()
                    .filter(|&&v| v != CharId::NA)
                    .map(|&v| s.char_bytes(v).ok()),
                strict,
            ),
            _ => return Ok(roxide_value::NA_LGL_SCALAR),
        }
    };
    Ok(ctx
        .session_mut()
        .scalar_lgl(RLogical::from_option(Some(unsorted)))?)
}

fn sortedness(ctx: &mut Ctx, x: SexpId) -> EvalResult<Sortedness> {
    if !ctx
        .session()
        .foreign_alt(x)?
        .is_some_and(|info| !info.owned)
    {
        return ctx.altrep_sorted(x);
    }
    // Ctx::altrep_sorted maps both KNOWN_UNSORTED and UNKNOWN_SORTEDNESS to Unknown.
    let invalid = || {
        Unwind::Internal(InternalError {
            message: "foreign ALTREP sortedness callback unavailable".into(),
        })
    };
    let hooks = ctx.native_hooks().ok_or_else(invalid)?;
    let reply = ctx.scoped(|ctx| {
        ctx.protect(x);
        hooks.altrep(ctx, AltOp::IsSorted(x))
    })?;
    let AltReply::Flag(code) = reply else {
        return Err(invalid());
    };
    Ok(match code {
        0 => Sortedness::Unsorted,
        1 => Sortedness::Increasing,
        2 => Sortedness::IncreasingNaFirst,
        -1 => Sortedness::Decreasing,
        -2 => Sortedness::DecreasingNaFirst,
        _ => Sortedness::Unknown,
    })
}

fn scan_order<T: PartialOrd>(mut values: impl Iterator<Item = T>, strict: bool) -> bool {
    let Some(mut previous) = values.next() else {
        return false;
    };
    for value in values {
        if value < previous || (strict && value == previous) {
            return true;
        }
        previous = value;
    }
    false
}

use SexpType::*;
predicate!(
    is_null: |_, _, ty| ty == Nil,
    is_logical: |_, _, ty| ty == Lgl,
    is_integer: |_, _, ty| ty == Int,
    is_double: |_, _, ty| ty == Real,
    is_numeric: |s, x, ty| matches!(ty, Int | Real) && !has_class(s, x, "factor"),
    is_complex: |_, _, ty| ty == Cplx,
    is_character: |_, _, ty| ty == Str,
    is_raw: |_, _, ty| ty == Raw,
    is_list: |_, _, ty| matches!(ty, Vec | List),
    is_atomic: |_, _, ty| atomic(ty),
    is_function: |_, _, ty| matches!(ty, Clo | Builtin | Special),
    is_environment: |_, _, ty| ty == Env,
    is_matrix: |s: &RSession, x, _| s.xlength(get(s, x, "dim")).ok() == Some(2),
    is_array: |s, x, _| get(s, x, "dim") != SEXP_NIL,
    is_factor: |s, x, _| has_class(s, x, "factor"),
    is_data_frame: |s, x, _| has_class(s, x, "data.frame"),
);

pub(super) fn is_vector(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let [x, mode] = formals(ctx.session(), args, ["x", "mode"]);
    let s = ctx.session_mut();
    let ty = s.type_of(x)?;
    let mode = string(s, mode);
    let valid = match mode.as_deref().unwrap_or("any") {
        "any" => atomic(ty) || matches!(ty, Vec | Expr),
        "numeric" => matches!(ty, Real | Int),
        "list" => ty == Vec,
        "pairlist" => ty == List,
        name => (atomic(ty) || ty == Expr) && name == ty.type_name(false),
    };
    let attrs = s.attrs(x)?;
    let valid = valid
        && attrs
            .iter()
            .all(|(sym, _)| s.sym_name(*sym).ok() == Some("names"));
    Ok(s.scalar_lgl(RLogical::from_option(Some(valid)))?)
}

#[derive(Clone, Copy)]
enum MissingTest {
    Na,
    Nan,
    Finite,
    Infinite,
}

macro_rules! missing_test {
    ($($name:ident:$op:ident),*) => {$(
        pub(super) fn $name(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
            missing(ctx, call, args.get(0), MissingTest::$op)
        }
    )*};
}

missing_test!(is_na:Na, is_nan:Nan, is_finite:Finite, is_infinite:Infinite);

pub(super) fn any_na(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, recursive] = formals(ctx.session(), args, ["x", "recursive"]);
    let s = ctx.session();
    let recurse =
        if recursive != SEXP_MISSING && matches!(s.type_of(x)?, Vec | List) && !s.is_object(x)? {
            ctx.resolve_altrep(recursive)?;
            let len = ctx.session().xlength(recursive)?;
            if len > 1 {
                return Err(error(
                    ctx,
                    call,
                    format!("'length = {len}' in coercion to 'logical(1)'"),
                ));
            }
            let (value, _) = ctx.session_mut().coerce(recursive, Lgl)?;
            let flag = ctx
                .session()
                .lgl(value)?
                .iter()
                .next()
                .and_then(RLogical::get);
            flag.ok_or_else(|| error(ctx, call, "NA in coercion to boolean"))?
        } else {
            false
        };
    let limit = ctx
        .opts()
        .expressions
        .min((ctx.stack_budget() / 4096).max(1));
    let found = any_na_value(ctx, call, x, recurse, limit)?;
    Ok(ctx
        .session_mut()
        .scalar_lgl(RLogical::from_option(Some(found)))?)
}

fn any_na_value(
    ctx: &mut Ctx,
    call: &Call,
    x: SexpId,
    recursive: bool,
    remaining: usize,
) -> EvalResult<bool> {
    let s = ctx.session();
    let ty = s.type_of(x)?;
    if s.is_object(x)? {
        let result = ctx.call_function("is.na", &[(None, x)], call.env)?;
        return Ok(ctx
            .session()
            .lgl(result)?
            .iter()
            .any(|v| v.get() == Some(true)));
    }
    match ty {
        Nil | Raw => Ok(false),
        ty if atomic(ty) => {
            if ctx.altrep_no_na(x)? {
                return Ok(false);
            }
            ctx.resolve_altrep(x)?;
            let s = ctx.session();
            for i in 0..s.xlength(x)? {
                if test_at(s, x, i, MissingTest::Na)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Vec | List => {
            if remaining == 0 {
                return Err(error(ctx, call, "evaluation nested too deeply"));
            }
            ctx.resolve_altrep(x)?;
            let mut tail = x;
            for i in 0..ctx.session().xlength(x)? {
                let value = if ty == List {
                    let (value, next, _) = ctx.session().cons_parts(tail)?;
                    tail = next;
                    value
                } else {
                    ctx.session().list_get(x, i)?
                };
                if recursive {
                    if any_na_value(ctx, call, value, recursive, remaining - 1)? {
                        return Ok(true);
                    }
                } else {
                    if atomic(ctx.session().type_of(value)?) && ctx.altrep_len(value)? == 1 {
                        ctx.resolve_altrep(value)?;
                        if test_at(ctx.session(), value, 0, MissingTest::Na)? {
                            return Ok(true);
                        }
                    }
                }
            }
            Ok(false)
        }
        _ => Err(error(
            ctx,
            call,
            format!(
                "anyNA() applied to non-(list or vector) of type '{}'",
                ty.type_name(false)
            ),
        )),
    }
}

fn missing(ctx: &mut Ctx, call: &Call, x: SexpId, op: MissingTest) -> EvalResult {
    let ty = ctx.session().type_of(x)?;
    if matches!(op, MissingTest::Na) && matches!(ty, Vec | List | Expr) {
        ctx.resolve_altrep_deep(x)?;
    }
    let s = ctx.session_mut();
    let len = s.xlength(x)?;
    let mut out = std::vec::Vec::with_capacity(len);
    for i in 0..len {
        let result = if matches!(ty, Vec | List | Expr) && matches!(op, MissingTest::Na) {
            let v = element(s, x, i)?;
            s.xlength(v)? == 1 && atomic(s.type_of(v)?) && test_at(s, v, 0, op)?
        } else {
            test_at(s, x, i, op)?
        };
        out.push(RLogical::from_option(Some(result)));
    }
    if !atomic(ty) && !matches!(ty, Nil | Vec | List | Expr) && matches!(op, MissingTest::Na) {
        ctx.warn_now(
            call.err_call(),
            format!(
                "is.na() applied to non-(list or vector) of type '{}'",
                ty.type_name(false)
            ),
        )?;
    } else if !atomic(ty) && ty != Nil && !matches!(op, MissingTest::Na) {
        return Err(error(
            ctx,
            call,
            format!(
                "default method not implemented for type '{}'",
                ty.type_name(false)
            ),
        ));
    }
    let s = ctx.session_mut();
    let out = s.alloc_lgl_from(&out)?;
    common::copy_attrs(s, x, out, |n| matches!(n, "names" | "dim" | "dimnames"))?;
    Ok(out)
}

pub(crate) fn is_missing(s: &RSession, x: SexpId, i: usize) -> bool {
    test_at(s, x, i, MissingTest::Na).unwrap_or(false)
}

fn test_at(
    s: &RSession,
    x: SexpId,
    i: usize,
    op: MissingTest,
) -> Result<bool, roxide_value::ValueError> {
    let real_test = match op {
        MissingTest::Na => f64::is_nan,
        MissingTest::Nan => is_nan_not_na,
        MissingTest::Finite => f64::is_finite,
        MissingTest::Infinite => f64::is_infinite,
    };
    Ok(match s.type_of(x)? {
        Real => real_test(s.real(x)?.get(i)),
        Cplx => {
            let v = s.cplx(x)?.get(i);
            match op {
                MissingTest::Finite => v.re.is_finite() && v.im.is_finite(),
                MissingTest::Nan => !v.is_na() && (real_test(v.re) || real_test(v.im)),
                _ => real_test(v.re) || real_test(v.im),
            }
        }
        Int | Lgl => {
            let na = if s.type_of(x)? == Int {
                s.int(x)?.get(i) == NA_INTEGER
            } else {
                s.lgl(x)?.get(i).get().is_none()
            };
            match op {
                MissingTest::Na => na,
                MissingTest::Finite => !na,
                _ => false,
            }
        }
        Str => matches!(op, MissingTest::Na) && s.str_slice(x)?[i] == CharId::NA,
        _ => false,
    })
}

pub(super) fn typeof_(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let ty = if crate::compiler::source(ctx, x).is_some() {
        Bcode
    } else {
        ctx.session().type_of(x)?
    };
    let s = ctx.session_mut();
    Ok(s.scalar_str(ty.type_name(s.is_s4(x)?))?)
}

pub(super) fn storage_mode(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    typeof_(ctx, call, args)
}

pub(super) fn mode(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let s = ctx.session_mut();
    let ty = s.type_of(args.get(0))?;
    let name = match ty {
        Int | Real => "numeric",
        Builtin | Special | Clo => "function",
        Sym => "name",
        Lang => "call",
        _ => ty.type_name(false),
    };
    Ok(s.scalar_str(name)?)
}

pub(super) fn identical(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let [x, y, num, na, attrs, code, env, src] = formals(
        ctx.session(),
        args,
        [
            "x",
            "y",
            "num.eq",
            "single.NA",
            "attrib.as.set",
            "ignore.bytecode",
            "ignore.environment",
            "ignore.srcref",
        ],
    );
    let opts = IdenticalOpts {
        num_eq: flag(ctx, num, true)?,
        single_na: flag(ctx, na, true)?,
        attrib_as_set: flag(ctx, attrs, true)?,
        ignore_bytecode: flag(ctx, code, true)?,
        ignore_environment: flag(ctx, env, false)?,
        ignore_srcref: flag(ctx, src, true)?,
    };
    ctx.resolve_altrep_deep(x)?;
    ctx.resolve_altrep_deep(y)?;
    let s = ctx.session_mut();
    let yes = same(s, x, y, opts)?;
    Ok(s.scalar_lgl(RLogical::from_option(Some(yes)))?)
}

macro_rules! constructor {
    ($($name:ident:$ty:ident),*) => {$(
        pub(super) fn $name(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
            construct(ctx, call, args, SexpType::$ty)
        }
    )*};
}
constructor!(logical:Lgl, integer:Int, double:Real, character:Str, raw:Raw);

fn construct(ctx: &mut Ctx, call: &Call, args: &Args, ty: SexpType) -> EvalResult {
    let [n] = formals(ctx.session(), args, ["length"]);
    let n = if n == SEXP_MISSING {
        0.0
    } else {
        number(ctx.session_mut(), n)?
    };
    let n = count(ctx, call, n)?;
    vector_limit(
        ctx,
        call,
        n,
        match ty {
            Raw => 1,
            Int | Lgl => 4,
            _ => 8,
        },
    )?;
    let s = ctx.session_mut();
    Ok(match ty {
        Raw => s.alloc_raw_from(&vec![0; n])?,
        Lgl => s.alloc_lgl_from(&vec![RLogical::FALSE; n])?,
        Int => s.alloc_int_from(&vec![0; n])?,
        Real => s.alloc_real_from(&vec![0.0; n])?,
        _ => s.alloc_str_from(&vec![CharId::BLANK; n])?,
    })
}

pub(super) fn complex(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [n, re, im] = formals(ctx.session(), args, ["length.out", "real", "imaginary"]);
    let numeric = |s: &mut RSession, v| {
        if v == SEXP_MISSING {
            return Ok(std::vec::Vec::new());
        }
        let (v, _) = s.coerce(v, Real)?;
        Ok::<_, roxide_value::ValueError>(s.real(v)?.to_vec())
    };
    let re = numeric(ctx.session_mut(), re)?;
    let im = numeric(ctx.session_mut(), im)?;
    let n = if n == SEXP_MISSING {
        0
    } else {
        let n = number(ctx.session_mut(), n)?;
        count(ctx, call, n)?
    };
    let n = n.max(re.len()).max(im.len());
    vector_limit(ctx, call, n, size_of::<RComplex>())?;
    let at = |v: &[f64], i| if v.is_empty() { 0.0 } else { v[i % v.len()] };
    let vals: std::vec::Vec<_> = (0..n)
        .map(|i| RComplex {
            re: at(&re, i),
            im: at(&im, i),
        })
        .collect();
    Ok(ctx.session_mut().alloc_cplx_from(&vals)?)
}

macro_rules! coercion {
    ($($name:ident:$ty:ident),*) => {$(
        pub(super) fn $name(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
            for &arg in args.values().iter().skip(1) {
                ctx.resolve_altrep(arg)?;
            }
            convert(ctx, call, args.get(0), SexpType::$ty)
        }
    )*};
}
coercion!(as_logical:Lgl, as_integer:Int, as_double:Real, as_complex:Cplx,
    as_character:Str, as_raw:Raw);

pub(super) fn as_list(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    for &arg in args.values().iter().skip(1) {
        ctx.resolve_altrep(arg)?;
    }
    let s = ctx.session_mut();
    if s.type_of(x)? != Clo {
        return convert(ctx, call, x, Vec);
    }
    let (formals, body, _) = s.closure_parts(x)?;
    let pairs = s
        .pairlist_iter(formals)
        .chain(std::iter::once((None, body)));
    let (tags, values) = pairs.unzip();
    tagged_list(s, tags, values)
}

fn tagged_list(
    s: &mut RSession,
    tags: std::vec::Vec<Option<Symbol>>,
    values: std::vec::Vec<SexpId>,
) -> EvalResult {
    let out = list(s, &values)?;
    if tags.iter().any(Option::is_some) {
        let names = tags
            .into_iter()
            .map(|n| {
                let name = tag(s, n).unwrap_or("").to_owned();
                ch(s, &name)
            })
            .collect::<Result<std::vec::Vec<_>, _>>()?;
        common::set_names(s, out, &names)?;
    }
    Ok(out)
}

pub(super) fn convert(ctx: &mut Ctx, call: &Call, x: SexpId, ty: SexpType) -> EvalResult {
    let from = ctx.session().type_of(x)?;
    if let Some(result) = foreign_coerce(ctx, x, from, ty)? {
        return Ok(result);
    }
    if (from == Vec && atomic(ty)) || (matches!(from, List | Lang) && ty == Str) {
        ctx.resolve_altrep_deep(x)?;
    } else if from != ty || !ctx.session().attrs(x)?.is_empty() {
        ctx.resolve_altrep(x)?;
    }
    let s = ctx.session_mut();
    if ty == Vec && matches!(from, List | Lang | Expr | Env) {
        let pairs: std::vec::Vec<_> = match from {
            Env => s
                .env_entries(x)?
                .into_iter()
                .map(|(n, v)| (Some(n), v.value))
                .collect(),
            Expr => s
                .list_slice(x)?
                .iter()
                .copied()
                .map(|v| (None, v))
                .collect(),
            _ => s.pairlist_iter(x).collect(),
        };
        let (tags, values) = pairs.into_iter().unzip();
        return tagged_list(s, tags, values);
    }
    if ty == Str && has_class(s, x, "factor") {
        let levels = chars(s, get(s, x, "levels"));
        let values = common::factor_labels(s, x, levels)?;
        return Ok(s.alloc_str_from(&values)?);
    }
    if ty == Str && matches!(from, Sym | Lang) {
        if from == Sym {
            let name = s.sym_name_of(x)?.to_owned();
            return Ok(s.scalar_str(&name)?);
        }
        let items: std::vec::Vec<_> = s.pairlist_iter(x).map(|(_, v)| v).collect();
        let mut values = std::vec::Vec::with_capacity(items.len());
        for v in items {
            let lines = roxide_value::deparse(s, v, Default::default())?;
            let value = if let Ok(symbol) = s.sym_name_of(v) {
                symbol.to_owned()
            } else {
                lines.join(" ")
            };
            values.push(ch(s, &value)?);
        }
        return Ok(s.alloc_str_from(&values)?);
    }
    let raw_invalid = if ty == Raw && from != Raw {
        let (ints, _) = s.coerce(x, Int)?;
        s.int(ints)?.iter().any(|v| !(0..=255).contains(&v))
    } else {
        false
    };
    let (mut out, warnings) = s.coerce(x, ty)?;
    if out == x
        && s.attrs(x)?
            .iter()
            .any(|(key, _)| ty != Vec || s.sym_name(*key).ok() != Some("names"))
    {
        out = s.shallow_duplicate(x)?;
        if ty == Vec {
            let names = get(s, x, "names");
            s.clear_attrs(out)?;
            set(s, out, "names", names)?;
        } else {
            s.clear_attrs(out)?;
        }
    }
    ctx.protect(out);
    if warnings.na_introduced {
        ctx.warn_now(call.err_call(), "NAs introduced by coercion")?;
    }
    if warnings.imaginary_discarded {
        ctx.warn_now(call.err_call(), "imaginary parts discarded in coercion")?;
    }
    if raw_invalid {
        ctx.warn_now(
            call.err_call(),
            "out-of-range values treated as 0 in coercion to raw",
        )?;
    }
    Ok(out)
}

fn foreign_coerce(
    ctx: &mut Ctx,
    x: SexpId,
    from: SexpType,
    to: SexpType,
) -> EvalResult<Option<SexpId>> {
    if from == to
        || !ctx
            .session()
            .foreign_alt(x)?
            .is_some_and(|info| !info.owned)
    {
        return Ok(None);
    }
    if matches!(to, Str | Vec) && ctx.session().is_object(x)? {
        ctx.resolve_altrep(get(ctx.session(), x, "class"))?;
        if has_class(ctx.session(), x, "factor") {
            return Ok(None);
        }
    }
    ctx.altrep_method(AltOp::Coerce { x, to })
}

pub(super) fn combine(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let option = |ctx: &Ctx, name| args.named(ctx.session(), name).unwrap_or(SEXP_MISSING);
    let recursive = flag(ctx, option(ctx, "recursive"), false)?;
    let use_names = flag(ctx, option(ctx, "use.names"), true)?;
    let work: std::vec::Vec<_> = args
        .iter()
        .filter_map(|(n, v)| {
            let name = tag(ctx.session(), n).unwrap_or("");
            (!matches!(name, "recursive" | "use.names")).then(|| (name.to_owned(), v))
        })
        .collect();
    if recursive {
        for &(_, val) in &work {
            ctx.resolve_altrep_deep(val)?;
        }
    }
    let s = ctx.session();
    if work.iter().all(|(name, v)| {
        s.type_of(*v).is_ok_and(|ty| {
            (atomic(ty) || ty == Nil)
                && (!use_names || (name.is_empty() && get(s, *v, "names") == SEXP_NIL))
        })
    }) {
        let mut ty = Nil;
        let mut len = 0usize;
        let mut values = std::vec::Vec::with_capacity(work.len());
        for (_, val) in work {
            let t = s.type_of(val)?;
            if rank(t) > rank(ty) {
                ty = t;
            }
            len = len.saturating_add(s.xlength(val)?);
            values.push(val);
        }
        vector_limit(ctx, call, len, if ty == Cplx { 16 } else { 8 })?;
        return Ok(concatenate(ctx.session_mut(), &values, ty)?);
    }
    let limit = ctx.host().limits().max_vector_bytes / size_of::<(CharId, SexpId)>();
    let s = ctx.session_mut();
    let mut parts = Parts {
        values: std::vec::Vec::new(),
        ty: Nil,
        recursive,
        use_names,
        limit,
    };
    for (name, val) in work {
        let prefix = if use_names {
            ch(s, &name)?
        } else {
            CharId::BLANK
        };
        parts.push(s, val, prefix, 0, true)?;
    }
    let (names, values): (std::vec::Vec<_>, std::vec::Vec<_>) = parts.values.into_iter().unzip();
    let out = concatenate(s, &values, parts.ty)?;
    if use_names && out != SEXP_NIL && names.iter().any(|&n| n != CharId::BLANK) {
        common::set_names(s, out, &names)?;
    }
    vector_limit(ctx, call, values.len(), size_of::<SexpId>())?;
    Ok(out)
}

struct Parts {
    values: std::vec::Vec<(CharId, SexpId)>,
    ty: SexpType,
    recursive: bool,
    use_names: bool,
    limit: usize,
}

impl Parts {
    fn push(
        &mut self,
        s: &mut RSession,
        x: SexpId,
        prefix: CharId,
        depth: usize,
        flatten: bool,
    ) -> Result<(), roxide_value::ValueError> {
        let ty = s.type_of(x)?;
        if depth >= 256 || self.values.len() >= self.limit {
            return Err(roxide_value::ValueError::InvalidAttribute(
                "concatenation limit exceeded".into(),
            ));
        }
        if flatten && ty == Nil {
            return Ok(());
        }
        if !flatten || (!atomic(ty) && !matches!(ty, Vec | List | Expr)) {
            self.ty = Vec;
            self.values.push((prefix, x));
            return Ok(());
        }
        let container = matches!(ty, Vec | List | Expr);
        if !container || !self.recursive {
            let ty = if ty == List { Vec } else { ty };
            if rank(ty) > rank(self.ty) {
                self.ty = ty;
            }
        }
        let len = s.xlength(x)?;
        if len > self.limit.saturating_sub(self.values.len()) {
            return Err(roxide_value::ValueError::InvalidAttribute(
                "concatenation limit exceeded".into(),
            ));
        }
        let start = self.values.len();
        let pair_tags: std::vec::Vec<_> = if self.use_names && ty == List {
            s.pairlist_iter(x)
                .map(|(n, _)| tag(s, n).unwrap_or("").to_owned())
                .collect()
        } else {
            vec![]
        };
        let mut generated = std::vec::Vec::new();
        for i in 0..len {
            let name = if !self.use_names {
                CharId::BLANK
            } else if let Some(&name) = common::names(s, x).get(i) {
                name
            } else if let Some(name) = pair_tags.get(i) {
                ch(s, name)?
            } else {
                CharId::BLANK
            };
            let val = element(s, x, i)?;
            if container && self.recursive {
                let path = join_name(s, prefix, name)?;
                let from = self.values.len();
                self.push(s, val, path, depth + 1, true)?;
                if name == CharId::BLANK && prefix != CharId::BLANK {
                    let bytes = crate::strings::utf8(s, prefix)?;
                    for j in from..self.values.len() {
                        if crate::strings::utf8(s, self.values[j].0)?
                            .strip_prefix(bytes.as_ref())
                            .is_some_and(|suffix| !suffix.starts_with(b"."))
                        {
                            generated.push(j);
                        }
                    }
                }
            } else if name == CharId::BLANK && prefix != CharId::BLANK {
                generated.push(self.values.len());
                self.values.push((prefix, val));
            } else {
                self.values.push((join_name(s, prefix, name)?, val));
            }
        }
        if self.values.len() - start > 1 {
            for j in generated {
                let mut bytes = name_utf8(s, prefix)?.into_owned();
                bytes.extend_from_slice((j - start + 1).to_string().as_bytes());
                self.values[j].0 = s.mk_char(&bytes, Encoding::Utf8)?;
            }
        } else if let [j] = generated.as_slice() {
            self.values[*j].0 = prefix;
        }
        Ok(())
    }
}

fn join_name(s: &mut RSession, prefix: CharId, suffix: CharId) -> Result<CharId, ValueError> {
    if prefix == CharId::BLANK {
        return Ok(suffix);
    }
    if suffix == CharId::BLANK {
        return Ok(prefix);
    }
    let mut name = name_utf8(s, prefix)?.into_owned();
    name.push(b'.');
    name.extend_from_slice(&name_utf8(s, suffix)?);
    s.mk_char(&name, Encoding::Utf8)
}

fn name_utf8(s: &RSession, name: CharId) -> Result<Cow<'_, [u8]>, ValueError> {
    if s.char_encoding(name)? == Encoding::Bytes {
        return Err(ValueError::InvalidAttribute(
            "translating strings with \"bytes\" encoding is not allowed".into(),
        ));
    }
    crate::strings::utf8(s, name)
}

pub(super) fn as_vector(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, mode] = formals(ctx.session(), args, ["x", "mode"]);
    ctx.resolve_altrep(mode)?;
    let named = if mode == SEXP_MISSING {
        "any".to_owned()
    } else {
        string(ctx.session(), mode).ok_or_else(|| error(ctx, call, "invalid 'mode' argument"))?
    };
    let ty = match named.as_str() {
        "any" => ctx.session().type_of(x)?,
        "logical" => Lgl,
        "integer" => Int,
        "numeric" | "double" => Real,
        "complex" => Cplx,
        "character" => Str,
        "raw" => Raw,
        "list" => Vec,
        "expression" => Expr,
        "pairlist" => {
            let from = ctx.session().type_of(x)?;
            if let Some(result) = foreign_coerce(ctx, x, from, List)? {
                return Ok(result);
            }
            ctx.resolve_altrep(x)?;
            let items = convert(ctx, call, x, Vec)?;
            let s = ctx.session_mut();
            let ids = chars(s, get(s, items, "names")).to_vec();
            let mut out = SEXP_NIL;
            for (i, value) in s.list_slice(items)?.to_vec().into_iter().enumerate().rev() {
                let tag = if let Some(&id) = ids.get(i).filter(|&&id| id != CharId::BLANK) {
                    let name = text(s, id);
                    s.intern(&name)?.as_sexp()
                } else {
                    SEXP_NIL
                };
                out = s.cons(value, out, tag)?;
            }
            return Ok(out);
        }
        "name" | "symbol" => {
            let from = ctx.session().type_of(x)?;
            if from == Sym {
                return Ok(x);
            }
            if let Some(result) = foreign_coerce(ctx, x, from, Sym)? {
                return Ok(result);
            }
            ctx.resolve_altrep(x)?;
            let value = convert(ctx, call, x, Str)?;
            let name = string(ctx.session(), value)
                .filter(|name| !name.is_empty())
                .ok_or_else(|| error(ctx, call, "invalid argument of type 'symbol'"))?;
            return Ok(ctx.session_mut().intern(&name)?.as_sexp());
        }
        _ => return Err(error(ctx, call, "invalid 'mode' argument")),
    };
    if named == "any" && !matches!(ty, Lgl | Int | Real | Cplx | Str | Raw | Vec) {
        if matches!(ty, Nil | Lang | List | Expr) {
            return Ok(x);
        }
        return Err(error(
            ctx,
            call,
            format!(
                "cannot coerce type '{}' to vector of type 'any'",
                ty.type_name(false)
            ),
        ));
    }
    convert(ctx, call, x, ty)
}

pub(super) fn all_vars(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    use std::collections::HashSet;

    let [expr, max, unique, functions] = formals(
        ctx.session(),
        args,
        ["expr", "max.names", "unique", "functions"],
    );
    let unique = flag(ctx, unique, true)?;
    let functions = flag(ctx, functions, false)?;
    let max = if max == SEXP_MISSING {
        usize::MAX
    } else {
        let max = number(ctx.session_mut(), max)?;
        if max < 0.0 { usize::MAX } else { max as usize }
    };
    let bound = ctx.host().limits().max_vector_bytes;
    let s = ctx.session_mut();
    let mut names = std::vec::Vec::new();
    let mut seen = HashSet::new();
    let mut pending = vec![expr];
    while let Some(value) = pending.pop() {
        if names.len() == max {
            break;
        }
        match s.type_of(value)? {
            Sym if value != SEXP_MISSING && (!unique || seen.insert(value)) => {
                let name = s.sym_name_of(value)?.to_owned();
                names.push(ch(s, &name)?);
            }
            Lang | List | Dot => {
                let (head, tail, _) = s.cons_parts(value)?;
                let start = pending.len();
                pending.extend(s.pairlist_iter(tail).map(|(_, value)| value));
                pending[start..].reverse();
                if functions || s.type_of(value)? != Lang {
                    pending.push(head);
                }
            }
            Expr => pending.extend(s.list_slice(value)?.iter().rev()),
            _ => {}
        }
        if names.len().saturating_add(pending.len()) > bound / size_of::<SexpId>() {
            return Err(error(ctx, call, "vector size limit exceeded"));
        }
    }
    Ok(s.alloc_str_from(&names)?)
}

pub(super) fn environment_name(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    use roxide_value::{BASE_ENV, BASE_NAMESPACE, EMPTY_ENV, GLOBAL_ENV};
    let x = args.get(0);
    let name = match x {
        BASE_ENV | BASE_NAMESPACE => "base".to_owned(),
        EMPTY_ENV => "R_EmptyEnv".to_owned(),
        GLOBAL_ENV => "R_GlobalEnv".to_owned(),
        _ => ctx.namespace_name(x).unwrap_or_else(|| {
            string(ctx.session(), get(ctx.session(), x, "name")).unwrap_or_default()
        }),
    };
    Ok(ctx.session_mut().scalar_str(&name)?)
}
