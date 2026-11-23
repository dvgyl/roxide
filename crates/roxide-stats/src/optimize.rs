//! Optimization, root finding, and numerical integration in the stats namespace.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod multidim;
mod scalar;

use roxide_eval::{Args, Call, Ctx, EvalResult, Module};
use roxide_gc::SexpId;
use roxide_value::{SEXP_NIL, ValueApi};

use crate::distn::support::{internal, set_attr};

/// Registration table for the optimization and integration functions.
pub const MODULE: Module = Module {
    prims: &[
        internal("C_optim", 8, multidim::optim),
        internal("C_optimhess", 4, multidim::hessian),
        internal("C_fmin", 5, scalar::minimize),
        internal("C_zeroin2", 10, scalar::root),
        internal("C_dqags", 8, scalar::finite_integral),
        internal("C_dqagi", 8, scalar::infinite_integral),
        internal("C_print_integrate", 2, scalar::print),
        internal("C_optim_match_arg", 2, match_arg),
    ],
    prelude: include_str!("optimize/prelude.R"),
};

fn strings(ctx: &mut Ctx, names: &[&str]) -> EvalResult {
    use roxide_value::Encoding;

    let chars = names
        .iter()
        .map(|name| ctx.session_mut().mk_char(name.as_bytes(), Encoding::Native))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ctx.session_mut().alloc_str_from(&chars)?)
}

fn named_list(ctx: &mut Ctx, names: &[&str], values: &[SexpId]) -> EvalResult {
    ctx.scoped(|ctx| {
        for &id in values {
            ctx.protect(id);
        }
        let list = ctx.session_mut().alloc_list(values.len())?;
        ctx.protect(list);
        for (i, &value) in values.iter().enumerate() {
            ctx.session_mut().list_set(list, i, value)?;
        }
        let names = strings(ctx, names)?;
        ctx.protect(names);
        set_attr(ctx, list, "names", names)?;
        Ok(list)
    })
}

fn match_arg(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let arg = args.get(0);
    let options = if arg == SEXP_NIL {
        &[][..]
    } else if let Ok(options) = ctx.session().str_slice(arg) {
        options
    } else {
        return Err(ctx.stop(call.err_call(), "'arg' must be NULL or a character vector"));
    };
    let choices = ctx.session().str_slice(args.get(1))?;
    if (arg == SEXP_NIL || options == choices)
        && let Some(&first) = choices.first()
    {
        return Ok(ctx.session_mut().alloc_str_from(&[first])?);
    }
    if options.len() != 1 {
        return Err(ctx.stop(call.err_call(), "'arg' must be of length 1"));
    }
    let prefix = ctx.session().char_bytes(options[0])?;
    let mut matches = choices.iter().filter(|&&id| {
        !prefix.is_empty()
            && ctx
                .session()
                .char_bytes(id)
                .is_ok_and(|choice| choice.starts_with(prefix))
    });
    let selected = choices
        .iter()
        .find(|&&id| ctx.session().char_bytes(id).ok() == Some(prefix))
        .or_else(|| matches.next().filter(|_| matches.next().is_none()));
    if let Some(&id) = selected {
        return Ok(ctx.session_mut().alloc_str_from(&[id])?);
    }
    let names: Vec<_> = choices
        .iter()
        .filter_map(|&id| ctx.session().char_bytes(id).ok())
        .map(|s| format!("“{}”", String::from_utf8_lossy(s)))
        .collect();
    Err(ctx.stop(
        call.err_call(),
        format!("'arg' should be one of {}", names.join(", ")),
    ))
}

fn callback(ctx: &mut Ctx, call: &Call, fun: SexpId, x: &[f64], names: SexpId) -> EvalResult {
    let arg = ctx.session_mut().alloc_real_from(x)?;
    ctx.protect(arg);
    if names != SEXP_NIL {
        set_attr(ctx, arg, "names", names)?;
    }
    let result = ctx.apply(fun, &[(None, arg)], call.err_call(), call.env)?;
    ctx.protect(result);
    Ok(result)
}
