use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{RLogical, SEXP_NIL, SexpType, ValueApi};

use crate::engine::GraphicsExt;

use super::{active, alloc_strings, attr, par, string};

fn set_attr(ctx: &mut Ctx, object: SexpId, name: &str, value: SexpId) -> EvalResult<()> {
    ctx.scoped(|ctx| {
        ctx.protect(value);
        let key = ctx.session_mut().intern(name)?;
        Ok(ctx.session_mut().set_attrib(object, key, value)?)
    })
}

pub(super) fn control(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let enabled = match string(ctx, args.get(0))?.as_str() {
        "enable" => true,
        "inhibit" => false,
        _ => return Err(ctx.stop(call.err_call(), "invalid value for 'displaylist'")),
    };
    if ctx.ext::<GraphicsExt>().current == 1 {
        return Err(ctx.stop(
            call.err_call(),
            "dev.control() called without an open graphics device",
        ));
    }
    let slot = active(ctx, call, false)?;
    slot.recording = enabled;
    slot.display_list.set_enabled(enabled);
    if !enabled {
        slot.display_list.clear();
        slot.initial_state = SEXP_NIL;
    }
    Ok(SEXP_NIL)
}

pub(super) fn snapshot(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    if ctx.ext::<GraphicsExt>().current == 1 {
        return Err(ctx.stop(call.err_call(), "no current device to record from"));
    }
    let slot = active(ctx, call, false)?;
    let entries = slot.display_list.entries().to_vec();
    let initial = slot.initial_state;
    ctx.scoped(|ctx| {
        ctx.protect(initial);
        let mut calls = SEXP_NIL;
        for entry in entries.into_iter().rev() {
            calls = ctx.session_mut().cons(entry, calls, SEXP_NIL)?;
            ctx.protect(calls);
        }
        let final_state = par::snapshot(ctx, call)?;
        ctx.protect(final_state);
        let state = ctx.session_mut().alloc_list(2)?;
        ctx.protect(state);
        ctx.session_mut().list_set(state, 0, initial)?;
        ctx.session_mut().list_set(state, 1, final_state)?;
        let pkg = alloc_strings(ctx, ["graphics"])?;
        set_attr(ctx, state, "pkgName", pkg)?;
        let result = ctx.session_mut().alloc_list(2)?;
        ctx.protect(result);
        ctx.session_mut().list_set(result, 0, calls)?;
        ctx.session_mut().list_set(result, 1, state)?;

        let version = ctx.session_mut().alloc_list(1)?;
        ctx.protect(version);
        let numbers = ctx.session_mut().alloc_int_from(&[4, 6, 2])?;
        ctx.session_mut().list_set(version, 0, numbers)?;
        let class = alloc_strings(
            ctx,
            ["R_system_version", "package_version", "numeric_version"],
        )?;
        set_attr(ctx, version, "class", class)?;
        let engine = ctx.session_mut().alloc_int_from(&[17])?;
        set_attr(ctx, result, "engineVersion", engine)?;
        let pid = ctx.host().pid() as i32;
        let pid = ctx.session_mut().alloc_int_from(&[pid])?;
        set_attr(ctx, result, "pid", pid)?;
        set_attr(ctx, result, "Rversion", version)?;
        set_attr(ctx, result, "load", args.get(0))?;
        set_attr(ctx, result, "attach", args.get(1))?;
        let class = alloc_strings(ctx, ["recordedplot"])?;
        set_attr(ctx, result, "class", class)?;
        Ok(result)
    })
}

pub(super) fn replay(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let plot = args.get(0);
    let class = attr(ctx, plot, "class")?;
    if !super::strings(ctx, class)?
        .iter()
        .any(|s| s.as_deref() == Some("recordedplot"))
    {
        return Err(ctx.stop(call.err_call(), "argument is not of class “recordedplot”"));
    }
    let invalid = |ctx: &mut Ctx| ctx.stop(call.err_call(), "invalid recorded plot");
    if ctx.session().type_of(plot)? != SexpType::Vec || ctx.session().xlength(plot)? != 2 {
        return Err(invalid(ctx));
    }
    let entries = ctx.session().list_get(plot, 0)?;
    let state = ctx.session().list_get(plot, 1)?;
    let pkg = attr(ctx, state, "pkgName")?;
    if string(ctx, pkg)? != "graphics"
        || ctx.session().type_of(state)? != SexpType::Vec
        || ctx.session().xlength(state)? != 2
    {
        return Err(invalid(ctx));
    }
    let initial = ctx.session().list_get(state, 0)?;
    let final_state = ctx.session().list_get(state, 1)?;
    let slot = active(ctx, call, true)?;
    let recording = slot.recording;
    let device = ctx.ext::<GraphicsExt>().current;
    let slot = active(ctx, call, false)?;
    slot.recording = false;
    slot.display_list.set_enabled(false);
    let result = (|| {
        par::restore(
            ctx,
            call,
            if initial == SEXP_NIL {
                final_state
            } else {
                initial
            },
        )?;
        let mut replayed = Vec::new();
        let mut tail = entries;
        while tail != SEXP_NIL {
            if ctx.session().type_of(tail)? != SexpType::List {
                return Err(invalid(ctx));
            }
            let (entry, next, _) = ctx.session().cons_parts(tail)?;
            ctx.eval(entry, call.env)?;
            replayed.push(entry);
            tail = next;
        }
        par::restore(ctx, call, final_state)?;
        Ok(replayed)
    })();
    if let Some(slot) = ctx.ext::<GraphicsExt>().slots[device].as_mut() {
        slot.recording = recording;
        slot.display_list.set_enabled(recording);
        if let Ok(entries) = &result {
            slot.display_list.clear();
            slot.initial_state = if recording { initial } else { SEXP_NIL };
            for &entry in entries {
                slot.display_list.push(entry);
            }
        }
    }
    result?;
    Ok(SEXP_NIL)
}

pub(super) fn is_recording(ctx: &mut Ctx, _: &Call, _: &mut Args) -> EvalResult {
    let enabled = ctx
        .ext::<GraphicsExt>()
        .active()
        .is_some_and(|slot| slot.recording);
    Ok(ctx
        .session_mut()
        .alloc_lgl_from(&[RLogical::from_option(Some(enabled))])?)
}
