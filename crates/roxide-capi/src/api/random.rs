//! Native random draws and synchronization with the global R seed.

use std::sync::atomic::{AtomicI32, Ordering};

use roxide_eval::{Ctx, CtxExt, EvalResult};
use roxide_gc::{SexpId, Trace, Tracer};
use roxide_math::rng::{Kinds, NormalKind, Rng, SeedError};
use roxide_value::{GLOBAL_ENV, SEXP_NIL, SEXP_UNBOUND, Sym, ValueApi};

use super::vector::api;

/// The active normal generator code.
#[unsafe(no_mangle)]
pub static N01_kind: AtomicI32 = AtomicI32::new(NormalKind::Inversion as i32);

#[derive(Default)]
struct NativeRng {
    stream: Option<Rng>,
    kinds: Kinds,
    saved_spare: Option<(SexpId, f64)>,
}

impl Trace for NativeRng {
    fn trace(&self, _: &mut Tracer<'_>) {}
}

impl CtxExt for NativeRng {}

fn seed_symbol(ctx: &mut Ctx) -> EvalResult<Sym> {
    Ok(ctx.session_mut().intern(".Random.seed")?)
}

fn load(ctx: &mut Ctx) -> EvalResult<Rng> {
    let name = seed_symbol(ctx)?;
    let host = ctx.host();
    let fallback = (host.now().unix_us as u32) ^ host.pid().wrapping_shl(16);
    let Some(seed) = ctx
        .get_local(name, GLOBAL_ENV)?
        .filter(|&seed| seed != SEXP_UNBOUND)
    else {
        return Ok(Rng::seeded(fallback, ctx.ext::<NativeRng>().kinds));
    };
    let words = match ctx.session().int(seed) {
        Ok(words) => words,
        Err(_) => {
            ctx.warn_now(SEXP_NIL, "'.Random.seed' is not an integer vector")?;
            return Ok(Rng::seeded(fallback, Kinds::default()));
        }
    };
    let words: Vec<_> = (0..words.len().min(626)).map(|i| words.get(i)).collect();
    match Rng::decode_with_seed(&words, fallback) {
        Ok(mut rng) => {
            if rng.kinds.normal == NormalKind::BoxMuller
                && let Some((previous, spare)) = ctx.ext::<NativeRng>().saved_spare
                && previous == seed
            {
                rng.bm_saved = Some(spare);
            }
            Ok(rng)
        }
        Err(SeedError::WrongLength) => Err(ctx.stop(SEXP_NIL, "'.Random.seed' has wrong length")),
        Err(_) => {
            ctx.warn_now(SEXP_NIL, "'.Random.seed' has an invalid RNG kind")?;
            Ok(Rng::seeded(fallback, Kinds::default()))
        }
    }
}

fn get_state(ctx: &mut Ctx) -> EvalResult<()> {
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

api! {
    /// Read the global seed into the native random stream.
    fn GetRNGstate() -> () = (); |ctx| {
        get_state(ctx)
    }

    /// Write the native random stream back to the global seed.
    fn PutRNGstate() -> () = (); |ctx| {
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
        let state = ctx.ext::<NativeRng>();
        state.saved_spare = spare.map(|spare| (seed, spare));
        state.stream = None;
        Ok(())
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

    /// Draw a uniform index using the active sampling algorithm.
    fn R_unif_index(n: f64) -> f64 = f64::NAN; |ctx| {
        draw(ctx, |rng| rng.unif_index(n))
    }
}
