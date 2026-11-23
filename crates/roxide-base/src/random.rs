//! Base random streams and synchronization with `.GlobalEnv$.Random.seed`.

#![deny(missing_docs)]

mod bindings;

use roxide_eval::{Call, Ctx, CtxExt, EvalResult, Module};
use roxide_gc::{SexpId, Trace, Tracer};
use roxide_math::rng::{Kinds, NormalKind, Rng, SeedError};
use roxide_value::{GLOBAL_ENV, SEXP_UNBOUND, SexpType, Sym, ValueApi};

/// Registration table for the base random functions.
pub const MODULE: Module = bindings::MODULE;

#[derive(Default)]
struct RngExt {
    bm_saved: Option<f64>,
}

impl Trace for RngExt {
    fn trace(&self, _: &mut Tracer<'_>) {}
}

impl CtxExt for RngExt {}

/// The most recent algorithm selection. Use it only when the seed binding is absent.
#[derive(Default)]
struct LastKinds(Kinds);

impl Trace for LastKinds {
    fn trace(&self, _: &mut Tracer<'_>) {}
}

impl CtxExt for LastKinds {}

fn seed_binding(ctx: &mut Ctx) -> EvalResult<(Sym, Option<SexpId>)> {
    let symbol = ctx.session_mut().intern(".Random.seed")?;
    let value = ctx
        .session()
        .env_get(GLOBAL_ENV, symbol)
        .map(|binding| binding.value)
        .filter(|&value| value != SEXP_UNBOUND);
    Ok((symbol, value))
}

fn header(ctx: &Ctx, value: SexpId) -> EvalResult<Result<Kinds, SeedError>> {
    let ty = ctx.session().type_of(value)?;
    if ty != SexpType::Int {
        return Ok(Err(SeedError::NotInteger(ty.type_name(false))));
    }
    let words = ctx.session().int(value)?;
    Ok(if words.is_empty() {
        Err(SeedError::NaKind)
    } else {
        Kinds::decode(words.get(0))
    })
}

fn seed_warning(ctx: &mut Ctx, call: &Call, value: SexpId, error: SeedError) -> EvalResult<()> {
    let message = match error {
        SeedError::NotInteger(ty) => {
            format!("'.Random.seed' is not an integer vector but of type '{ty}', so ignored")
        }
        SeedError::NaKind => "'.Random.seed[1]' is not a valid integer, so ignored".into(),
        SeedError::InvalidKind if ctx.session().int(value)?.get(0) % 100 <= 7 => {
            "'.Random.seed[1]' is not a valid Normal type, so ignored".into()
        }
        SeedError::InvalidKind => "'.Random.seed[1]' is not a valid RNG kind so ignored".into(),
        SeedError::UserSupplied => {
            "'.Random.seed[1] = 5' but no user-supplied generator, so ignored".into()
        }
        SeedError::WrongLength => {
            return Err(ctx.stop(call.err_call(), "'.Random.seed' has wrong length"));
        }
    };
    ctx.warn_now(call.err_call(), message)
}

fn read_kinds(ctx: &mut Ctx, call: &Call) -> EvalResult<Kinds> {
    let (_, value) = seed_binding(ctx)?;
    let Some(value) = value else {
        return Ok(ctx.ext::<LastKinds>().0);
    };
    match header(ctx, value)? {
        Ok(kinds) => Ok(kinds),
        Err(error) => {
            seed_warning(ctx, call, value, error)?;
            Ok(Kinds::default())
        }
    }
}

fn query_kinds(ctx: &mut Ctx, call: &Call) -> EvalResult<Kinds> {
    let (_, Some(value)) = seed_binding(ctx)? else {
        return Ok(ctx.ext::<LastKinds>().0);
    };
    let valid = header(ctx, value)?.is_ok();
    let rng = get_rng(ctx, call)?;
    if !valid {
        put_rng(ctx, &rng)?;
    }
    Ok(rng.kinds)
}

fn time_seed(ctx: &mut Ctx) -> u32 {
    let host = ctx.host();
    (host.now().unix_us as u32) ^ host.pid().wrapping_shl(16)
}

/// Load the random stream from the global seed binding.
pub fn get_rng(ctx: &mut Ctx, call: &Call) -> EvalResult<Rng> {
    let (_, value) = seed_binding(ctx)?;
    if let Some(value) = value {
        match header(ctx, value)? {
            Ok(kinds) => {
                ctx.ext::<LastKinds>().0 = kinds;
                let view = ctx.session().int(value)?;
                if view.len() == 1 {
                    return Ok(Rng::seeded(time_seed(ctx), kinds));
                }
                // A seed can be a compact vector. Only the longest generator state is needed.
                let words: Vec<_> = (0..view.len().min(626)).map(|i| view.get(i)).collect();
                let mut rng = match Rng::decode_with_seed(&words, time_seed(ctx)) {
                    Ok(rng) => rng,
                    Err(error) => {
                        seed_warning(ctx, call, value, error)?;
                        return Ok(Rng::seeded(time_seed(ctx), kinds));
                    }
                };
                if kinds.normal == NormalKind::BoxMuller {
                    rng.bm_saved = ctx.ext::<RngExt>().bm_saved;
                }
                return Ok(rng);
            }
            Err(error) => {
                seed_warning(ctx, call, value, error)?;
                return Ok(Rng::seeded(time_seed(ctx), Kinds::default()));
            }
        }
    }
    let kinds = ctx.ext::<LastKinds>().0;
    Ok(Rng::seeded(time_seed(ctx), kinds))
}

/// Write the stream to the global seed binding and save the Box-Muller spare.
pub fn put_rng(ctx: &mut Ctx, rng: &Rng) -> EvalResult<()> {
    let mut words = Vec::new();
    rng.encode(&mut words);
    let (symbol, value) = seed_binding(ctx)?;
    let session = ctx.session_mut();
    if let Some(value) = value
        && session
            .env_get(GLOBAL_ENV, symbol)
            .is_some_and(|binding| !binding.locked && !binding.active)
        && session.type_of(value)? == SexpType::Int
        && !session.heap.maybe_shared(value)
        && !session.is_object(value)?
        && session.xlength(value)? == words.len()
        && let Ok(target) = session.int_mut(value)
    {
        target.copy_from_slice(&words);
    } else {
        let value = session.alloc_int_from(&words)?;
        ctx.assign(symbol, value, GLOBAL_ENV)?;
    }
    ctx.ext::<RngExt>().bm_saved = rng.bm_saved;
    ctx.ext::<LastKinds>().0 = rng.kinds;
    Ok(())
}

/// Load the stream, run a sequence of draws, and save the updated stream.
pub fn with_rng<R>(ctx: &mut Ctx, call: &Call, f: impl FnOnce(&mut Rng) -> R) -> EvalResult<R> {
    let mut rng = get_rng(ctx, call)?;
    let result = f(&mut rng);
    put_rng(ctx, &rng)?;
    Ok(result)
}
