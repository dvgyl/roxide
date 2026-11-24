use roxide_gc::SexpId;
use roxide_value::{SEXP_MISSING, SEXP_NIL, SexpType, ValueApi};

use crate::{Args, Call, Ctx, EvalResult, FrameId, Transfer, Unwind};

use super::Restart;

impl Ctx {
    pub(super) fn push_restart(
        &mut self,
        target: FrameId,
        name: String,
        fun: SexpId,
    ) -> EvalResult<()> {
        let value = self.session.scalar_str(&name)?;
        let args = if name == "abort" {
            vec![(None, value), (None, SEXP_NIL)]
        } else {
            let tags = [
                "name",
                "exit",
                "handler",
                "description",
                "test",
                "interactive",
            ];
            let description = self.session.scalar_str("")?;
            tags.into_iter()
                .zip([value, SEXP_NIL, fun, description, SEXP_NIL, SEXP_NIL])
                .map(|(tag, value)| Ok((Some(self.session.intern(tag)?), value)))
                .collect::<EvalResult<Vec<_>>>()?
        };
        let restart = self.list(&Args::from_pairs(args))?;
        let class = self.session.scalar_str("restart")?;
        let tag = self.session.intern("class")?;
        self.session.set_attrib(restart, tag, class)?;
        self.conditions.restarts.push(Restart {
            target,
            name,
            value: restart,
            fun,
        });
        Ok(())
    }
}

pub(super) fn with_restarts(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let args = &ctx.promise_args(args, call.env, call.err_call())?;
    for &value in args.values() {
        ctx.protect(value);
    }
    let matched = ctx.match_names(args, &["expr", "..."], call.err_call())?;
    let id = ctx.next_frame();
    let base = ctx.conditions.restarts.len();
    let mut restarts = Vec::new();
    for i in matched.dots.iter().copied() {
        let Some(tag) = args.name(i) else {
            return Err(ctx.stop(call.err_call(), "restarts must be named"));
        };
        let name = ctx.session.sym_name(tag)?.to_owned();
        let fun = ctx.eval(args.get(i), call.env)?;
        ctx.protect(fun);
        restarts.push((name, fun));
    }
    let result = (|| {
        for (name, fun) in restarts.into_iter().rev() {
            ctx.push_restart(id, name, fun)?;
        }
        ctx.eval_arg(args, &matched, 0, call, SEXP_MISSING)
    })();
    let fun = match result {
        Err(Unwind::Transfer {
            target,
            kind: Transfer::Restart { restart, .. },
        }) if target == id => ctx.conditions.restarts[base..]
            .iter()
            .find(|entry| entry.value == restart)
            .map(|entry| entry.fun),
        _ => None,
    };
    ctx.conditions.restarts.truncate(base);
    if let (
        Some(fun),
        Err(Unwind::Transfer {
            kind: Transfer::Restart { args, .. },
            ..
        }),
    ) = (fun, &result)
    {
        let pairs: Vec<_> = ctx.session.pairlist_iter(*args).collect();
        return ctx.apply(fun, &pairs, SEXP_NIL, call.env);
    }
    result
}

pub(super) fn find_restart(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let name = ctx.string(args.get(0), call.err_call())?;
    Ok(ctx
        .conditions
        .restarts
        .iter()
        .rfind(|entry| entry.name == name)
        .map_or(SEXP_NIL, |entry| entry.value))
}

pub(super) fn invoke_restart(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    if args.is_empty() {
        return Err(ctx.stop(
            call.err_call(),
            "argument \"r\" is missing, with no default",
        ));
    }
    let r = args.get(0);
    let found = if ctx.session.type_of(r)? == SexpType::Str {
        let name = ctx.string(r, call.err_call())?;
        ctx.conditions
            .restarts
            .iter()
            .rfind(|entry| entry.name == name)
    } else {
        ctx.conditions
            .restarts
            .iter()
            .rfind(|entry| entry.value == r)
    };
    let Some(entry) = found else {
        if ctx.prims[usize::from(call.prim.0)].name == "tryInvokeRestart" {
            ctx.visible = false;
            return Ok(SEXP_NIL);
        }
        return Err(ctx.stop(call.err_call(), "no such restart"));
    };
    let (target, restart) = (entry.target, entry.value);
    let args = ctx.pairlist(&Args::from_pairs(args.iter().skip(1).collect()), false)?;
    Err(Unwind::Transfer {
        target,
        kind: Transfer::Restart { restart, args },
    })
}

pub(super) fn compute_restarts(ctx: &mut Ctx, _: &Call, _: &mut Args) -> EvalResult {
    let mut pairs: Vec<_> = ctx
        .conditions
        .restarts
        .iter()
        .rev()
        .map(|entry| (None, entry.value))
        .collect();
    let id = ctx.next_frame();
    ctx.push_restart(id, "abort".to_owned(), SEXP_NIL)?;
    if let Some(abort) = ctx.conditions.restarts.pop() {
        pairs.push((None, abort.value));
    }
    ctx.list(&Args::from_pairs(pairs))
}
