use roxide_eval::{Args, Call, Ctx, CtxExt, EvalResult, FrameId};
use roxide_gc::{SexpId, Trace, Tracer};
use roxide_value::{
    BASE_ENV, EMPTY_ENV, PromiseState, SEXP_MISSING, SEXP_NIL, SexpType, Sym, ValueApi,
};

use super::metadata::{
    Method, attr, by_definition, canonical_generic_name, dispatch_tables, formal_pairs,
    generic_definition, lookup, make_call, merge_tables, methods, package_string, signature_object,
    texts,
};
use super::resolver::{self, Class, Selection};
use crate::registry::GenericInfo;

type PrimitiveFormals = (
    &'static [&'static str],
    &'static [&'static str],
    &'static str,
    bool,
);

pub(super) fn primitive_formals(name: &str) -> Option<PrimitiveFormals> {
    let (params, signature, package, builtin): PrimitiveFormals = match name {
        "[" => (
            &["x", "i", "j", "...", "drop"],
            &["x", "i", "j", "drop"],
            "base",
            true,
        ),
        "[[" => (&["x", "i", "j", "..."], &["x", "i", "j"], "base", true),
        "[<-" | "[[<-" => (
            &["x", "i", "j", "...", "value"],
            &["x", "i", "j", "value"],
            "base",
            true,
        ),
        "$" => (&["x", "name"], &["x"], "base", true),
        "$<-" => (&["x", "name", "value"], &["x", "value"], "base", true),
        "length<-" | "names<-" | "dim<-" | "dimnames<-" | "levels<-" => {
            (&["x", "value"], &["x", "value"], "base", true)
        }
        "!" | "length" | "names" | "dim" | "dimnames" | "levels" | "as.raw" => {
            (&["x"], &["x"], "base", true)
        }
        "as.character" | "as.numeric" | "as.double" | "as.integer" | "as.logical"
        | "as.complex" | "as.list" | "c" => (&["x", "..."], &["x"], "base", true),
        "as.vector" => (&["x", "mode"], &["x", "mode"], "base", true),
        "Ops" | "Arith" | "Logic" => (&["e1", "e2"], &["e1", "e2"], "base", false),
        "Compare" => (&["e1", "e2"], &["e1", "e2"], "methods", false),
        "round" | "signif" => (&["x", "digits"], &["x", "digits"], "base", true),
        "Math2" => (&["x", "digits"], &["x", "digits"], "methods", false),
        "Math" => (&["x"], &["x"], "base", false),
        "Complex" => (&["z"], &["z"], "base", false),
        "Summary" => (&["x", "...", "na.rm"], &["x", "na.rm"], "base", false),
        _ => match super::groups::parent(name)? {
            "Arith" | "Compare" | "Logic" => (&["e1", "e2"], &["e1", "e2"], "base", true),
            "Math" if matches!(name, "log" | "trunc") => (&["x", "..."], &["x"], "base", true),
            "Math" => (&["x"], &["x"], "base", true),
            "Summary" => (&["x", "...", "na.rm"], &["x", "na.rm"], "base", true),
            "Complex" => (&["z"], &["z"], "base", true),
            _ => return None,
        },
    };
    Some((params, signature, package, builtin))
}

pub(super) fn primitive_active(name: &str) -> bool {
    !matches!(
        name,
        "[[" | "length" | "names" | "!" | "as.list" | "levels" | "levels<-"
    ) && super::groups::parent(name) != Some("Summary")
}

#[derive(Clone)]
struct Invocation {
    info: GenericInfo,
    target: Vec<String>,
    method: Method,
    marker: SexpId,
    primitive: Option<String>,
}

#[derive(Default)]
struct MethodStack(Vec<Invocation>);

impl Trace for MethodStack {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        for method in &self.0 {
            tracer.edge(method.info.table);
            tracer.edge(method.info.default_fun);
            tracer.edge(method.method.fun);
            tracer.edge(method.marker);
        }
    }
}

impl CtxExt for MethodStack {}

fn force(ctx: &mut Ctx, value: SexpId) -> EvalResult<SexpId> {
    if ctx.session().type_of(value)? == SexpType::Prom {
        ctx.force(value)
    } else {
        Ok(value)
    }
}

fn target(ctx: &mut Ctx, info: &GenericInfo, matched: &[(Sym, SexpId)]) -> EvalResult<Vec<Class>> {
    info.signature
        .iter()
        .map(|param| {
            let tag = ctx.session_mut().intern(param)?;
            let value = matched
                .iter()
                .find(|&&(sym, _)| sym == tag)
                .map_or(SEXP_MISSING, |pair| pair.1);
            let value = force(ctx, value)?;
            Class::of(ctx, value)
        })
        .collect()
}

