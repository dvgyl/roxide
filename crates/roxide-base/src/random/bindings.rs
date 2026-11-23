use roxide_eval::{
    ArgEval, Args, Arity, Call, Ctx, Dispatch, EvalResult, Module, PrimDef, PrimFn, PrimKind,
    Visibility,
};
use roxide_gc::SexpId;
use roxide_math::rng::{Kinds, NormalKind, Rng, SampleKind, UniformKind};
use roxide_value::{CharId, Encoding, NA_INTEGER, RLogical, SEXP_NIL, SexpType, ValueApi};

use super::{get_rng, put_rng, query_kinds, read_kinds, time_seed, with_rng};

pub(super) const MODULE: Module = Module {
    prims: &[
        internal("rox_set_seed", set_seed, 4, Visibility::Off),
        internal("rox_rng_kind", rng_kind, 3, Visibility::SetByFn),
        internal("rox_sample_int", sample_int, 5, Visibility::On),
    ],
    prelude: r#"
set.seed <- function(seed, kind = NULL, normal.kind = NULL, sample.kind = NULL)
    .Internal(rox_set_seed(seed, kind, normal.kind, sample.kind))
RNGkind <- function(kind = NULL, normal.kind = NULL, sample.kind = NULL)
    .Internal(rox_rng_kind(kind, normal.kind, sample.kind))
sample.int <- function(n, size = n, replace = FALSE, prob = NULL,
                       useHash = n > 10000000 && !replace && is.null(prob) && size <= n / 2) {
    if (length(n) != 1L) stop("length(n) == 1L is not TRUE")
    .Internal(rox_sample_int(n, size, replace, prob, useHash))
}
sample <- function(x, size, replace = FALSE, prob = NULL) {
    if (length(x) == 1L && is.numeric(x) && is.finite(x) && x >= 1) {
        if (missing(size)) size <- x
        sample.int(x, size, replace, prob)
    } else {
        n <- length(x)
        if (missing(size)) size <- n
        x[sample.int(n, size, replace, prob)]
    }
}
"#,
};

const fn internal(name: &'static str, fun: PrimFn, n: usize, visibility: Visibility) -> PrimDef {
    PrimDef {
        name,
        fun,
        kind: PrimKind::Internal,
        args: ArgEval::Eval,
        arity: Arity::Exact(n),
        dispatch: Dispatch::None,
        visibility,
    }
}

const UNIFORM: [(&str, UniformKind); 9] = [
    ("Wichmann-Hill", UniformKind::WichmannHill),
    ("Marsaglia-Multicarry", UniformKind::MarsagliaMulticarry),
    ("Super-Duper", UniformKind::SuperDuper),
    ("Mersenne-Twister", UniformKind::MersenneTwister),
    ("Knuth-TAOCP", UniformKind::KnuthTaocp),
    ("user-supplied", UniformKind::UserSupplied),
    ("Knuth-TAOCP-2002", UniformKind::KnuthTaocp2002),
    ("L'Ecuyer-CMRG", UniformKind::LecuyerCmrg),
    ("default", UniformKind::MersenneTwister),
];

const NORMAL: [(&str, NormalKind); 7] = [
    ("Buggy Kinderman-Ramage", NormalKind::BuggyKindermanRamage),
    ("Ahrens-Dieter", NormalKind::AhrensDieter),
    ("Box-Muller", NormalKind::BoxMuller),
    ("user-supplied", NormalKind::UserNorm),
    ("Inversion", NormalKind::Inversion),
    ("Kinderman-Ramage", NormalKind::KindermanRamage),
    ("default", NormalKind::Inversion),
];

const SAMPLE: [(&str, SampleKind); 3] = [
    ("Rounding", SampleKind::Rounding),
    ("Rejection", SampleKind::Rejection),
    ("default", SampleKind::Rejection),
];

