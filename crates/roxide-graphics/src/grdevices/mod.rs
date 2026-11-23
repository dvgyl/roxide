//! Color conversion, palettes, device control, and plot range functions.

mod colors;
mod dev;
mod palette;
mod ranges;

pub use colors::parse_rgba;

use roxide_eval::{
    ArgEval, Arity, Call, Ctx, Dispatch, EvalResult, Module, PrimDef, PrimFn, PrimKind, Visibility,
};
use roxide_gc::SexpId;
use roxide_value::{CharId, Encoding, NA_REAL, SEXP_NIL, SexpType, ValueApi};

/// Evaluator bindings for the graphics device and color functions.
pub const MODULE: Module = Module {
    prims: &[
        internal("C_gd_colors", 1, colors::named_colors),
        internal("C_gd_col2rgb", 2, colors::col2rgb),
        internal("C_gd_rgb", 6, colors::rgb),
        internal("C_gd_hsv", 4, colors::hsv),
        internal("C_gd_hcl", 5, colors::hcl),
        internal("C_gd_gray", 2, colors::gray),
        internal("C_gd_lab", 2, colors::lab),
        internal("C_gd_series", 3, palette::series),
        internal("C_gd_palette", 1, palette::palette),
        internal("C_gd_palette_names", 1, palette::names),
        internal("C_gd_palette_colors", 4, palette::discrete),
        internal("C_gd_hcl_colors", 5, palette::continuous),
        internal("C_gd_dev_cur", 0, dev::current),
        internal("C_gd_dev_list", 0, dev::list),
        internal("C_gd_dev_step", 2, dev::step),
        internal("C_gd_dev_set", 1, dev::set),
        internal("C_gd_dev_off", 1, dev::off),
        internal("C_gd_dev_size", 1, dev::size),
        internal("C_gd_graphics_off", 0, dev::close_all),
        internal("C_gd_pretty", 9, ranges::pretty),
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

fn limit<T>(ctx: &mut Ctx, call: &Call, len: usize) -> EvalResult<()> {
    if len > ctx.host().limits().max_vector_bytes / std::mem::size_of::<T>() {
        return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
    }
    Ok(())
}

fn numbers(ctx: &mut Ctx, call: &Call, x: SexpId) -> EvalResult<Vec<f64>> {
    limit::<f64>(ctx, call, ctx.session().xlength(x)?)?;
    let (value, _) = ctx.session_mut().coerce(x, SexpType::Real)?;
    Ok(ctx.session().real(value)?.iter().collect())
}

fn number(ctx: &mut Ctx, x: SexpId) -> EvalResult<f64> {
    let (value, _) = ctx.session_mut().coerce(x, SexpType::Real)?;
    Ok(ctx.session().real(value)?.iter().next().unwrap_or(NA_REAL))
}

fn flag(ctx: &mut Ctx, call: &Call, x: SexpId, message: &str) -> EvalResult<bool> {
    let (value, _) = ctx.session_mut().coerce(x, SexpType::Lgl)?;
    let value = ctx
        .session()
        .lgl(value)?
        .iter()
        .next()
        .and_then(|v| v.get());
    value.ok_or_else(|| ctx.stop(call.err_call(), message))
}

fn strings<'a>(ctx: &mut Ctx, values: impl IntoIterator<Item = Option<&'a str>>) -> EvalResult {
    ctx.scoped(|ctx| {
        let mut ids = Vec::new();
        for value in values {
            let id = if let Some(text) = value {
                ctx.session_mut().mk_char(text.as_bytes(), Encoding::Utf8)?
            } else {
                CharId::NA
            };
            ctx.protect(id.as_sexp());
            ids.push(id);
        }
        Ok(ctx.session_mut().alloc_str_from(&ids)?)
    })
}

fn attr(ctx: &mut Ctx, x: SexpId, name: &str) -> EvalResult {
    let key = ctx.session_mut().intern(name)?;
    Ok(ctx.session().attr(x, key))
}

fn set_attr(ctx: &mut Ctx, x: SexpId, name: &str, value: SexpId) -> EvalResult<()> {
    let key = ctx.session_mut().intern(name)?;
    Ok(ctx.session_mut().set_attrib(x, key, value)?)
}

fn matrix_attributes(
    ctx: &mut Ctx,
    call: &Call,
    result: SexpId,
    rows: usize,
    cols: usize,
    dimnames: Option<[SexpId; 2]>,
) -> EvalResult {
    ctx.scoped(|ctx| {
        ctx.protect(result);
        if let Some([row_names, col_names]) = dimnames {
            ctx.protect(row_names);
            ctx.protect(col_names);
        }
        let (Ok(rows), Ok(cols)) = (i32::try_from(rows), i32::try_from(cols)) else {
            return Err(ctx.stop(call.err_call(), "matrix size limit exceeded"));
        };
        let dim = ctx.session_mut().alloc_int_from(&[rows, cols])?;
        ctx.protect(dim);
        set_attr(ctx, result, "dim", dim)?;
        if let Some([row_names, col_names]) = dimnames {
            let names = ctx.session_mut().alloc_list(2)?;
            ctx.protect(names);
            ctx.session_mut().list_set(names, 0, row_names)?;
            ctx.session_mut().list_set(names, 1, col_names)?;
            set_attr(ctx, result, "dimnames", names)?;
        }
        Ok(result)
    })
}

fn optional_numbers(ctx: &mut Ctx, call: &Call, x: SexpId) -> EvalResult<Vec<f64>> {
    if x == SEXP_NIL {
        return Ok(Vec::new());
    }
    numbers(ctx, call, x)
}

fn recycled(values: &[f64], index: usize, default: f64) -> f64 {
    if values.is_empty() {
        default
    } else {
        values[index % values.len()]
    }
}
