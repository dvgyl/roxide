use roxide_gc::SexpId;
use roxide_value::{
    BASE_NAMESPACE, RLogical, SEXP_MISSING, SEXP_NIL, SEXP_S4_NULL, SexpType, Sym, ValueApi,
};

use crate::{Args, Call, Ctx, EvalResult, S4Hooks};

fn slot_name(ctx: &mut Ctx, raw: SexpId, call: SexpId) -> EvalResult<String> {
    match ctx.session.type_of(raw)? {
        SexpType::Sym if raw != SEXP_MISSING => Ok(ctx.session.sym_name_of(raw)?.to_owned()),
        SexpType::Str => ctx.string(raw, call),
        _ => Err(ctx.stop(call, "invalid slot name")),
    }
}

fn first_class_name(ctx: &mut Ctx, obj: SexpId) -> EvalResult<String> {
    let classes = ctx.classes(obj, true)?;
    Ok(classes.into_iter().next().unwrap_or_else(|| {
        ctx.session
            .type_of(obj)
            .map_or("NULL", |t| t.type_name(false))
            .to_owned()
    }))
}

impl Ctx {
    /// Install the stateless S4 dispatch hook.
    pub fn set_s4_hooks(&mut self, hooks: &'static dyn S4Hooks) {
        self.s4_hooks = Some(hooks);
    }