fn target_names(target: &[Class]) -> Vec<String> {
    target.iter().map(|c| c.name.clone()).collect()
}

fn forced_promise(ctx: &mut Ctx, value: SexpId) -> EvalResult<SexpId> {
    if value == SEXP_MISSING {
        return Ok(value);
    }
    let promise = ctx.session_mut().alloc_promise(value, SEXP_NIL)?;
    ctx.session_mut()
        .set_promise_state(promise, PromiseState::Forced, SEXP_NIL, value)?;
    Ok(promise)
}

fn flatten(ctx: &mut Ctx, matched: &[(Sym, SexpId)]) -> EvalResult<Vec<(Option<Sym>, SexpId)>> {
    let dots = ctx.session_mut().intern("...")?;
    let mut args = Vec::new();
    for &(tag, value) in matched {
        if tag == dots {
            if value != SEXP_MISSING {
                args.extend(ctx.session().pairlist_iter(value));
            }
        } else {
            args.push((Some(tag), value));
        }
    }
    Ok(args)
}

fn primitive_default(
    ctx: &mut Ctx,
    name: &str,
    matched: &[(Sym, SexpId)],
    caller: SexpId,
) -> EvalResult<SexpId> {
    let fun_sym = ctx.session_mut().intern(name)?;
    let fun = ctx.find_fun(fun_sym, BASE_ENV)?;
    let default = ctx.session_mut().intern(&format!("{name}.default"))?;
    let mut args = flatten(ctx, matched)?;
    while args.last().is_some_and(|&(_, value)| value == SEXP_MISSING) {
        args.pop();
    }
    for (_, value) in &mut args {
        *value = force(ctx, *value)?;
        ctx.protect(*value);
    }
    let call = make_call(ctx, default.as_sexp(), &args)?;
    ctx.apply(fun, &args, call, caller)
}

fn invoke(
    ctx: &mut Ctx,
    mut invocation: Invocation,
    matched: &[(Sym, SexpId)],
    call: SexpId,
    caller: SexpId,
) -> EvalResult<SexpId> {
    ctx.scoped(|ctx| {
        let fun = invocation.method.fun;
        ctx.protect(fun);
        ctx.protect(invocation.info.table);
        for &(_, value) in matched {
            ctx.protect(value);
        }
        if ctx.session().type_of(fun)? != SexpType::Clo {
            return primitive_default(ctx, &invocation.info.name, matched, caller);
        }
        let defined = signature_object(ctx, &invocation.info, &invocation.method.signature)?;
        let target = signature_object(ctx, &invocation.info, &invocation.target)?;
        let generic = package_string(ctx, &invocation.info.name, &invocation.info.package)?;
        let marker = ctx.session_mut().alloc_env(EMPTY_ENV)?;
        let values = [
            (".Generic", generic),
            (".Method", fun),
            (".defined", defined),
            (".target", target),
            (".rx_S4_method_frame", marker),
        ];
        let frame_vars = values
            .into_iter()
            .map(|(name, value)| Ok((ctx.session_mut().intern(name)?, value)))
            .collect::<EvalResult<Vec<_>>>()?;
        invocation.marker = marker;
        let matched = bind_method_dots(ctx, fun, matched)?;
        ctx.ext::<MethodStack>().0.push(invocation);
        let result = ctx.apply_matched(fun, &matched, call, caller, &frame_vars);
        ctx.ext::<MethodStack>().0.pop();
        result
    })
}

fn bind_method_dots(
    ctx: &mut Ctx,
    fun: SexpId,
    matched: &[(Sym, SexpId)],
) -> EvalResult<Vec<(Sym, SexpId)>> {
    let dots = ctx.session_mut().intern("...")?;
    let params = formal_pairs(ctx, fun)?;
    let Some(&(_, values)) = matched.iter().find(|&&(tag, _)| tag == dots) else {
        return Ok(matched.to_vec());
    };
    if values == SEXP_MISSING {
        return Ok(matched.to_vec());
    }
    let args = ctx.session().pairlist_iter(values).collect::<Vec<_>>();
    let extras = params
        .iter()
        .filter_map(|&(tag, _)| {
            (tag == dots || !matched.iter().any(|&(key, _)| key == tag)).then_some(tag)
        })
        .collect::<Vec<_>>();
    if extras.iter().all(|&tag| tag == dots) {
        return Ok(matched.to_vec());
    }
    let mut out = matched
        .iter()
        .filter(|&&(tag, _)| tag != dots)
        .copied()
        .collect::<Vec<_>>();
    out.extend(match_args(ctx, &extras, &args, SEXP_NIL, false)?);
    Ok(out)
}

