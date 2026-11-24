use roxide_gc::SexpId;
use roxide_value::{RLogical, SEXP_MISSING, SEXP_NIL, SexpType, Sym, ValueApi};

use crate::{Args, Call, Ctx, EvalResult};

pub(super) fn make_active_binding(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(args, &["sym", "fun", "env"], call.err_call())?;
    let sym = ctx.matched_arg(args, &matched, 0).unwrap_or(SEXP_NIL);
    let sym = if ctx.session.type_of(sym)? == SexpType::Sym {
        ctx.sym_from_sexp(sym)?
    } else {
        let name = ctx.string(sym, call.err_call())?;
        ctx.session.intern(&name)?
    };
    let fun = ctx.matched_arg(args, &matched, 1).unwrap_or(SEXP_NIL);
    if !matches!(
        ctx.session.type_of(fun)?,
        SexpType::Clo | SexpType::Builtin | SexpType::Special
    ) {
        return Err(ctx.stop(call.err_call(), "invalid function"));
    }
    let env = ctx.require_env(
        ctx.matched_arg(args, &matched, 2).unwrap_or(SEXP_NIL),
        call.err_call(),
    )?;
    if ctx
        .session
        .env_get(env, sym)
        .is_some_and(|binding| !binding.active)
    {
        return Err(ctx.stop(call.err_call(), "symbol already has a regular binding"));
    }
    ctx.session.env_define_binding(
        env,
        sym,
        roxide_value::Binding {
            value: fun,
            active: true,
            locked: false,
            missing: 0,
        },
    )?;
    Ok(SEXP_NIL)
}

pub(super) fn active_binding_function(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let sym = binding_sym(ctx, args.get(0), call.err_call())?;
    let env = ctx.require_env(args.get(1), call.err_call())?;
    let name = ctx.session.sym_name(sym)?.to_owned();
    match ctx.session.env_get(env, sym) {
        Some(binding) if binding.active => Ok(binding.value),
        Some(_) => Err(ctx.stop(call.err_call(), format!("no active binding for \"{name}\""))),
        None => Err(ctx.stop(call.err_call(), format!("no binding for \"{name}\""))),
    }
}

pub(super) fn lock_environment(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let env = ctx.require_env(args.get(0), call.err_call())?;
    let bindings = if args.len() >= 2 {
        ctx.truth(args.get(1), call.err_call())?
    } else {
        false
    };
    ctx.session.env_set_locked(env, true)?;
    if bindings {
        ctx.session.env_lock_all_bindings(env)?;
    }
    Ok(SEXP_NIL)
}

pub(super) fn env_is_locked(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let env = ctx.require_env(args.get(0), call.err_call())?;
    let locked = ctx.session.env_is_locked(env)?;
    Ok(ctx
        .session
        .scalar_lgl(RLogical::from_option(Some(locked)))?)
}

fn binding_sym(ctx: &mut Ctx, val: SexpId, call: SexpId) -> EvalResult<Sym> {
    if ctx.session.type_of(val)? == SexpType::Sym && val != SEXP_MISSING {
        ctx.sym_from_sexp(val)
    } else {
        let name = ctx.string(val, call)?;
        Ok(ctx.session.intern(&name)?)
    }
}

pub(super) fn lock_binding(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let sym = binding_sym(ctx, args.get(0), call.err_call())?;
    let env = ctx.require_env(args.get(1), call.err_call())?;
    if !ctx.session.env_lock_binding(env, sym, true)? {
        let name = ctx.session.sym_name(sym)?.to_owned();
        return Err(ctx.stop(call.err_call(), format!("no binding for \"{name}\"")));
    }
    Ok(SEXP_NIL)
}

pub(super) fn unlock_binding(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let sym = binding_sym(ctx, args.get(0), call.err_call())?;
    let env = ctx.require_env(args.get(1), call.err_call())?;
    if !ctx.session.env_lock_binding(env, sym, false)? {
        let name = ctx.session.sym_name(sym)?.to_owned();
        return Err(ctx.stop(call.err_call(), format!("no binding for \"{name}\"")));
    }
    Ok(SEXP_NIL)
}

pub(super) fn binding_is_locked(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let sym = binding_sym(ctx, args.get(0), call.err_call())?;
    let env = ctx.require_env(args.get(1), call.err_call())?;
    let Some(locked) = ctx.session.env_binding_is_locked(env, sym)? else {
        let name = ctx.session.sym_name(sym)?.to_owned();
        return Err(ctx.stop(call.err_call(), format!("no binding for \"{name}\"")));
    };
    Ok(ctx
        .session
        .scalar_lgl(RLogical::from_option(Some(locked)))?)
}

pub(super) fn binding_is_active(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let sym = binding_sym(ctx, args.get(0), call.err_call())?;
    let env = ctx.require_env(args.get(1), call.err_call())?;
    let Some(active) = ctx.session.env_binding_is_active(env, sym)? else {
        let name = ctx.session.sym_name(sym)?.to_owned();
        return Err(ctx.stop(call.err_call(), format!("no binding for \"{name}\"")));
    };
    Ok(ctx
        .session
        .scalar_lgl(RLogical::from_option(Some(active)))?)
}
