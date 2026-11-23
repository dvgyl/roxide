//! Base graphics state, coordinate conversion, and drawing.

mod coords;
mod layout;
mod lowlevel;
mod par;

use roxide_eval::{
    ArgEval, Arity, Call, Ctx, Dispatch, EvalResult, Module, PrimDef, PrimFn, PrimKind, Visibility,
};
use roxide_gc::SexpId;
use roxide_value::{CharId, Encoding, RLogical, SEXP_NIL, SexpType, ValueApi};

use crate::engine::{DeviceSlot, GraphicsExt, open_default_device};

/// Evaluator bindings for the base graphics package.
pub const MODULE: Module = Module {
    prims: &[
        internal("C_rx_par", 3, par::par),
        internal("C_rx_layout", 4, layout::layout),
        internal("C_rx_plot_new", 0, coords::plot_new),
        internal("C_rx_plot_window", 5, coords::plot_window),
        internal("C_rx_grconvert", 4, coords::convert),
        internal("C_rx_strmetric", 5, coords::strmetric),
        internal("C_rx_ax_ticks", 5, coords::ax_ticks),
        internal("C_rx_draw", 3, lowlevel::draw),
    ],
    prelude: include_str!("prelude.R"),
};

const fn internal(name: &'static str, arity: usize, fun: PrimFn) -> PrimDef {
    PrimDef {
        name,
        kind: PrimKind::Internal,
        args: ArgEval::Eval,
        arity: Arity::Exact(arity),
        dispatch: Dispatch::None,
        visibility: Visibility::On,
        fun,
    }
}

fn active<'a>(ctx: &'a mut Ctx, call: &Call, open: bool) -> EvalResult<&'a mut DeviceSlot> {
    let id = ctx.ext::<GraphicsExt>().current;
    let id = if open && id == 1 {
        open_default_device(ctx)?
    } else {
        id
    };
    if ctx
        .ext::<GraphicsExt>()
        .slots
        .get(id)
        .is_none_or(Option::is_none)
    {
        return Err(ctx.stop(call.err_call(), "no active graphics device"));
    }
    ctx.ext::<GraphicsExt>()
        .slots
        .get_mut(id)
        .and_then(Option::as_mut)
        .ok_or_else(|| {
            roxide_eval::Unwind::Internal(roxide_eval::InternalError {
                message: "active graphics device disappeared".into(),
            })
        })
}

fn nums(ctx: &mut Ctx, x: SexpId) -> EvalResult<Vec<f64>> {
    let (x, _) = ctx.session_mut().coerce(x, SexpType::Real)?;
    Ok(ctx.session().real(x)?.iter().collect())
}

fn strings(ctx: &mut Ctx, x: SexpId) -> EvalResult<Vec<Option<String>>> {
    let (x, _) = ctx.session_mut().coerce(x, SexpType::Str)?;
    ctx.session()
        .str_slice(x)?
        .iter()
        .map(|&id| {
            if id == CharId::NA {
                Ok(None)
            } else {
                Ok(Some(
                    String::from_utf8_lossy(ctx.session().char_bytes(id)?).into_owned(),
                ))
            }
        })
        .collect()
}

fn string(ctx: &mut Ctx, x: SexpId) -> EvalResult<String> {
    Ok(strings(ctx, x)?
        .into_iter()
        .next()
        .flatten()
        .unwrap_or_default())
}

fn flag(ctx: &mut Ctx, x: SexpId) -> EvalResult<Option<bool>> {
    let (x, _) = ctx.session_mut().coerce(x, SexpType::Lgl)?;
    Ok(ctx.session().lgl(x)?.iter().next().and_then(RLogical::get))
}

fn attr(ctx: &mut Ctx, x: SexpId, name: &str) -> EvalResult {
    let key = ctx.session_mut().intern(name)?;
    Ok(ctx.session().attr(x, key))
}

fn names(ctx: &mut Ctx, x: SexpId) -> EvalResult<Vec<Option<String>>> {
    let names = attr(ctx, x, "names")?;
    strings(ctx, names)
}

fn alloc_strings<'a>(ctx: &mut Ctx, values: impl IntoIterator<Item = &'a str>) -> EvalResult {
    ctx.scoped(|ctx| {
        let mut ids = Vec::new();
        for value in values {
            let id = ctx
                .session_mut()
                .mk_char(value.as_bytes(), Encoding::Utf8)?;
            ctx.protect(id.as_sexp());
            ids.push(id);
        }
        Ok(ctx.session_mut().alloc_str_from(&ids)?)
    })
}

fn element(ctx: &Ctx, list: SexpId, i: usize) -> EvalResult {
    if list == SEXP_NIL || i >= ctx.session().xlength(list)? {
        Ok(SEXP_NIL)
    } else {
        Ok(ctx.session().list_get(list, i)?)
    }
}

fn record(ctx: &mut Ctx, call: &Call, name: &str, args: &[SexpId]) -> EvalResult<()> {
    ctx.scoped(|ctx| {
        let mut tail = SEXP_NIL;
        for &value in args.iter().rev() {
            tail = ctx.session_mut().cons(value, tail, SEXP_NIL)?;
            ctx.protect(tail);
        }
        let name = ctx.session_mut().intern(name)?.as_sexp();
        let expr = ctx.session_mut().lcons(name, tail, SEXP_NIL)?;
        ctx.protect(expr);
        let tail = ctx.session_mut().cons(expr, SEXP_NIL, SEXP_NIL)?;
        ctx.protect(tail);
        let internal = ctx.session_mut().intern(".Internal")?.as_sexp();
        let entry = ctx.session_mut().lcons(internal, tail, SEXP_NIL)?;
        active(ctx, call, false)?.display_list.push(entry);
        Ok(())
    })
}