pub(super) fn standard_generic(ctx: &mut Ctx, name: Sym, frame: FrameId) -> EvalResult<SexpId> {
    let Some((call, _, fun, caller)) = ctx.frame_info(frame) else {
        return Err(ctx.stop(
            SEXP_NIL,
            "standardGeneric() called outside a generic function",
        ));
    };
    let name = ctx.session().sym_name(name)?.to_owned();
    let package = attr(ctx, fun, "package")?;
    let package = texts(ctx, package)?.into_iter().next().unwrap_or_default();
    let info = match by_definition(ctx, fun, &name)? {
        Some(info) => Some(info),
        None => lookup(ctx, &name, &package, caller)?,
    };
    let Some(info) = info else {
        return Err(ctx.stop(call, format!("no generic function found for ‘{name}’")));
    };
    ctx.protect(info.table);
    let sources = dispatch_tables(ctx, &info, caller)?;
    let info = merge_tables(ctx, info, &sources, true)?;
    let value_class = attr(ctx, fun, "valueClass")?;
    let value_classes = texts(ctx, value_class)?;
    let matched = ctx.generic_frame_args(frame)?;
    let target = target(ctx, &info, &matched)?;
    let opts = Selection {
        inherited: true,
        next: false,
        cache: true,
    };
    let Some(method) = resolver::select(ctx, &info, &target, opts, call)? else {
        let definitions = methods(ctx, &info)?;
        let desc = info
            .signature
            .iter()
            .zip(&target)
            .enumerate()
            .filter(|(i, _)| {
                definitions
                    .iter()
                    .any(|method| method.signature.get(*i).is_some_and(|class| class != "ANY"))
            })
            .map(|(_, (param, class))| format!("{param} = \"{}\"", class.name))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(ctx.stop(
            call,
            format!(
                "unable to find an inherited method for function ‘{name}’ for signature ‘{desc}’"
            ),
        ));
    };
    let invocation = Invocation {
        info,
        target: target_names(&target),
        method,
        marker: SEXP_NIL,
        primitive: None,
    };
    let result = invoke(ctx, invocation, &matched, call, caller)?;
    if !value_classes.is_empty() && !value_classes.iter().any(|cl| cl == "ANY") {
        let class = Class::of(ctx, result)?;
        if !value_classes.contains(&class.name)
            && !class
                .ancestors(ctx)
                .iter()
                .any(|(parent, _, _)| value_classes.contains(parent))
        {
            let expected = value_classes.join("”, “");
            return Err(ctx.stop(call, format!("invalid value from generic function ‘{name}’, class “{}”, expected “{expected}”", class.name)));
        }
    }
    Ok(result)
}

fn match_args(
    ctx: &mut Ctx,
    params: &[Sym],
    args: &[(Option<Sym>, SexpId)],
    call: SexpId,
    reject_unused: bool,
) -> EvalResult<Vec<(Sym, SexpId)>> {
    let dots = ctx.session_mut().intern("...")?;
    let dots_at = params
        .iter()
        .position(|&tag| tag == dots)
        .unwrap_or(params.len());
    let mut values = vec![None; params.len()];
    let mut used = vec![false; args.len()];
    for (i, &(tag, value)) in args.iter().enumerate() {
        if let Some(tag) = tag
            && let Some(pos) = params
                .iter()
                .position(|&param| param == tag && param != dots)
        {
            if values[pos].replace(value).is_some() {
                return Err(ctx.stop(
                    call,
                    format!(
                        "formal argument \"{}\" matched by multiple actual arguments",
                        ctx.session().sym_name(tag)?
                    ),
                ));
            }
            used[i] = true;
        }
    }
    for (i, &(tag, value)) in args.iter().enumerate() {
        if let Some(tag) = tag.filter(|_| !used[i]) {
            let short = ctx.session().sym_name(tag)?;
            let mut matches = params[..dots_at].iter().enumerate().filter(|&(_, &p)| {
                ctx.session()
                    .sym_name(p)
                    .is_ok_and(|name| name.starts_with(short))
            });
            if let Some((pos, _)) = matches.next() {
                if matches.next().is_some() || values[pos].replace(value).is_some() {
                    return Err(ctx.stop(call, format!("ambiguous argument match for '{short}'")));
                }
                used[i] = true;
            }
        }
    }
    let mut positionals = args
        .iter()
        .zip(&used)
        .enumerate()
        .filter_map(|(i, (&(tag, value), &used))| (tag.is_none() && !used).then_some((i, value)));
    let mut positioned = Vec::new();
    for slot in &mut values[..dots_at] {
        if slot.is_none()
            && let Some((i, value)) = positionals.next()
        {
            *slot = Some(value);
            positioned.push(i);
        }
    }
    for i in positioned {
        used[i] = true;
    }
    let mut rest = SEXP_NIL;
    for (&(tag, value), &used) in args.iter().zip(&used).rev() {
        if used {
            continue;
        }
        if dots_at == params.len() {
            if reject_unused {
                return Err(ctx.stop(call, "unused arguments in method call"));
            }
        } else {
            rest = ctx
                .session_mut()
                .dcons(value, rest, tag.map_or(SEXP_NIL, Sym::as_sexp))?;
        }
    }
    if rest != SEXP_NIL {
        values[dots_at] = Some(rest);
    }
    Ok(params
        .iter()
        .copied()
        .zip(
            values
                .into_iter()
                .map(|value| value.unwrap_or(SEXP_MISSING)),
        )
        .collect())
}

