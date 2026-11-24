use std::path::Path;

use roxide_eval::{Args, Call, Ctx, EvalResult, NativeCallKind};
use roxide_value::{CharId, FALSE_SCALAR, SEXP_NIL, TRUE_SCALAR, ValueApi};

use super::{character_scalar, hooks, logical, package};
use crate::ops::common::{set, set_names, text};

pub(super) fn load(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let path = character_scalar(ctx, call, args.get(0), "x")?;
    let local = logical(ctx, call, args.get(1), "local")?;
    let now = logical(ctx, call, args.get(2), "now")?;
    hooks(ctx).dyn_load(ctx, Path::new(&path), local, now, call.err_call())
}

pub(super) fn unload(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let path = character_scalar(ctx, call, args.get(0), "x")?;
    hooks(ctx).dyn_unload(ctx, Path::new(&path), call.err_call())?;
    Ok(SEXP_NIL)
}

pub(super) fn is_loaded(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let symbol = character_scalar(ctx, call, args.get(0), "symbol")?;
    let package = package(ctx, call, args.get(1))?;
    let kind = match character_scalar(ctx, call, args.get(2), "type")?.as_str() {
        "" => None,
        "Call" => Some(NativeCallKind::Call),
        "External" => Some(NativeCallKind::External),
        "C" => Some(NativeCallKind::C),
        "Fortran" => Some(NativeCallKind::Fortran),
        other => {
            return Err(ctx.stop(
                call.err_call(),
                format!("unknown native call type '{other}'"),
            ));
        }
    };
    if hooks(ctx).is_loaded(ctx, &symbol, package.as_deref(), kind)? {
        Ok(TRUE_SCALAR)
    } else {
        Ok(FALSE_SCALAR)
    }
}

pub(super) fn symbol_info(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let Ok(names) = ctx.session().str_slice(args.get(0)) else {
        return Err(ctx.stop(call.err_call(), "'name' must be a character vector"));
    };
    if names.contains(&CharId::NA) {
        return Err(ctx.stop(call.err_call(), "'name' must not contain missing values"));
    }
    let names = names.to_vec();
    let package = package(ctx, call, args.get(1))?;
    let unlist = logical(ctx, call, args.get(2), "unlist")?;
    let registration = logical(ctx, call, args.get(3), "withRegistrationInfo")?;
    let lookup = |ctx: &mut Ctx, name| {
        let name = text(ctx.session(), name);
        hooks(ctx).get_native_symbol_info(
            ctx,
            &name,
            package.as_deref(),
            registration,
            call.err_call(),
        )
    };
    if unlist && let [name] = names.as_slice() {
        return lookup(ctx, *name);
    }
    ctx.scoped(|ctx| {
        let result = ctx.session_mut().alloc_list(names.len())?;
        ctx.protect(result);
        for (i, &name) in names.iter().enumerate() {
            let info = lookup(ctx, name)?;
            ctx.session_mut().list_set(result, i, info)?;
        }
        set_names(ctx.session_mut(), result, &names)?;
        Ok(result)
    })
}

pub(super) fn loaded(ctx: &mut Ctx, _: &Call, _: &mut Args) -> EvalResult {
    if let Some(hooks) = ctx.native_hooks() {
        return hooks.get_loaded_dlls(ctx);
    }
    ctx.scoped(|ctx| {
        let result = ctx.session_mut().alloc_list(0)?;
        ctx.protect(result);
        let class = ctx.session_mut().scalar_str("DLLInfoList")?;
        set(ctx.session_mut(), result, "class", class)?;
        Ok(result)
    })
}

pub(super) fn registered(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let add_names = logical(ctx, call, args.get(1), "addNames")?;
    hooks(ctx).get_dll_registered_routines(ctx, args.get(0), add_names, call.err_call())
}