    /// Print an S4 value through installed `S4Hooks::show`, returning `true` if handled.
    pub fn show_s4(&mut self, value: SexpId, env: SexpId) -> EvalResult<bool> {
        if let Some(hooks) = self.s4_hooks {
            hooks.show(self, value, env)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Query the full S4 superclass chain (`STRSXP`) for `value` via `S4Hooks::class_chain`.
    pub fn s4_class_chain(&mut self, value: SexpId) -> EvalResult<Option<SexpId>> {
        if let Some(hooks) = self.s4_hooks {
            hooks.class_chain(self, value)
        } else {
            Ok(None)
        }
    }

    /// Validate/coerce an S4 slot assignment via `S4Hooks::check_slot`.
    pub fn check_s4_slot(&mut self, obj: SexpId, name: Sym, value: SexpId) -> EvalResult<SexpId> {
        if let Some(hooks) = self.s4_hooks {
            hooks.check_slot(self, obj, name, value)
        } else {
            Ok(value)
        }
    }

    /// Find the object that supplies the data part of an S4 object.
    pub fn s4_data_source(&mut self, object: SexpId) -> EvalResult<Option<SexpId>> {
        if !self.session.is_s4(object)? {
            return Ok(None);
        }
        for name in [".Data", ".xData"] {
            let key = self.session.intern(name)?;
            let value = self.session.attr(object, key);
            if value != SEXP_NIL {
                return Ok(Some(if value == SEXP_S4_NULL {
                    SEXP_NIL
                } else {
                    value
                }));
            }
        }
        Ok((self.session.type_of(object)? != SexpType::Obj).then_some(object))
    }

    /// Read an S4 data part. Clear the S4 flag on the returned data.
    pub fn s4_data_part(&mut self, object: SexpId) -> EvalResult<Option<SexpId>> {
        let Some(source) = self.s4_data_source(object)? else {
            return Ok(None);
        };
        if source != object {
            return Ok(Some(source));
        }
        self.scoped(|ctx| {
            ctx.protect(object);
            let copy = ctx.session.shallow_duplicate(object)?;
            if copy != object {
                ctx.protect(copy);
                ctx.session.clear_attrs(copy)?;
                for key in [roxide_value::SYM_DIM, roxide_value::SYM_DIMNAMES] {
                    let value = ctx.session.attr(object, key);
                    if value != SEXP_NIL {
                        ctx.session.set_attrib(copy, key, value)?;
                    }
                }
            }
            ctx.session.set_s4_flag(copy, false)?;
            Ok(Some(copy))
        })
    }
}

pub(super) fn slot_get(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let obj = args.get(0);
    let name = slot_name(ctx, args.get(1), call.err_call())?;
    if !ctx.session.is_s4(obj)? {
        let classes = ctx.classes(obj, true)?;
        for cls in &classes {
            if let Some(fun) =
                ctx.lookup_method("@", &format!("@.{cls}"), call.env, BASE_NAMESPACE, false)?
            {
                let name_val = ctx.session.scalar_str(&name)?;
                return ctx.scoped(|ctx| {
                    ctx.protect(obj);
                    ctx.protect(name_val);
                    ctx.apply(
                        fun,
                        &[(None, obj), (None, name_val)],
                        call.err_call(),
                        call.env,
                    )
                });
            }
        }
        if let Some(fun) = ctx.lookup_method("@", "@.default", call.env, BASE_NAMESPACE, true)? {
            let name_val = ctx.session.scalar_str(&name)?;
            return ctx.scoped(|ctx| {
                ctx.protect(obj);
                ctx.protect(name_val);
                ctx.apply(
                    fun,
                    &[(None, obj), (None, name_val)],
                    call.err_call(),
                    call.env,
                )
            });
        }
        let cls = first_class_name(ctx, obj)?;
        return Err(ctx.stop(
            call.err_call(),
            format!("no applicable method for `@` applied to an object of class \"{cls}\""),
        ));
    }
    if name == ".Data" {
        return Ok(ctx.s4_data_part(obj)?.unwrap_or(SEXP_NIL));
    }
    let sym = ctx.session.intern(&name)?;
    let val = ctx.session.attr(obj, sym);
    if val == SEXP_NIL {
        let cls = first_class_name(ctx, obj)?;
        return Err(ctx.stop(
            call.err_call(),
            format!("no slot of name \"{name}\" for this object of class \"{cls}\""),
        ));
    }
    if val == SEXP_S4_NULL {
        return Ok(SEXP_NIL);
    }
    Ok(val)
}

pub(super) fn slot_set(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let obj = args.get(0);
    let name = slot_name(ctx, args.get(1), call.err_call())?;
    let val = args.get(2);
    if !ctx.session.is_s4(obj)? {
        let classes = ctx.classes(obj, true)?;
        for cls in &classes {
            if let Some(fun) = ctx.lookup_method(
                "@<-",
                &format!("@<-.{cls}"),
                call.env,
                BASE_NAMESPACE,
                false,
            )? {
                let name_val = ctx.session.scalar_str(&name)?;
                return ctx.scoped(|ctx| {
                    ctx.protect(obj);
                    ctx.protect(name_val);
                    ctx.protect(val);
                    ctx.apply(
                        fun,
                        &[(None, obj), (None, name_val), (None, val)],
                        call.err_call(),
                        call.env,
                    )
                });
            }
        }
        if let Some(fun) =
            ctx.lookup_method("@<-", "@<-.default", call.env, BASE_NAMESPACE, true)?
        {
            let name_val = ctx.session.scalar_str(&name)?;
            return ctx.scoped(|ctx| {
                ctx.protect(obj);
                ctx.protect(name_val);
                ctx.protect(val);
                ctx.apply(
                    fun,
                    &[(None, obj), (None, name_val), (None, val)],
                    call.err_call(),
                    call.env,
                )
            });
        }
        if ctx.s4_hooks.is_some() {
            let sym = ctx.session.intern(&name)?;
            let _ = ctx.check_s4_slot(obj, sym, val)?;
        }
        let cls = first_class_name(ctx, obj)?;
        return Err(ctx.stop(
            call.err_call(),
            format!("no applicable method for `@<-` applied to an object of class \"{cls}\""),
        ));
    }
    let sym = ctx.session.intern(&name)?;
    let checked = ctx.check_s4_slot(obj, sym, val)?;
    ctx.scoped(|ctx| {
        ctx.protect(obj);
        ctx.protect(checked);
        if name == ".Data" && ctx.session.type_of(obj)? != SexpType::Obj {
            let dim_sym = ctx.session.intern("dim")?;
            let dimnames_sym = ctx.session.intern("dimnames")?;
            let names_sym = ctx.session.intern("names")?;
            let new_dim = ctx.session.attr(checked, dim_sym);
            let new_dimnames = ctx.session.attr(checked, dimnames_sym);
            let new_names = ctx.session.attr(checked, names_sym);
            let target = ctx.session.shallow_duplicate(checked)?;
            ctx.protect(target);
            ctx.session.clear_attrs(target)?;
            if new_dim != SEXP_NIL {
                ctx.session.set_attrib(target, dim_sym, new_dim)?;
            }
            for (k, v) in ctx.session.attrs(obj)? {
                if k == dim_sym || k == dimnames_sym || k == names_sym {
                    continue;
                }
                ctx.session.set_attrib(target, k, v)?;
            }
            if new_dim != SEXP_NIL && new_dimnames != SEXP_NIL {
                ctx.session.set_attrib(target, dimnames_sym, new_dimnames)?;
            }
            if new_names != SEXP_NIL {
                ctx.session.set_attrib(target, names_sym, new_names)?;
            }
            ctx.session.set_s4_flag(target, true)?;
            return Ok(target);
        }
        let target = ctx.session.unshare(obj)?;
        ctx.protect(target);
        let stored = if checked == SEXP_NIL {
            SEXP_S4_NULL
        } else {
            checked
        };
        ctx.session.set_attrib(target, sym, stored)?;
        Ok(target)
    })
}

pub(super) fn standard_generic(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let name = ctx.string(args.get(0), call.err_call())?;
    let sym = ctx.session.intern(&name)?;
    let Some(frame_id) = ctx.current_frame_id() else {
        return Err(ctx.stop(
            call.err_call(),
            "standardGeneric() called outside a generic function",
        ));
    };
    let Some(hooks) = ctx.s4_hooks else {
        return Err(ctx.stop(call.err_call(), "S4 methods support is not loaded"));
    };
    hooks.standard_generic(ctx, sym, frame_id)
}

pub(super) fn is_s4_prim(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let flag = ctx.session.is_s4(args.get(0))?;
    Ok(ctx.session.scalar_lgl(RLogical::from_option(Some(flag)))?)
}

pub(super) fn as_s4_prim(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let obj = args.get(0);
    let flag = if args.len() >= 2 {
        ctx.truth(args.get(1), call.err_call())?
    } else {
        true
    };
    if obj == SEXP_NIL {
        return Ok(SEXP_NIL);
    }
    let copy = ctx.session.shallow_duplicate(obj)?;
    ctx.session.set_s4_flag(copy, flag)?;
    Ok(copy)
}