fn active_method(ctx: &mut Ctx) -> EvalResult<Option<(Invocation, FrameId)>> {
    let marker = ctx.session_mut().intern(".rx_S4_method_frame")?;
    let mut frame = ctx.current_frame_id();
    while let Some(id) = frame {
        if let Some((_, env, _, _)) = ctx.frame_info(id)
            && let Some(binding) = ctx.session().env_get(env, marker)
            && let Some(method) = ctx
                .ext::<MethodStack>()
                .0
                .iter()
                .rfind(|m| m.marker == binding.value)
        {
            return Ok(Some((method.clone(), id)));
        }
        frame = ctx.parent_frame_id(id);
    }
    Ok(None)
}

fn explicit_args(
    ctx: &mut Ctx,
    call: &Call,
    args: &Args,
) -> EvalResult<Vec<(Option<Sym>, SexpId)>> {
    let mut promised = Vec::new();
    let dots = ctx.session_mut().intern("...")?;
    for (tag, expr) in args.iter() {
        if expr == dots.as_sexp() {
            let mut env = call.env;
            while env != EMPTY_ENV {
                if let Some(binding) = ctx.session().env_get(env, dots) {
                    promised.extend(ctx.session().pairlist_iter(binding.value));
                    break;
                }
                env = ctx.session().env_enclos(env)?;
            }
        } else {
            let value = if expr == SEXP_MISSING || ctx.session().type_of(expr)? == SexpType::Prom {
                expr
            } else {
                ctx.session_mut().alloc_promise(expr, call.env)?
            };
            promised.push((tag, value));
        }
    }
    Ok(promised)
}

