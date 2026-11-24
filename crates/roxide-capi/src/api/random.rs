//! Native random draws and synchronization with the global R seed.

use std::sync::atomic::{AtomicI32, Ordering};

use roxide_eval::{Ctx, CtxExt, EvalResult};
use roxide_gc::{Trace, Tracer};
use roxide_math::rng::{Kinds, NormalKind, Rng, SampleKind, SeedError};
use roxide_value::{BASE_ENV, GLOBAL_ENV, SEXP_NIL, SEXP_UNBOUND, Sym, ValueApi};

use super::vector::api;

/// The active normal generator code.
#[unsafe(no_mangle)]
pub static N01_kind: AtomicI32 = AtomicI32::new(NormalKind::Inversion as i32);

#[derive(Default)]
struct NativeRng {
    stream: Option<Rng>,
    kinds: Kinds,
}

impl Trace for NativeRng {
    fn trace(&self, _: &mut Tracer<'_>) {}
}

impl CtxExt for NativeRng {}

fn seed_symbol(ctx: &mut Ctx) -> EvalResult<Sym> {
    Ok(ctx.session_mut().intern(".Random.seed")?)
}

fn base_sample_kind(ctx: &mut Ctx) -> EvalResult<Option<SampleKind>> {
    let rng_kind = ctx.session_mut().intern("RNGkind")?;
    if ctx.session().env_get(BASE_ENV, rng_kind).is_none() {
        return Ok(None);
    }
    let internal = ctx.session_mut().intern(".Internal")?;
    let Some(internal) = ctx.session().env_get(BASE_ENV, internal) else {
        return Ok(None);
    };
    if internal.active || ctx.primitive_name(internal.value) != Some(".Internal") {
        return Ok(None);
    }
    // Base stores the last selected kind independently of the seed binding.
    // Its query reads that selection without creating a seed when it is absent.
    let name = ctx.session_mut().intern("rox_rng_kind")?;
    let mut args = SEXP_NIL;
    for _ in 0..3 {
        args = ctx.session_mut().cons(SEXP_NIL, args, SEXP_NIL)?;
    }
    let expr = ctx.session_mut().lcons(name.as_sexp(), args, SEXP_NIL)?;
    let visible = ctx.is_visible();
    let result = ctx.apply(internal.value, &[(None, expr)], SEXP_NIL, BASE_ENV);
    ctx.set_visible(visible);
    let value = result?;
    let kind = ctx
        .session()
        .str_slice(value)?
        .get(2)
        .copied()
        .map(|kind| ctx.session().char_bytes(kind))
        .transpose()?;
    Ok(match kind {
        Some(b"Rounding") => Some(SampleKind::Rounding),
        Some(b"Rejection") => Some(SampleKind::Rejection),
        _ => None,
    })
}

fn load(ctx: &mut Ctx) -> EvalResult<Rng> {
    let name = seed_symbol(ctx)?;
    let host = ctx.host();
    let fallback = (host.now().unix_us as u32) ^ host.pid().wrapping_shl(16);
    let Some(seed) = ctx
        .get_local(name, GLOBAL_ENV)?
        .filter(|&seed| seed != SEXP_UNBOUND)
    else {
        ctx.clear_box_muller_spare();
        return Ok(Rng::seeded(fallback, ctx.ext::<NativeRng>().kinds));
    };
    let words = match ctx.session().int(seed) {
        Ok(words) => words,
        Err(_) => {
            ctx.clear_box_muller_spare();
            ctx.warn_now(SEXP_NIL, "'.Random.seed' is not an integer vector")?;
            return Ok(Rng::seeded(fallback, Kinds::default()));
        }
    };
    let words: Vec<_> = (0..words.len().min(626)).map(|i| words.get(i)).collect();
    let spare = ctx.box_muller_spare(seed, &words);
    match Rng::decode_with_seed(&words, fallback) {
        Ok(mut rng) => {
            if rng.kinds.normal == NormalKind::BoxMuller {
                rng.bm_saved = spare;
            }
            Ok(rng)
        }
        Err(SeedError::WrongLength) => Err(ctx.stop(SEXP_NIL, "'.Random.seed' has wrong length")),
        Err(_) => {
            ctx.clear_box_muller_spare();
            ctx.warn_now(SEXP_NIL, "'.Random.seed' has an invalid RNG kind")?;
            Ok(Rng::seeded(fallback, Kinds::default()))
        }
    }
}

pub(super) fn get_state(ctx: &mut Ctx) -> EvalResult<()> {
    let rng = load(ctx)?;
    N01_kind.store(rng.kinds.normal as i32, Ordering::Relaxed);
    let state = ctx.ext::<NativeRng>();
    state.kinds = rng.kinds;
    state.stream = Some(rng);
    Ok(())
}

pub(super) fn draw<T>(ctx: &mut Ctx, f: impl FnOnce(&mut Rng) -> T) -> EvalResult<T> {
    if ctx.ext::<NativeRng>().stream.is_none() {
        get_state(ctx)?;
    }
    ctx.ext::<NativeRng>()
        .stream
        .as_mut()
        .map(f)
        .ok_or_else(|| ctx.stop(SEXP_NIL, "cannot initialize the random stream"))
}

pub(super) fn put_state(ctx: &mut Ctx) -> EvalResult<()> {
    let Some(rng) = ctx.ext::<NativeRng>().stream.as_ref() else {
        return Ok(());
    };
    let mut words = Vec::new();
    rng.encode(&mut words);
    let spare = rng.bm_saved;
    let name = seed_symbol(ctx)?;
    let seed = ctx.session_mut().alloc_int_from(&words)?;
    ctx.scoped(|ctx| {
        ctx.protect(seed);
        ctx.assign(name, seed, GLOBAL_ENV)
    })?;
    ctx.save_box_muller_spare(seed, words, spare);
    ctx.ext::<NativeRng>().stream = None;
    Ok(())
}

pub(super) fn with_synced_state<T>(
    ctx: &mut Ctx,
    f: impl FnOnce(&mut Ctx) -> EvalResult<T>,
) -> EvalResult<T> {
    let previous = std::mem::take(ctx.ext::<NativeRng>());
    let normal = N01_kind.load(Ordering::Relaxed);
    ctx.ext::<NativeRng>().kinds = previous.kinds;
    let result = get_state(ctx).and_then(|()| f(ctx)).and_then(|value| {
        put_state(ctx)?;
        Ok(value)
    });
    if result.is_err() {
        *ctx.ext::<NativeRng>() = previous;
        N01_kind.store(normal, Ordering::Relaxed);
    }
    result
}

api! {
    /// Read the global seed into the native random stream.
    fn GetRNGstate() -> () = (); |ctx| {
        get_state(ctx)
    }

    /// Write the native random stream back to the global seed.
    fn PutRNGstate() -> () = (); |ctx| {
        put_state(ctx)
    }

    /// Draw a uniform variate.
    fn unif_rand() -> f64 = f64::NAN; |ctx| {
        draw(ctx, Rng::unif_rand)
    }

    /// Draw a standard normal variate.
    fn norm_rand() -> f64 = f64::NAN; |ctx| {
        draw(ctx, Rng::norm_rand)
    }

    /// Draw a unit exponential variate.
    fn exp_rand() -> f64 = f64::NAN; |ctx| {
        draw(ctx, Rng::exp_rand)
    }

    /// Read the sampling algorithm without drawing from the random stream.
    fn R_sample_kind() -> i32 = SampleKind::Rejection as i32; |ctx| {
        if let Some(stream) = &ctx.ext::<NativeRng>().stream {
            return Ok(stream.kinds.sample as i32);
        }
        let name = seed_symbol(ctx)?;
        let kind = match ctx.session().env_get(GLOBAL_ENV, name) {
            None => base_sample_kind(ctx)?,
            Some(binding) if !binding.active => ctx
                .session()
                .int(binding.value)
                .ok()
                .filter(|words| !words.is_empty())
                .and_then(|words| Kinds::decode(words.get(0)).ok())
                .map(|kinds| kinds.sample),
            Some(_) => None,
        };
        let state = ctx.ext::<NativeRng>();
        if let Some(kind) = kind {
            state.kinds.sample = kind;
        }
        Ok(state.kinds.sample as i32)
    }

    /// Draw a uniform index using the active sampling algorithm.
    fn R_unif_index(n: f64) -> f64 = f64::NAN; |ctx| {
        draw(ctx, |rng| rng.unif_index(n))
    }
}
