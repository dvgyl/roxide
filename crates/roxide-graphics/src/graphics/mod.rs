//! Base graphics state, coordinate conversion, and drawing.

mod coords;
mod histogram;
mod layout;
mod lowlevel;
mod par;
mod replay;

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
        internal("C_rx_require_frame", 0, coords::check_frame),
        internal("C_rx_plot_window", 5, coords::plot_window),
        internal("C_rx_grconvert", 4, coords::convert),
        internal("C_rx_strmetric", 5, coords::strmetric),
        internal("C_rx_ax_ticks", 5, coords::ax_ticks),
        internal("C_rx_draw", 3, lowlevel::draw),
        internal("C_rx_hist_counts", 5, histogram::counts),
        internal("C_rx_formula_frame", 4, formula_frame),
        internal("C_rx_dev_control", 1, replay::control),
        internal("C_rx_is_recording", 0, replay::is_recording),
        internal("C_rx_record_plot", 2, replay::snapshot),
        internal("C_rx_replay_plot", 1, replay::replay),
    ],
    prelude: concat!(
        include_str!("prelude.R"),
        include_str!("axes.R"),
        include_str!("distributions.R"),
        include_str!("multivariate.R"),
    ),
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
    if !active(ctx, call, false)?.recording {
        return Ok(());
    }
    let entry = recorded_call(ctx, name, args)?;
    active(ctx, call, false)?.display_list.push(entry);
    Ok(())
}

fn recorded_call(ctx: &mut Ctx, name: &str, args: &[SexpId]) -> EvalResult {
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
        Ok(ctx.session_mut().lcons(internal, tail, SEXP_NIL)?)
    })
}

fn formula_frame(ctx: &mut Ctx, _: &Call, args: &mut roxide_eval::Args) -> EvalResult {
    let values = args.get(0);
    let labels = names(ctx, values)?;
    ctx.scoped(|ctx| {
        let mut tail = SEXP_NIL;
        if !labels
            .iter()
            .any(|name| name.as_deref() == Some("na.action"))
        {
            let key = ctx.session_mut().intern("na.action")?.as_sexp();
            tail = ctx.session_mut().cons(args.get(3), tail, key)?;
            ctx.protect(tail);
        }
        for (i, label) in labels.iter().enumerate().rev() {
            if let Some(name @ ("formula" | "data" | "subset" | "na.action")) = label.as_deref() {
                let value = ctx.session().list_get(values, i)?;
                let key = ctx.session_mut().intern(name)?.as_sexp();
                tail = ctx.session_mut().cons(value, tail, key)?;
                ctx.protect(tail);
            }
        }
        let expr = ctx.session_mut().lcons(args.get(2), tail, SEXP_NIL)?;
        ctx.protect(expr);
        ctx.eval(expr, args.get(1))
    })
}