fn match_kind<T: Copy>(
    ctx: &mut Ctx,
    call: &Call,
    arg: SexpId,
    name: &str,
    choices: &[(&str, T)],
) -> EvalResult<Option<T>> {
    if arg == SEXP_NIL {
        return Ok(None);
    }
    let Ok([choice]) = ctx.session().str_slice(arg) else {
        let suffix = if name == "kind" {
            " (RNG to be used)."
        } else {
            ""
        };
        return Err(ctx.stop(
            call.err_call(),
            format!("'{name}' must be a character string{suffix}"),
        ));
    };
    let text = if *choice == CharId::NA {
        "NA".to_owned()
    } else {
        String::from_utf8_lossy(ctx.session().char_bytes(*choice)?).into_owned()
    };
    if let Some(&(_, kind)) = choices.iter().find(|(key, _)| *key == text) {
        return Ok(Some(kind));
    }
    if !text.is_empty() {
        let mut matches = choices.iter().filter(|(key, _)| key.starts_with(&text));
        if let Some(&(_, kind)) = matches.next()
            && matches.next().is_none()
        {
            return Ok(Some(kind));
        }
    }
    let error = if name == "kind" {
        format!("'{text}' is not a valid abbreviation of an RNG")
    } else {
        "'NA' is not a valid choice".into()
    };
    Err(ctx.stop(call.err_call(), error))
}

struct Changes {
    uniform: Option<UniformKind>,
    normal: Option<NormalKind>,
    sample: Option<SampleKind>,
}

impl Changes {
    fn parse(ctx: &mut Ctx, call: &Call, args: &[SexpId]) -> EvalResult<Self> {
        let uniform = match_kind(ctx, call, args[0], "kind", &UNIFORM)?;
        let normal = match_kind(ctx, call, args[1], "normal.kind", &NORMAL)?;
        let sample = match_kind(ctx, call, args[2], "sample.kind", &SAMPLE)?;
        if sample == Some(SampleKind::Rounding) {
            ctx.warn_now(call.err_call(), "non-uniform 'Rounding' sampler used")?;
        }
        Ok(Self {
            uniform,
            normal,
            sample,
        })
    }

    fn validate(&self, ctx: &mut Ctx, call: &Call, old: Kinds) -> EvalResult<()> {
        if self.uniform == Some(UniformKind::UserSupplied) {
            return Err(ctx.stop(call.err_call(), "'user_unif_rand' not in load table"));
        }
        if self.normal == Some(NormalKind::UserNorm) {
            return Err(ctx.stop(call.err_call(), "'user_norm_rand' not in load table"));
        }
        if self.uniform == Some(UniformKind::MarsagliaMulticarry) {
            ctx.warn_now(
                call.err_call(),
                "RNGkind: Marsaglia-Multicarry has poor statistical properties",
            )?;
        }
        if self.normal == Some(NormalKind::BuggyKindermanRamage) {
            ctx.warn_now(
                call.err_call(),
                "buggy version of Kinderman-Ramage generator used",
            )?;
        }
        if let Some(normal) = self.normal
            && self.apply(old).uniform == UniformKind::MarsagliaMulticarry
        {
            let diagnostic = match normal {
                NormalKind::AhrensDieter => Some(
                    "RNGkind: deviations from normality for Ahrens-Dieter + Marsaglia-Multicarry",
                ),
                NormalKind::KindermanRamage => Some(
                    "RNGkind: severe deviations from normality for Kinderman-Ramage + Marsaglia-Multicarry",
                ),
                _ => None,
            };
            if let Some(message) = diagnostic {
                ctx.warn_now(call.err_call(), message)?;
            }
        }
        Ok(())
    }

    fn apply(&self, old: Kinds) -> Kinds {
        Kinds {
            uniform: self.uniform.unwrap_or(old.uniform),
            normal: self.normal.unwrap_or(old.normal),
            sample: self.sample.unwrap_or(old.sample),
        }
    }
}

