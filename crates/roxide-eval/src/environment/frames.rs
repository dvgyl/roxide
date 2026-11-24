use roxide_gc::SexpId;
use roxide_value::{GLOBAL_ENV, SEXP_NIL, ValueApi};

use crate::{Args, Call, Ctx, EvalResult};

impl Ctx {
    /// Resolve a numeric environment argument against the call stack.
    pub(crate) fn eval_env_at(&mut self, position: SexpId, call: &Call) -> EvalResult {
        let current = self
            .frames
            .iter()
            .rposition(|frame| frame.cloenv == call.env)
            .map_or(0, |i| i + 1) as i64;
        let index = i64::from(self.integer(position, call.err_call())?);
        let absolute = if index < 0 { current + index } else { index };
        if absolute == 0 {
            return Ok(GLOBAL_ENV);
        }
        if absolute < 0 || absolute > current {
            return Err(self.stop(call.err_call(), "not that many frames on the stack"));
        }
        Ok(self.frames[absolute as usize - 1].cloenv)
    }
}

pub(super) fn sys_stack(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let current = ctx
        .frames
        .iter()
        .rposition(|frame| frame.cloenv == call.env)
        .map_or(0, |index| index + 1);
    let parent = |frames: &[crate::ctx::Frame], child: usize| {
        let Some(frame) = child.checked_sub(1).and_then(|i| frames.get(i)) else {
            return 0;
        };
        frames[..child - 1]
            .iter()
            .rposition(|entry| entry.cloenv == frame.caller)
            .map_or(0, |index| index + 1)
    };
    match ctx.prims[usize::from(call.prim.0)].name {
        "sys.parent" => {
            let n = if args.is_empty() {
                1
            } else {
                ctx.integer(args.get(0), call.err_call())?
            };
            if n < 0 {
                return Err(ctx.stop(call.err_call(), "invalid 'n' value"));
            }
            let mut index = current;
            for _ in 0..n {
                index = parent(&ctx.frames, index);
                if index == 0 {
                    break;
                }
            }
            Ok(ctx.session.scalar_int(index as i32)?)
        }
        "sys.parents" => {
            let parents: Vec<_> = (1..=current)
                .map(|i| parent(&ctx.frames, i) as i32)
                .collect();
            Ok(ctx.session.alloc_int_from(&parents)?)
        }
        "sys.on.exit" => {
            let Some(frame) = ctx.frame(call.env) else {
                return Ok(SEXP_NIL);
            };
            match frame.on_exit.as_slice() {
                [] => Ok(SEXP_NIL),
                [expr] => Ok(*expr),
                exprs => {
                    let pairs = exprs.iter().map(|&expr| (None, expr)).collect();
                    let head = ctx.session.intern("{")?.as_sexp();
                    ctx.make_call(head, &Args::from_pairs(pairs))
                }
            }
        }
        kind => {
            let pairs = ctx.frames[..current]
                .iter()
                .map(|frame| {
                    (
                        None,
                        if kind == "sys.calls" {
                            frame.call
                        } else {
                            frame.cloenv
                        },
                    )
                })
                .collect();
            ctx.pairlist(&Args::from_pairs(pairs), false)
        }
    }
}

pub(super) fn parent_frame(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let n = if args.is_empty() {
        1
    } else {
        ctx.integer(args.get(0), call.err_call())?
    };
    if n < 1 {
        return Err(ctx.stop(call.err_call(), "invalid 'n' value"));
    }
    let mut env = call.env;
    let mut end = ctx.frames.len();
    for _ in 0..n {
        let Some(index) = ctx.frames[..end]
            .iter()
            .rposition(|frame| frame.cloenv == env)
        else {
            return Ok(GLOBAL_ENV);
        };
        env = ctx.frames[index].caller;
        if env == GLOBAL_ENV {
            break;
        }
        end = index;
    }
    Ok(env)
}

pub(super) fn sys_nframe(ctx: &mut Ctx, call: &Call, _: &mut Args) -> EvalResult {
    let depth = ctx
        .frames
        .iter()
        .rposition(|frame| frame.cloenv == call.env)
        .map_or(0, |i| i + 1);
    Ok(ctx.session.scalar_int(depth as i32)?)
}

pub(super) fn sys_frame(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let current = ctx
        .frames
        .iter()
        .rposition(|frame| frame.cloenv == call.env)
        .map_or(0, |i| i + 1) as i64;
    let which = if args.is_empty() {
        0
    } else {
        i64::from(ctx.integer(args.get(0), call.err_call())?)
    };
    let name = ctx.prims[usize::from(call.prim.0)].name;
    let index = if which == 0 && name == "sys.frame" {
        0
    } else if which <= 0 {
        current + which
    } else {
        which
    };
    if index == 0 {
        return Ok(if name == "sys.frame" {
            GLOBAL_ENV
        } else {
            SEXP_NIL
        });
    }
    if index < 0 || index > current {
        return Err(ctx.stop(call.err_call(), "not that many frames on the stack"));
    }
    let frame = &ctx.frames[index as usize - 1];
    Ok(match name {
        "sys.call" => frame.call,
        "sys.frame" => frame.cloenv,
        _ => frame.fun,
    })
}
