use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_value::{SEXP_NIL, ValueApi};

use crate::engine::{GraphicsExt, open_default_device};

use super::{flag, number, set_attr, strings};

fn named_ids(ctx: &mut Ctx, ids: &[usize]) -> EvalResult {
    let names: Vec<_> = {
        let graphics = ctx.ext::<GraphicsExt>();
        ids.iter()
            .filter_map(|&id| graphics.slots.get(id)?.as_ref())
            .map(|slot| slot.device.name().to_owned())
            .collect()
    };
    let ids: Vec<_> = ids.iter().map(|&id| id as i32).collect();
    ctx.scoped(|ctx| {
        let result = ctx.session_mut().alloc_int_from(&ids)?;
        ctx.protect(result);
        let names = strings(ctx, names.iter().map(|name| Some(name.as_str())))?;
        ctx.protect(names);
        set_attr(ctx, result, "names", names)?;
        Ok(result)
    })
}

pub(super) fn current(ctx: &mut Ctx, _: &Call, _: &mut Args) -> EvalResult {
    let id = ctx.ext::<GraphicsExt>().current;
    named_ids(ctx, &[id])
}

pub(super) fn list(ctx: &mut Ctx, _: &Call, _: &mut Args) -> EvalResult {
    let ids = ctx.ext::<GraphicsExt>().open_ids();
    if ids.is_empty() {
        Ok(SEXP_NIL)
    } else {
        named_ids(ctx, &ids)
    }
}

fn which(ctx: &mut Ctx, call: &Call, args: &Args) -> EvalResult<i32> {
    let id = number(ctx, args.get(0))?;
    if id.is_finite() && id > f64::from(i32::MIN) && id <= f64::from(i32::MAX) {
        Ok(id as i32)
    } else {
        Err(ctx.stop(call.err_call(), "NA argument is invalid"))
    }
}

fn neighbor(graphics: &GraphicsExt, id: i32, forward: bool) -> usize {
    let ids = graphics.open_ids();
    let index = ids.partition_point(|&candidate| {
        let candidate = candidate as i32;
        if forward {
            candidate <= id
        } else {
            candidate < id
        }
    });
    if forward {
        ids.get(index).or_else(|| ids.first()).copied().unwrap_or(1)
    } else {
        index
            .checked_sub(1)
            .and_then(|i| ids.get(i))
            .or_else(|| ids.last())
            .copied()
            .unwrap_or(1)
    }
}

pub(super) fn step(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let id = which(ctx, call, args)?;
    let forward = flag(ctx, call, args.get(1), "invalid device direction")?;
    let id = neighbor(ctx.ext::<GraphicsExt>(), id, forward);
    named_ids(ctx, &[id])
}

pub(super) fn set(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let id = which(ctx, call, args)?;
    let graphics = ctx.ext::<GraphicsExt>();
    let requested = id as usize;
    let selected = if graphics.slots.get(requested).is_some_and(Option::is_some) {
        requested
    } else {
        neighbor(graphics, id, true)
    };
    if selected == 1 {
        open_default_device(ctx)?;
    } else {
        graphics.current = selected;
    }
    named_ids(ctx, &[selected])
}

fn close(ctx: &mut Ctx, call: &Call, id: usize) -> EvalResult<()> {
    let graphics = ctx.ext::<GraphicsExt>();
    if let Some(slot) = graphics.slots.get_mut(id).and_then(Option::take) {
        if graphics.current == id {
            graphics.current = neighbor(graphics, id as i32, true);
        }
        let mut slot = slot;
        slot.device
            .close(ctx.host())
            .map_err(|err| ctx.stop(call.err_call(), err.to_string()))?;
    }
    Ok(())
}

pub(super) fn off(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let id = which(ctx, call, args)?;
    if id == 1 {
        return Err(ctx.stop(
            call.err_call(),
            "cannot shut down device 1 (the null device)",
        ));
    }
    close(ctx, call, id as usize)?;
    current(ctx, call, args)
}

pub(super) fn close_all(ctx: &mut Ctx, call: &Call, _: &mut Args) -> EvalResult {
    for id in ctx.ext::<GraphicsExt>().open_ids() {
        close(ctx, call, id)?;
    }
    Ok(SEXP_NIL)
}

pub(super) fn size(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let unit = number(ctx, args.get(0))?;
    if ctx.ext::<GraphicsExt>().current == 1 {
        open_default_device(ctx)?;
    }
    let Some((width, height)) = ctx
        .ext::<GraphicsExt>()
        .active()
        .map(|slot| slot.device.size_in())
    else {
        return Err(ctx.stop(call.err_call(), "no active device"));
    };
    Ok(ctx
        .session_mut()
        .alloc_real_from(&[width * unit, height * unit])?)
}