fn set_seed(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let changes = Changes::parse(ctx, call, &args.values()[1..])?;
    let seed = args.values()[0];
    let seed = if seed == SEXP_NIL {
        time_seed(ctx)
    } else {
        let ty = ctx.session().type_of(seed)?;
        if !matches!(
            ty,
            SexpType::Lgl
                | SexpType::Int
                | SexpType::Real
                | SexpType::Cplx
                | SexpType::Str
                | SexpType::Raw
        ) {
            return Err(ctx.stop(call.err_call(), "supplied seed is not a valid integer"));
        }
        let first = if ctx.session().xlength(seed)? > 1 {
            crate::ops::common::element(ctx.session_mut(), seed, 0)?
        } else {
            seed
        };
        ctx.protect(first);
        let (value, warn) = ctx.session_mut().coerce(first, SexpType::Int)?;
        let words = ctx.session().int(value)?;
        let seed = if !words.is_empty() {
            words.get(0)
        } else {
            NA_INTEGER
        };
        if warn.na_introduced {
            let message = if ty == SexpType::Real {
                "NAs introduced by coercion to integer range"
            } else {
                "NAs introduced by coercion"
            };
            ctx.warn_now(call.err_call(), message)?;
        }
        if warn.imaginary_discarded {
            ctx.warn_now(call.err_call(), "imaginary parts discarded in coercion")?;
        }
        if seed == NA_INTEGER {
            return Err(ctx.stop(call.err_call(), "supplied seed is not a valid integer"));
        }
        seed as u32
    };
    let kinds = read_kinds(ctx, call)?;
    if changes.normal == Some(NormalKind::BuggyKindermanRamage) {
        return Err(ctx.stop(
            call.err_call(),
            "buggy version of Kinderman-Ramage generator is not allowed",
        ));
    }
    changes.validate(ctx, call, kinds)?;
    put_rng(ctx, &Rng::seeded(seed, changes.apply(kinds)))?;
    Ok(SEXP_NIL)
}

fn rng_kind(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let changes = Changes::parse(ctx, call, args.values())?;
    let query = changes.uniform.is_none() && changes.normal.is_none() && changes.sample.is_none();
    let old = if query {
        query_kinds(ctx, call)?
    } else {
        let mut rng = get_rng(ctx, call)?;
        let old = rng.kinds;
        changes.validate(ctx, call, old)?;
        if let Some(kind) = changes.uniform {
            rng.switch_uniform(kind);
        }
        if changes.normal.is_some() || changes.uniform.is_some() {
            rng.bm_saved = None;
        }
        rng.kinds = changes.apply(old);
        put_rng(ctx, &rng)?;
        old
    };
    let names = [
        UNIFORM[old.uniform as usize].0,
        NORMAL[old.normal as usize].0,
        SAMPLE[old.sample as usize].0,
    ];
    let mut chars = [CharId::NA; 3];
    for (slot, name) in chars.iter_mut().zip(names) {
        *slot = ctx
            .session_mut()
            .mk_char(name.as_bytes(), Encoding::Native)?;
    }
    let value = ctx.session_mut().alloc_str_from(&chars)?;
    ctx.set_visible(query);
    Ok(value)
}

fn scalar_real(ctx: &mut Ctx, value: SexpId) -> EvalResult<f64> {
    if ctx.session().xlength(value)? != 1 {
        return Ok(f64::NAN);
    }
    let (value, _) = ctx.session_mut().coerce(value, SexpType::Real)?;
    let values = ctx.session().real(value)?;
    Ok(if values.len() == 1 {
        values.get(0)
    } else {
        f64::NAN
    })
}

fn logical(ctx: &mut Ctx, value: SexpId) -> EvalResult<Option<bool>> {
    if ctx.session().xlength(value)? != 1 {
        return Ok(None);
    }
    let (value, _) = ctx.session_mut().coerce(value, SexpType::Lgl)?;
    let flags = ctx.session().lgl(value)?;
    Ok(match (flags.len() == 1).then(|| flags.get(0)) {
        Some(RLogical::TRUE) => Some(true),
        Some(RLogical::FALSE) => Some(false),
        _ => None,
    })
}

fn weights(
    ctx: &mut Ctx,
    call: &Call,
    prob: SexpId,
    n: u64,
    size: usize,
    replace: bool,
) -> EvalResult<Vec<f64>> {
    let len = ctx.session().xlength(prob)?;
    if len as u64 != n {
        return Err(ctx.stop(call.err_call(), "incorrect number of probabilities"));
    }
    let (prob, warning) = ctx.session_mut().coerce(prob, SexpType::Real)?;
    let weights = ctx.session().real(prob)?;
    let mut weights = if weights.len() as u64 == n {
        Some(weights.to_vec())
    } else {
        None
    };
    if warning.na_introduced {
        ctx.warn_now(call.err_call(), "NAs introduced by coercion")?;
    }
    let weights = weights
        .as_mut()
        .ok_or_else(|| ctx.stop(call.err_call(), "incorrect number of probabilities"))?;
    let mut total = 0.0;
    let mut positive = 0;
    for &weight in weights.iter() {
        if !weight.is_finite() {
            return Err(ctx.stop(call.err_call(), "NA in probability vector"));
        }
        if weight < 0.0 {
            return Err(ctx.stop(call.err_call(), "negative probability"));
        }
        if weight > 0.0 {
            total += weight;
            positive += 1;
        }
    }
    if positive == 0 || (!replace && positive < size) {
        return Err(ctx.stop(call.err_call(), "too few positive probabilities"));
    }
    for p in weights.iter_mut() {
        *p /= total;
    }
    Ok(std::mem::take(weights))
}

