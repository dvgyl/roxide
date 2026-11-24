use roxide_gc::SexpId;
use roxide_value::{SEXP_NIL, SexpType, ValueApi};

use crate::control::logical;
use crate::{Args, Call, Ctx, EvalResult};

impl Ctx {
    /// Look up an R session option (`getOption(name)`), returning built-in defaults if unset.
    pub fn option_value(&mut self, name: &str) -> EvalResult<Option<SexpId>> {
        let tag = self.session.intern(name)?;
        if let Some(&(_, value)) = self.options.iter().find(|&&(key, _)| key == tag) {
            return Ok(Some(value));
        }
        Ok(Some(match name {
            "digits" => self.session.scalar_int(self.opts.digits as i32)?,
            "scipen" => self.session.scalar_int(self.opts.scipen)?,
            "warn" => self.session.scalar_int(self.opts.warn)?,
            "warning.length" => self.session.scalar_int(1000)?,
            "expressions" => self.session.scalar_int(self.opts.expressions as i32)?,
            "width" => self.session.scalar_int(self.opts.width as i32)?,
            "warnPartialMatchArgs" => self
                .session
                .scalar_lgl(logical(self.opts.warn_partial_match_args))?,
            "warnPartialMatchDollar" => self
                .session
                .scalar_lgl(logical(self.opts.warn_partial_match_dollar))?,
            "keep.source" => self.session.scalar_lgl(logical(self.interactive))?,
            _ => return Ok(None),
        }))
    }

    fn set_option(&mut self, name: &str, value: SexpId, call: &Call) -> EvalResult {
        let old = self.option_value(name)?.unwrap_or(SEXP_NIL);
        self.protect(old);
        let value = if name == "error" {
            match self.session.type_of(value)? {
                SexpType::Clo | SexpType::Builtin | SexpType::Special => {
                    self.make_call(value, &Args::from_pairs(Vec::new()))?
                }
                SexpType::Nil | SexpType::Lang | SexpType::Expr => value,
                _ => return Err(self.stop(call.err_call(), "invalid value for 'error'")),
            }
        } else {
            value
        };
        match name {
            "digits" => self.opts.digits = self.option_int(name, value, 1, 22, call)? as usize,
            "scipen" => self.opts.scipen = self.option_int(name, value, -999, 999, call)?,
            "warn" => self.opts.warn = self.integer(value, call.err_call())?,
            "expressions" => {
                self.opts.expressions = self.option_int(name, value, 25, 500_000, call)? as usize
            }
            "width" => self.opts.width = self.option_int(name, value, 10, 10_000, call)? as usize,
            "warnPartialMatchArgs" => {
                self.opts.warn_partial_match_args =
                    value != SEXP_NIL && self.truth(value, call.err_call())?
            }
            "warnPartialMatchDollar" => {
                self.opts.warn_partial_match_dollar =
                    value != SEXP_NIL && self.truth(value, call.err_call())?
            }
            _ => {}
        }
        let tag = self.session.intern(name)?;
        if let Some(i) = self.options.iter().position(|&(key, _)| key == tag) {
            if value == SEXP_NIL {
                self.options.remove(i);
            } else {
                self.options[i].1 = value;
            }
        } else if value != SEXP_NIL {
            self.options.push((tag, value));
        }
        Ok(old)
    }

    fn option_int(
        &mut self,
        name: &str,
        value: SexpId,
        lo: i32,
        hi: i32,
        call: &Call,
    ) -> EvalResult<i32> {
        let value = self.integer(value, call.err_call())?;
        if !(lo..=hi).contains(&value) {
            return Err(self.stop(
                call.err_call(),
                format!("invalid '{name}' parameter, allowed {lo}...{hi}"),
            ));
        }
        Ok(value)
    }
}

pub(super) fn options(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    if args.is_empty() {
        let mut tags: Vec<_> = ctx.options.iter().map(|&(tag, _)| tag).collect();
        for name in [
            "digits",
            "scipen",
            "warn",
            "expressions",
            "width",
            "keep.source",
            "warnPartialMatchArgs",
            "warnPartialMatchDollar",
        ] {
            let tag = ctx.session.intern(name)?;
            if !tags.contains(&tag) {
                tags.push(tag);
            }
        }
        tags.sort_by_key(|tag| ctx.session.sym_name(*tag).unwrap_or_default());
        let mut values = Vec::new();
        for tag in tags {
            let name = ctx.session.sym_name(tag)?.to_owned();
            values.push((Some(tag), ctx.option_value(&name)?.unwrap_or(SEXP_NIL)));
        }
        return ctx.list(&Args::from_pairs(values));
    }
    let mut visible = true;
    let mut out = Vec::new();
    for (tag, value) in args.iter() {
        if let Some(tag) = tag {
            let name = ctx.session.sym_name(tag)?.to_owned();
            out.push((Some(tag), ctx.set_option(&name, value, call)?));
            visible = false;
        } else if ctx.session.type_of(value)? == SexpType::Vec {
            for (tag, value) in ctx.list_args(value)?.iter() {
                let Some(tag) = tag else {
                    return Err(ctx.stop(call.err_call(), "invalid argument"));
                };
                let name = ctx.session.sym_name(tag)?.to_owned();
                out.push((Some(tag), ctx.set_option(&name, value, call)?));
            }
            visible = false;
        } else {
            let name = ctx.string(value, call.err_call())?;
            let tag = ctx.session.intern(&name)?;
            out.push((Some(tag), ctx.option_value(&name)?.unwrap_or(SEXP_NIL)));
        }
        if let Some((_, value)) = out.last() {
            ctx.protect(*value);
        }
    }
    ctx.visible = visible;
    ctx.list(&Args::from_pairs(out))
}

pub(super) fn get_option(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let matched = ctx.match_names(args, &["x", "default"], call.err_call())?;
    let x = ctx.matched_arg(args, &matched, 0).unwrap_or(SEXP_NIL);
    let name = ctx.string(x, call.err_call())?;
    Ok(ctx
        .option_value(&name)?
        .unwrap_or_else(|| ctx.matched_arg(args, &matched, 1).unwrap_or(SEXP_NIL)))
}
