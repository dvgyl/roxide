//! Bridge the grid viewport tree to the active graphics device.

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_value::{SEXP_NIL, ValueApi};

use crate::engine::{GraphicsExt, Rect, Rgba, open_default_device};

fn ensure_device(ctx: &mut Ctx) -> EvalResult<()> {
    if ctx.ext::<GraphicsExt>().current == 1 {
        open_default_device(ctx)?;
    }
    Ok(())
}

pub(super) fn device_info(ctx: &mut Ctx, call: &Call, _: &mut Args) -> EvalResult {
    ensure_device(ctx)?;
    let Some(slot) = ctx.ext::<GraphicsExt>().active() else {
        return Err(ctx.stop(call.err_call(), "no active graphics device"));
    };
    let (w, h) = slot.device.size_in();
    let size = super::gpar::pointsize(slot.device.as_ref());
    Ok(ctx.session_mut().alloc_real_from(&[w, h, size])?)
}

pub(super) fn state(ctx: &mut Ctx, _: &Call, _: &mut Args) -> EvalResult {
    ensure_device(ctx)?;
    Ok(ctx
        .ext::<GraphicsExt>()
        .active()
        .map_or(SEXP_NIL, |slot| slot.grid_state))
}

pub(super) fn set_state(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let Some(slot) = ctx.ext::<GraphicsExt>().active_mut() else {
        return Err(ctx.stop(call.err_call(), "no active graphics device"));
    };
    slot.grid_state = args.get(0);
    Ok(SEXP_NIL)
}

pub(super) fn page(ctx: &mut Ctx, call: &Call, _: &mut Args) -> EvalResult {
    ensure_device(ctx)?;
    let id = ctx.ext::<GraphicsExt>().current;
    let Some(mut slot) = ctx
        .ext::<GraphicsExt>()
        .slots
        .get_mut(id)
        .and_then(Option::take)
    else {
        return Err(ctx.stop(call.err_call(), "no active graphics device"));
    };
    let result = slot.device.new_page(ctx.host(), Rgba::TRANSPARENT);
    if result.is_ok() {
        slot.display_list.clear();
        slot.page_count += 1;
        slot.frame_open = false;
        slot.grid_state = SEXP_NIL;
    }
    ctx.ext::<GraphicsExt>().slots[id] = Some(slot);
    result.map_err(|err| ctx.stop(call.err_call(), err.to_string()))?;
    Ok(SEXP_NIL)
}

pub(super) fn clip(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let limits = super::numbers(ctx, call, args.get(0))?;
    let [x0, y0, x1, y1] = limits.as_slice() else {
        return Err(ctx.stop(call.err_call(), "invalid clip rectangle"));
    };
    let Some(slot) = ctx.ext::<GraphicsExt>().active_mut() else {
        return Err(ctx.stop(call.err_call(), "no active graphics device"));
    };
    slot.device
        .clip(Rect::normalized(x0 * 72.0, y0 * 72.0, x1 * 72.0, y1 * 72.0));
    Ok(SEXP_NIL)
}