fn sample_int(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [population, amount, replace, prob, hash] = args.values() else {
        return Err(ctx.stop(call.err_call(), "invalid sampling arguments"));
    };
    let population = scalar_real(ctx, *population)?;
    let amount = scalar_real(ctx, *amount)?;
    let Some(replace) = logical(ctx, *replace)? else {
        return Err(ctx.stop(call.err_call(), "invalid 'replace' argument"));
    };
    let Some(hash) = logical(ctx, *hash)? else {
        return Err(ctx.stop(call.err_call(), "missing value where TRUE/FALSE needed"));
    };
    if !(0.0..=4.5e15).contains(&population) {
        return Err(ctx.stop(call.err_call(), "invalid first argument"));
    }
    let weighted = *prob != SEXP_NIL;
    let n = population as u64;
    if amount.is_infinite() {
        return Err(ctx.stop(call.err_call(), "vector size cannot be infinite"));
    }
    if amount > 4.5e15 {
        return Err(ctx.stop(call.err_call(), "vector size specified is too large"));
    }
    let amount = amount.trunc();
    if amount.is_nan() || amount < 0.0 {
        return Err(ctx.stop(call.err_call(), "invalid 'size' argument"));
    }
    let size = amount as usize;
    if population == 0.0 && size > 0 {
        return Err(ctx.stop(call.err_call(), "invalid first argument"));
    }
    if hash {
        if replace {
            return Err(ctx.stop(call.err_call(), "!replace is not TRUE"));
        }
        if weighted {
            return Err(ctx.stop(call.err_call(), "is.null(prob) is not TRUE"));
        }
        if amount > population / 2.0 {
            return Err(ctx.stop(call.err_call(), "This algorithm is for size <= n/2"));
        }
    }
    if !replace && size as u64 > n {
        return Err(ctx.stop(
            call.err_call(),
            "cannot take a sample larger than the population when 'replace = FALSE'",
        ));
    }
    let limit = ctx.host().limits().max_vector_bytes;
    let workspace = if weighted || (!hash && !replace && size > 1) {
        n
    } else {
        0
    };
    let output_bytes = size.saturating_mul(8);
    let workspace_bytes = if hash {
        // Allow for buckets, control bytes, and power-of-two rounding.
        size.saturating_mul(24)
    } else {
        (workspace as usize).saturating_mul(if weighted { 32 } else { 8 })
    };
    if output_bytes.saturating_add(workspace_bytes) > limit {
        return Err(ctx.stop(
            call.err_call(),
            "sampling allocation exceeds the vector size limit",
        ));
    }
    let probabilities = if weighted {
        Some(weights(ctx, call, *prob, n, size, replace)?)
    } else {
        None
    };
    let indices = with_rng(ctx, call, |rng| match (replace, probabilities.as_deref()) {
        (true, Some(p)) => rng.sample_prob_with_replacement(p, size),
        (false, Some(p)) => rng.sample_prob_without_replacement(p, size),
        (true, None) => rng.sample_with_replacement(n, size),
        (false, None) => rng.sample_without_replacement(n, size, hash),
    })?;
    if indices.len() != size {
        return Err(ctx.stop(call.err_call(), "sampling did not converge"));
    }
    if population > f64::from(i32::MAX) {
        let indices: Vec<_> = indices.into_iter().map(|i| i as f64).collect();
        Ok(ctx.session_mut().alloc_real_from(&indices)?)
    } else {
        let indices: Vec<_> = indices.into_iter().map(|i| i as i32).collect();
        Ok(ctx.session_mut().alloc_int_from(&indices)?)
    }
}
