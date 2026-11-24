use roxide_gc::SexpId;
use roxide_value::{RSession, SEXP_MISSING, SEXP_NIL, SEXP_UNBOUND, SexpType, Sym, ValueApi};

use crate::eval::ArgInput;
use crate::{Args, Call, Ctx, EvalResult};

use super::exec::{Execution, invalid};
use super::{guard, scalar};

pub(super) fn get(
    ctx: &mut Ctx,
    vm: &Execution<'_>,
    call: Call,
    receiver: SexpId,
    expr: SexpId,
) -> EvalResult {
    if ctx.session.is_object(receiver)? || ctx.session.is_s4(receiver)? {
        let args = arguments(ctx, call.expr)?;
        return ctx.run_primitive(
            call,
            args,
            ArgInput::AssignmentLayer {
                target: receiver,
                replacement: None,
            },
            true,
        );
    }

    // The AST evaluator retains the receiver while it evaluates the index.
    ctx.session.heap.inc_ref(receiver);
    let index = ctx.eval(expr, call.env);
    ctx.session.heap.dec_ref(receiver);
    let index = index?;
    vm.temp(ctx, index)?;

    if scalar::plain(&ctx.session, receiver)
        && matches!(
            ctx.session.type_of(receiver)?,
            SexpType::Int
                | SexpType::Lgl
                | SexpType::Real
                | SexpType::Str
                | SexpType::Vec
                | SexpType::Expr
                | SexpType::Raw
                | SexpType::Cplx
        )
        && let Some(i) = position(&ctx.session, receiver, index)
    {
        let value = ctx.element(receiver, i)?;
        ctx.visible = true;
        if ctx.prims[usize::from(call.prim.0)].name == "[" {
            return Ok(match ctx.session.type_of(receiver)? {
                SexpType::Vec => {
                    let out = ctx.session.alloc_list(1)?;
                    ctx.session.list_set(out, 0, value)?;
                    out
                }
                SexpType::Expr => ctx.session.alloc_expr_from(&[value])?,
                _ => value,
            });
        }
        return Ok(value);
    }

    ctx.run_primitive(
        call,
        Args::from_pairs(vec![(None, receiver), (None, index)]),
        ArgInput::Values,
        false,
    )
}

pub(super) fn set(
    ctx: &mut Ctx,
    call: Call,
    sym: Sym,
    index_expr: SexpId,
    value: SexpId,
    replacement: Option<SexpId>,
) -> EvalResult {
    if replacement.is_some()
        && let Some(binding) = ctx.session.env_get(call.env, sym)
        && !binding.active
        && !binding.locked
        && binding.value != value
        && ctx.session.heap.refcnt(binding.value) == 1
        && scalar::plain(&ctx.session, binding.value)
        && !ctx.session.is_altrep(binding.value)?
        && let Some(index) = immediate_index(&ctx.session, index_expr, call.env)?
        && let Some(i) = position(&ctx.session, binding.value, index)
        && update(&mut ctx.session, binding.value, i, value)?
    {
        ctx.visible = false;
        return Ok(value);
    }

    let args = arguments(ctx, call.expr)?;
    let rhs = ctx.value_promise(args.get(1), value)?;
    ctx.run_primitive(
        call,
        Args::from_pairs(vec![(None, args.get(0)), (None, rhs)]),
        ArgInput::Expressions,
        true,
    )
}

fn position(session: &RSession, target: SexpId, index: SexpId) -> Option<usize> {
    if !matches!(session.type_of(index), Ok(SexpType::Int | SexpType::Real)) {
        return None;
    }
    let n = scalar::number(session, index)?.real().trunc();
    let len = session.xlength(target).ok()?;
    (n.is_finite() && n >= 1.0 && n <= len as f64 && n < usize::MAX as f64).then(|| n as usize - 1)
}

fn immediate_index(session: &RSession, expr: SexpId, env: SexpId) -> EvalResult<Option<SexpId>> {
    Ok(match session.type_of(expr)? {
        SexpType::Int | SexpType::Real => Some(expr),
        SexpType::Sym
            if expr != SEXP_MISSING
                && crate::closure::dots_index(session.sym_name_of(expr)?).is_none() =>
        {
            let sym = session.sexp_as_sym(expr)?;
            guard::variable(session, sym, env, &mut None)?
                .map(|(_, binding)| binding)
                .filter(|binding| !binding.active && binding.value != SEXP_UNBOUND)
                .map(|binding| binding.value)
        }
        _ => None,
    })
}

fn update(session: &mut RSession, target: SexpId, i: usize, value: SexpId) -> EvalResult<bool> {
    if !scalar::plain(session, value)
        || session.xlength(value)? != 1
        || session.type_of(target)? != session.type_of(value)?
    {
        return Ok(false);
    }
    match session.type_of(target)? {
        SexpType::Int => {
            let val = session.int(value)?.get(0);
            session.int_mut(target)?[i] = val;
        }
        SexpType::Real => {
            let val = session.real(value)?.get(0);
            session.real_mut(target)?[i] = val;
        }
        SexpType::Lgl => {
            let val = session.lgl(value)?.get(0);
            session.lgl_mut(target)?[i] = val;
        }
        SexpType::Raw => {
            let val = session.raw(value)?.get(0);
            session.raw_dataptr_mut(target)?[i] = val;
        }
        SexpType::Cplx => {
            let val = session.cplx(value)?.get(0);
            session.cplx_dataptr_mut(target)?[i] = val;
        }
        SexpType::Str => {
            let val = session.str_slice(value)?[0];
            session.str_set(target, i, val)?;
        }
        _ => return Ok(false),
    }
    Ok(true)
}

pub(super) fn arguments(ctx: &Ctx, expr: SexpId) -> EvalResult<Args> {
    let (_, tail, _) = ctx.session.cons_parts(expr)?;
    if tail == SEXP_NIL {
        return Err(invalid());
    }
    Ok(Args::from_pairs(ctx.session.pairlist_iter(tail).collect()))
}