pub(super) fn call_next_method(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult<SexpId> {
    let Some((mut invocation, frame)) = active_method(ctx)? else {
        return Err(ctx.stop(
            call.err_call(),
            "a call to callNextMethod() appears in a call to ‘callNextMethod’, but the call does not seem to come from either a generic function or another 'callNextMethod'",
        ));
    };
    let target = invocation
        .method
        .signature
        .iter()
        .cloned()
        .map(Class::unqualified)
        .collect::<Vec<_>>();
    let opts = Selection {
        inherited: true,
        next: true,
        cache: true,
    };
    let next = resolver::select(ctx, &invocation.info, &target, opts, call.err_call())?;
    let matched = if args.is_empty() {
        let matched = ctx.generic_frame_args(frame)?;
        let Some((_, _, fun, _)) = ctx.frame_info(frame) else {
            return Err(ctx.stop(call.err_call(), "unknown method frame"));
        };
        let generic = generic_definition(ctx, &invocation.info, call.env)?;
        let generic_formals = formal_pairs(ctx, generic)?;
        let method_formals = formal_pairs(ctx, fun)?;
        if generic_formals == method_formals {
            matched
        } else {
            let args = flatten(ctx, &matched)?;
            let names = generic_formals
                .iter()
                .map(|&(sym, _)| sym)
                .collect::<Vec<_>>();
            match_args(ctx, &names, &args, call.err_call(), false)?
        }
    } else {
        let generic = generic_definition(ctx, &invocation.info, call.env)?;
        let names = formal_pairs(ctx, generic)?
            .iter()
            .map(|&(sym, _)| sym)
            .collect::<Vec<_>>();
        let args = explicit_args(ctx, call, args)?;
        match_args(ctx, &names, &args, call.err_call(), true)?
    };
    let result = match next {
        Some(method)
            if ctx.session().type_of(method.fun)? == SexpType::Clo
                && (invocation.primitive.is_none()
                    || method.fun != invocation.info.default_fun) =>
        {
            invocation.method = method;
            invoke(ctx, invocation, &matched, call.expr, call.env)
        }
        _ if invocation.primitive.is_some() => primitive_default(
            ctx,
            invocation
                .primitive
                .as_deref()
                .unwrap_or(&invocation.info.name),
            &matched,
            call.env,
        ),
        _ => Err(ctx.stop(call.err_call(), "no next method available")),
    }?;
    ctx.set_visible(true);
    Ok(result)
}

pub(super) fn call_generic(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult<SexpId> {
    let Some((invocation, frame)) = active_method(ctx)? else {
        return Err(ctx.stop(
            call.err_call(),
            "no generic function found for ‘callGeneric’",
        ));
    };
    let sym = ctx.session_mut().intern(&invocation.info.name)?;
    if !args.is_empty() {
        let Some((_, env, _, _)) = ctx.frame_info(frame) else {
            return Err(ctx.stop(call.err_call(), "unknown method frame"));
        };
        let arguments = args.iter().collect::<Vec<_>>();
        let expr = make_call(ctx, sym.as_sexp(), &arguments)?;
        return ctx.eval(expr, env);
    }
    let matched = ctx.generic_frame_args(frame)?;
    let mut arguments = flatten(ctx, &matched)?;
    arguments.retain(|&(_, value)| value != SEXP_MISSING);
    let fun = if invocation.primitive.is_some() {
        ctx.find_fun(sym, BASE_ENV)?
    } else {
        generic_definition(ctx, &invocation.info, call.env)?
    };
    let expr = make_call(ctx, fun, &arguments)?;
    ctx.eval(expr, call.env)
}

pub(super) fn primitive_sources(
    ctx: &mut Ctx,
    info: &GenericInfo,
    env: SexpId,
) -> EvalResult<Vec<GenericInfo>> {
    let mut sources = dispatch_tables(ctx, info, env)?;
    for group in super::groups::parents(&info.name) {
        if let Some(info) = lookup(ctx, group, "", env)? {
            sources.extend(dispatch_tables(ctx, &info, env)?);
        }
    }
    Ok(sources)
}

pub(super) fn dispatch_primitive(
    ctx: &mut Ctx,
    generic: Sym,
    call: &Call,
    args: &Args,
) -> EvalResult<Option<SexpId>> {
    let name = canonical_generic_name(ctx.session().sym_name(generic)?).to_owned();
    let Some((params, _, package, _)) = primitive_formals(&name) else {
        return Ok(None);
    };
    let Some(info) = lookup(ctx, &name, package, call.env)? else {
        return Ok(None);
    };
    let sources = primitive_sources(ctx, &info, call.env)?;
    let info = merge_tables(ctx, info, &sources, false)?;
    let mut all_args = args
        .iter()
        .enumerate()
        .map(|(i, (tag, mut value))| {
            if matches!(name.as_str(), "$" | "$<-")
                && i == 1
                && let Ok(field) = ctx.session().sym_name_of(value)
            {
                let field = field.to_owned();
                value = ctx.session_mut().scalar_str(&field)?;
            }
            Ok((tag, forced_promise(ctx, value)?))
        })
        .collect::<EvalResult<Vec<_>>>()?;
    if super::groups::parent(&name) == Some("Summary") {
        let na_rm = ctx.session_mut().intern("na.rm")?;
        if !all_args.iter().any(|&(tag, _)| tag == Some(na_rm)) {
            all_args.push((
                Some(na_rm),
                forced_promise(ctx, roxide_value::FALSE_SCALAR)?,
            ));
        }
    }
    let params = params
        .iter()
        .map(|param| ctx.session_mut().intern(param))
        .collect::<Result<Vec<_>, _>>()?;
    let matched = match_args(ctx, &params, &all_args, call.err_call(), false)?;
    for &(_, value) in &matched {
        ctx.protect(value);
    }
    let target = target(ctx, &info, &matched)?;
    let opts = Selection {
        inherited: true,
        next: false,
        cache: true,
    };
    let Some(method) = resolver::select(ctx, &info, &target, opts, call.err_call())? else {
        return Ok(None);
    };
    if method.fun == info.default_fun || ctx.session().type_of(method.fun)? != SexpType::Clo {
        return Ok(None);
    }
    let invocation = Invocation {
        info,
        target: target_names(&target),
        method,
        marker: SEXP_NIL,
        primitive: Some(name),
    };
    invoke(ctx, invocation, &matched, call.expr, call.env).map(Some)
}
