use roxide_gc::SexpId;
use roxide_value::{SEXP_NIL, SexpType, ValueApi};

use crate::{Args, Call, Ctx, EvalResult};

impl Ctx {
    pub(super) fn simple_condition(
        &mut self,
        kind: &str,
        message: SexpId,
        call: SexpId,
    ) -> EvalResult {
        let class = match kind {
            "error" => "simpleError",
            "warning" => "simpleWarning",
            _ => "simpleMessage",
        };
        self.make_condition(message, call, [class, kind, "condition"])
    }

    pub(super) fn make_condition<'a>(
        &mut self,
        message: SexpId,
        call: SexpId,
        classes: impl IntoIterator<Item = &'a str>,
    ) -> EvalResult {
        let classes = self.strings(classes)?;
        let message_tag = self.session.intern("message")?;
        let call_tag = self.session.intern("call")?;
        let cond = self.list(&Args::from_pairs(vec![
            (Some(message_tag), message),
            (Some(call_tag), call),
        ]))?;
        let tag = self.session.intern("class")?;
        self.session.set_attrib(cond, tag, classes)?;
        Ok(cond)
    }
}

pub(super) fn simple_condition(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(args, &["message", "call"], call.err_call())?;
    let name = ctx.prims[usize::from(call.prim.0)].name;
    if name == "simpleCondition" {
        let message = ctx.matched_arg(args, &matched, 0).unwrap_or(SEXP_NIL);
        let condition_call = ctx.matched_arg(args, &matched, 1).unwrap_or(SEXP_NIL);
        return ctx.make_condition(message, condition_call, ["simpleCondition", "condition"]);
    }
    let kind = match name {
        "simpleError" => "error",
        "simpleWarning" => "warning",
        _ => "message",
    };
    let msg = ctx.matched_arg(args, &matched, 0).unwrap_or(SEXP_NIL);
    let msg = if kind == "message" {
        msg
    } else {
        ctx.session.coerce(msg, SexpType::Str)?.0
    };
    ctx.simple_condition(
        kind,
        msg,
        ctx.matched_arg(args, &matched, 1).unwrap_or(SEXP_NIL),
    )
}

pub(super) fn condition_field(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let name = ctx.prims[usize::from(call.prim.0)].name;
    let field = if name == "conditionMessage" {
        "message"
    } else {
        "call"
    };
    let args = ctx.list_args(args.get(0))?;
    Ok(args.named(&ctx.session, field).unwrap_or(SEXP_NIL))
}

pub(super) fn custom_condition(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(args, &["message", "...", "class", "call"], call.err_call())?;
    let message = ctx.matched_arg(args, &matched, 0).unwrap_or(SEXP_NIL);
    let msg = ctx.session.coerce(message, SexpType::Str)?.0;
    let condition_call = ctx.matched_arg(args, &matched, 3).unwrap_or(SEXP_NIL);
    let kind = if ctx.prims[usize::from(call.prim.0)].name == "errorCondition" {
        "error"
    } else {
        "warning"
    };
    let mut classes = match ctx.matched_arg(args, &matched, 2) {
        Some(value) if value != SEXP_NIL => ctx.session.str_slice(value)?.to_vec(),
        _ => Vec::new(),
    };
    for class in [kind, "condition"] {
        classes.push(
            ctx.session
                .mk_char(class.as_bytes(), roxide_value::Encoding::Utf8)?,
        );
    }
    let message = ctx.session.intern("message")?;
    let call_tag = ctx.session.intern("call")?;
    let mut fields = vec![(Some(message), msg), (Some(call_tag), condition_call)];
    for i in matched.dots {
        let tag = args.name(i);
        if tag == Some(message) || tag == Some(call_tag) {
            return Err(ctx.stop(call.err_call(), "duplicate condition fields"));
        }
        fields.push((tag, args.get(i)));
    }
    let cond = ctx.list(&Args::from_pairs(fields))?;
    let classes = ctx.session.alloc_str_from(&classes)?;
    let tag = ctx.session.intern("class")?;
    ctx.session.set_attrib(cond, tag, classes)?;
    Ok(cond)
}
