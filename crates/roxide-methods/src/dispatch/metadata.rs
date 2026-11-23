use std::collections::HashSet;
use std::fmt::Write as _;

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{
    BASE_ENV, CharId, EMPTY_ENV, Encoding, GLOBAL_ENV, RLogical, SEXP_MISSING, SEXP_NIL, SexpType,
    Sym, ValueApi,
};

use super::resolver::{self, Class};
use crate::registry::{GenericInfo, MethodsExt};

const ANY: &str = "ANY";

pub(super) fn attr(ctx: &mut Ctx, value: SexpId, name: &str) -> EvalResult<SexpId> {
    let sym = ctx.session_mut().intern(name)?;
    Ok(ctx.session().attr(value, sym))
}

fn set_attr(ctx: &mut Ctx, value: SexpId, name: &str, data: SexpId) -> EvalResult<()> {
    let sym = ctx.session_mut().intern(name)?;
    Ok(ctx.session_mut().set_attrib(value, sym, data)?)
}

pub(super) fn strings(ctx: &mut Ctx, values: &[String]) -> EvalResult<SexpId> {
    let chars = values
        .iter()
        .map(|value| {
            ctx.session_mut()
                .mk_char(value.as_bytes(), Encoding::Native)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ctx.session_mut().alloc_str_from(&chars)?)
}

pub(super) fn texts(ctx: &mut Ctx, value: SexpId) -> EvalResult<Vec<String>> {
    if value == SEXP_NIL {
        return Ok(Vec::new());
    }
    let values = ctx.session().str_slice(value)?.to_vec();
    values
        .iter()
        .map(|&ch| {
            if ch == CharId::NA {
                return Err(ctx.stop(SEXP_NIL, "missing class or argument name"));
            }
            Ok(String::from_utf8_lossy(ctx.session().char_bytes(ch)?).into_owned())
        })
        .collect()
}

fn text(ctx: &mut Ctx, value: SexpId) -> EvalResult<String> {
    match texts(ctx, value)?.as_slice() {
        [name] if !name.is_empty() => Ok(name.clone()),
        _ => Err(ctx.stop(SEXP_NIL, "expected one nonempty name")),
    }
}

pub(super) fn flag(ctx: &Ctx, value: SexpId) -> bool {
    ctx.session()
        .lgl(value)
        .is_ok_and(|v| v.get(0) == RLogical::TRUE)
}

fn logical(ctx: &mut Ctx, value: bool) -> EvalResult<SexpId> {
    Ok(ctx.session_mut().scalar_lgl(if value {
        RLogical::TRUE
    } else {
        RLogical::FALSE
    })?)
}

fn set_class(ctx: &mut Ctx, value: SexpId, name: &str) -> EvalResult<()> {
    let class = package_string(ctx, name, "methods")?;
    set_attr(ctx, value, "class", class)?;
    Ok(ctx.session_mut().set_s4_flag(value, true)?)
}

pub(super) fn package_string(ctx: &mut Ctx, value: &str, pkg: &str) -> EvalResult<SexpId> {
    let value = ctx.session_mut().scalar_str(value)?;
    let pkg = ctx.session_mut().scalar_str(pkg)?;
    set_attr(ctx, value, "package", pkg)?;
    Ok(value)
}

pub(super) fn formal_pairs(ctx: &Ctx, fun: SexpId) -> EvalResult<Vec<(Sym, SexpId)>> {
    let (formals, _, _) = ctx.session().closure_parts(fun)?;
    Ok(ctx
        .session()
        .pairlist_iter(formals)
        .filter_map(|(tag, default)| tag.map(|tag| (tag, default)))
        .collect())
}

pub(super) fn make_call(
    ctx: &mut Ctx,
    head: SexpId,
    args: &[(Option<Sym>, SexpId)],
) -> EvalResult<SexpId> {
    let mut tail = SEXP_NIL;
    for &(tag, value) in args.iter().rev() {
        tail = ctx
            .session_mut()
            .cons(value, tail, tag.map_or(SEXP_NIL, Sym::as_sexp))?;
    }
    Ok(ctx.session_mut().lcons(head, tail, SEXP_NIL)?)
}

fn bind(ctx: &mut Ctx, env: SexpId, name: &str, value: SexpId) -> EvalResult<()> {
    let key = ctx.session_mut().intern(name)?;
    Ok(ctx.session_mut().env_define(env, key, value)?)
}

fn binding(ctx: &mut Ctx, env: SexpId, name: &str) -> EvalResult<Option<SexpId>> {
    let key = ctx.session_mut().intern(name)?;
    Ok(ctx.session().env_get(env, key).map(|b| b.value))
}

fn table_name(name: &str, package: &str) -> String {
    format!(".__T__{name}:{package}")
}

pub(super) fn canonical(
    ctx: &mut Ctx,
    table: SexpId,
    name: &str,
    package: &str,
) -> EvalResult<GenericInfo> {
    let signature = attr(ctx, table, "signature")?;
    Ok(GenericInfo {
        name: name.to_owned(),
        package: package.to_owned(),
        signature: texts(ctx, signature)?,
        table,
        default_fun: attr(ctx, table, "default")?,
    })
}

pub(super) fn find_table(
    ctx: &mut Ctx,
    name: &str,
    package: &str,
    mut where_: SexpId,
) -> EvalResult<Option<GenericInfo>> {
    let prefix = format!(".__T__{name}:");
    let mut visited = HashSet::new();
    while where_ != EMPTY_ENV && where_ != SEXP_NIL && visited.insert(where_) {
        if !package.is_empty() {
            if let Some(table) = binding(ctx, where_, &table_name(name, package))?
                && ctx.session().type_of(table)? == SexpType::Env
            {
                return canonical(ctx, table, name, package).map(Some);
            }
            where_ = ctx.session().env_enclos(where_)?;
            continue;
        }
        for (sym, binding) in ctx.session().env_entries(where_)? {
            let key = ctx.session().sym_name(sym)?;
            if let Some(pkg) = key.strip_prefix(&prefix)
                && ctx.session().type_of(binding.value)? == SexpType::Env
            {
                let pkg = pkg.to_owned();
                return canonical(ctx, binding.value, name, &pkg).map(Some);
            }
        }
        where_ = ctx.session().env_enclos(where_)?;
    }
    Ok(None)
}

pub(super) fn lookup(
    ctx: &mut Ctx,
    name: &str,
    package: &str,
    where_: SexpId,
) -> EvalResult<Option<GenericInfo>> {
    if let Some(info) = find_table(ctx, name, package, where_)? {
        return Ok(Some(info));
    }
    let ns = ctx.ns_lookup("methods").unwrap_or(GLOBAL_ENV);
    let primitive = super::invoke::primitive_formals(name);
    let pkg = if name == "show" {
        "methods"
    } else if let Some((_, _, pkg, _)) = primitive {
        pkg
    } else {
        return Ok(None);
    };
    if !package.is_empty() && package != pkg {
        return Ok(None);
    }
    if let Some(table) = binding(ctx, ns, &table_name(name, pkg))? {
        return canonical(ctx, table, name, pkg).map(Some);
    }
    if primitive.is_none() {
        return Ok(None);
    }
    let spec = GenericSpec {
        name,
        package: pkg,
        where_: ns,
        def: SEXP_NIL,
        signature: SEXP_NIL,
        default: SEXP_NIL,
        group: SEXP_NIL,
        value_class: SEXP_NIL,
    };
    create(ctx, &spec, true).map(Some)
}

struct GenericSpec<'a> {
    name: &'a str,
    package: &'a str,
    where_: SexpId,
    def: SexpId,
    signature: SexpId,
    default: SexpId,
    group: SexpId,
    value_class: SexpId,
}

fn calls_standard_generic(ctx: &Ctx, expr: SexpId) -> EvalResult<bool> {
    let mut pending = vec![expr];
    while let Some(expr) = pending.pop() {
        if ctx.session().type_of(expr)? != SexpType::Lang {
            continue;
        }
        let (head, rest, _) = ctx.session().cons_parts(expr)?;
        if ctx.session().sym_name_of(head) == Ok("standardGeneric") {
            return Ok(true);
        }
        if ctx.session().sym_name_of(head) != Ok("function") {
            pending.extend(ctx.session().pairlist_iter(rest).map(|(_, value)| value));
        }
    }
    Ok(false)
}

fn create(ctx: &mut Ctx, spec: &GenericSpec<'_>, primitive: bool) -> EvalResult<GenericInfo> {
    let GenericSpec {
        name,
        package,
        where_,
        def,
        signature,
        default,
        group,
        value_class,
    } = *spec;
    let generic_sym = ctx.session_mut().intern(name)?;
    let standard = ctx.session_mut().intern("standardGeneric")?;
    let name_value = package_string(ctx, name, package)?;
    let (formals, params, env, body, default) = if primitive {
        let Some((params, signature, _, builtin)) = super::invoke::primitive_formals(name) else {
            return Err(ctx.stop(SEXP_NIL, "unknown primitive generic"));
        };
        let mut formals = SEXP_NIL;
        for param in params.iter().rev() {
            let tag = ctx.session_mut().intern(param)?;
            let default = match *param {
                "drop" => ctx.session_mut().scalar_lgl(RLogical::TRUE)?,
                "na.rm" => ctx.session_mut().scalar_lgl(RLogical::FALSE)?,
                _ => SEXP_MISSING,
            };
            formals = ctx.session_mut().cons(default, formals, tag.as_sexp())?;
        }
        let default = if builtin {
            binding(ctx, BASE_ENV, name)?.unwrap_or(SEXP_NIL)
        } else {
            SEXP_NIL
        };
        (
            formals,
            signature.iter().map(|s| (*s).to_owned()).collect(),
            where_,
            None,
            default,
        )
    } else {
        let def = if def == SEXP_NIL {
            ctx.find_fun(generic_sym, where_)?
        } else {
            def
        };
        let (formals, body, env) = ctx.session().closure_parts(def)?;
        let params = formal_pairs(ctx, def)?
            .into_iter()
            .map(|(sym, _)| ctx.session().sym_name(sym).map(str::to_owned))
            .filter(|name| name.as_ref().is_ok_and(|name| name != "..."))
            .collect::<Result<Vec<_>, _>>()?;
        if calls_standard_generic(ctx, body)? {
            (formals, params, env, Some(body), default)
        } else {
            let default = if default == SEXP_NIL { def } else { default };
            (formals, params, env, None, default)
        }
    };
    let signature = if signature == SEXP_NIL {
        params.clone()
    } else {
        texts(ctx, signature)?
    };
    for (i, param) in signature.iter().enumerate() {
        if !params.contains(param) || signature[..i].contains(param) {
            return Err(ctx.stop(
                SEXP_NIL,
                format!("invalid generic signature argument ‘{param}’"),
            ));
        }
    }
    let body = match body {
        Some(body) => body,
        None => make_call(ctx, standard.as_sexp(), &[(None, name_value)])?,
    };
    let fun = ctx.session_mut().alloc_closure(formals, body, env)?;
    let table = ctx.session_mut().alloc_env(EMPTY_ENV)?;
    let signature_value = strings(ctx, &signature)?;
    set_class(ctx, fun, "standardGeneric")?;
    let package_value = ctx.session_mut().scalar_str(package)?;
    for (key, value) in [
        ("generic", name_value),
        ("package", package_value),
        ("signature", signature_value),
        ("group", group),
        ("valueClass", value_class),
    ] {
        set_attr(ctx, fun, key, value)?;
    }
    if !primitive || super::invoke::primitive_active(name) {
        let active = logical(ctx, true)?;
        set_attr(ctx, table, ".rx_active", active)?;
    }
    set_attr(ctx, table, "signature", signature_value)?;
    set_attr(ctx, table, "generic", fun)?;
    set_attr(ctx, fun, ".rx_table", table)?;
    let mut info = GenericInfo {
        name: name.to_owned(),
        package: package.to_owned(),
        signature,
        table,
        default_fun: default,
    };
    if default != SEXP_NIL {
        let sig = vec![ANY.to_owned(); info.signature.len()];
        if ctx.session().type_of(default)? == SexpType::Clo {
            info.default_fun =
                method_definition(ctx, &info, &sig, &sig, default, "derivedDefaultMethod")?;
        }
        bind(ctx, table, "ANY", info.default_fun)?;
        set_attr(ctx, fun, "default", info.default_fun)?;
        set_attr(ctx, table, "default", info.default_fun)?;
    }
    bind(ctx, where_, &table_name(name, package), table)?;
    if !primitive {
        bind(ctx, where_, name, fun)?;
    }
    ctx.ext::<MethodsExt>()
        .generics
        .insert((name.to_owned(), package.to_owned()), info.clone());
    Ok(info)
}

pub(super) fn set_generic(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let name = text(ctx, args.get(0))?;
    let where_ = args.get(2);
    let package = if args.get(3) != SEXP_NIL {
        text(ctx, args.get(3))?
    } else {
        ctx.namespace_name(where_)
            .unwrap_or_else(|| ".GlobalEnv".to_owned())
    };
    let def = if args.get(6) == SEXP_NIL {
        args.get(1)
    } else {
        args.get(6)
    };
    let spec = GenericSpec {
        name: &name,
        package: &package,
        where_,
        def,
        signature: args.get(4),
        default: args.get(5),
        group: args.get(7),
        value_class: args.get(8),
    };
    let primitive = def == SEXP_NIL && super::invoke::primitive_formals(&name).is_some();
    if primitive {
        if let Some(mut info) = lookup(ctx, &name, "", where_)? {
            local_table(ctx, &mut info, where_)?;
            activate(ctx, &info)?;
            ctx.ext::<MethodsExt>()
                .generics
                .insert((name.clone(), info.package.clone()), info);
        }
    } else if def != SEXP_NIL || find_table(ctx, &name, &package, where_)?.is_none() {
        create(ctx, &spec, false)?;
    }
    ctx.ext::<MethodsExt>().bump_epoch();
    Ok(ctx.session_mut().scalar_str(&name)?)
}

pub(super) fn get_generic(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let name = text(ctx, args.get(0))?;
    let package = texts(ctx, args.get(2))?
        .into_iter()
        .next()
        .unwrap_or_default();
    match lookup(ctx, &name, &package, args.get(1))? {
        Some(info) => attr(ctx, info.table, "generic"),
        None if flag(ctx, args.get(3)) => Err(ctx.stop(
            call.err_call(),
            format!("no generic function found for ‘{name}’"),
        )),
        None => Ok(SEXP_NIL),
    }
}

pub(super) fn is_generic(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let name = text(ctx, args.get(0))?;
    let where_ = args.get(1);
    let fdef = args.get(2);
    let get_name = flag(ctx, args.get(3));
    let explicit = fdef != SEXP_NIL;
    let info = if explicit {
        by_definition(ctx, fdef, &name)?
    } else {
        lookup(ctx, &name, "", where_)?
    };
    if let Some(info) = info {
        let active = attr(ctx, info.table, ".rx_active")?;
        if explicit || flag(ctx, active) {
            if get_name {
                return package_string(ctx, &name, &info.package);
            }
            return logical(ctx, true);
        }
    }
    logical(ctx, false)
}

pub(super) fn remove_generic(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let name = text(ctx, args.get(0))?;
    let where_ = args.get(1);
    let Some(info) = find_table(ctx, &name, "", where_)? else {
        return logical(ctx, false);
    };
    let key = ctx
        .session_mut()
        .intern(&table_name(&name, &info.package))?;
    if !ctx.session_mut().env_remove(where_, key)? {
        return logical(ctx, false);
    }
    let key = ctx.session_mut().intern(&name)?;
    let fun = attr(ctx, info.table, "generic")?;
    if ctx
        .session()
        .env_get(where_, key)
        .is_some_and(|binding| binding.value == fun)
    {
        ctx.session_mut().env_remove(where_, key)?;
    }
    let registry = ctx.ext::<MethodsExt>();
    registry.generics.remove(&(name, info.package));
    registry.bump_epoch();
    logical(ctx, true)
}

fn normalize(ctx: &mut Ctx, info: &GenericInfo, value: SexpId) -> EvalResult<Vec<String>> {
    let classes = texts(ctx, value)?;
    let names = attr(ctx, value, "names")?;
    let names = texts(ctx, names)?;
    let mut sig = vec![None; info.signature.len()];
    for (i, class) in classes
        .iter()
        .enumerate()
        .filter(|(i, _)| names.get(*i).is_some_and(|name| !name.is_empty()))
    {
        let name = &names[i];
        let Some(pos) = info.signature.iter().position(|param| param == name) else {
            return Err(ctx.stop(
                SEXP_NIL,
                format!("argument ‘{name}’ is not in the generic signature"),
            ));
        };
        if sig[pos].replace(class.clone()).is_some() {
            return Err(ctx.stop(SEXP_NIL, format!("duplicate signature argument ‘{name}’")));
        }
    }
    let mut free = sig.iter_mut().filter(|slot| slot.is_none());
    for (_, class) in classes
        .into_iter()
        .enumerate()
        .filter(|(i, _)| names.get(*i).is_none_or(String::is_empty))
    {
        let Some(slot) = free.next() else {
            return Err(ctx.stop(
                SEXP_NIL,
                "more classes than arguments in the generic signature",
            ));
        };
        *slot = Some(class);
    }
    Ok(sig
        .into_iter()
        .map(|slot| slot.unwrap_or_else(|| ANY.to_owned()))
        .collect())
}

fn sig_width(sig: &[String]) -> usize {
    sig.iter().rposition(|cl| cl != ANY).map_or(1, |i| i + 1)
}

fn method_key(sig: &[String], width: usize) -> String {
    if sig.is_empty() {
        ANY.to_owned()
    } else {
        sig[..width.min(sig.len())].join("#")
    }
}

#[derive(Clone)]
pub(super) struct Method {
    pub(super) signature: Vec<String>,
    pub(super) fun: SexpId,
}

pub(super) fn methods(ctx: &Ctx, info: &GenericInfo) -> EvalResult<Vec<Method>> {
    let mut methods = Vec::new();
    for (sym, binding) in ctx.session().env_entries(info.table)? {
        let mut signature: Vec<_> = ctx
            .session()
            .sym_name(sym)?
            .split('#')
            .map(str::to_owned)
            .collect();
        signature.resize(info.signature.len(), ANY.to_owned());
        methods.push(Method {
            signature,
            fun: binding.value,
        });
    }
    methods.sort_by(|a, b| a.signature.cmp(&b.signature));
    Ok(methods)
}

pub(super) fn signature_object(
    ctx: &mut Ctx,
    info: &GenericInfo,
    sig: &[String],
) -> EvalResult<SexpId> {
    let value = strings(ctx, sig)?;
    let names = strings(ctx, &info.signature[..sig.len()])?;
    set_attr(ctx, value, "names", names)?;
    let packages = sig
        .iter()
        .map(|class| {
            crate::registry::class_info(ctx, class, None)
                .map_or_else(|| "methods".to_owned(), |info| info.package)
        })
        .collect::<Vec<_>>();
    let packages = strings(ctx, &packages)?;
    set_attr(ctx, value, "package", packages)?;
    set_class(ctx, value, "signature")?;
    Ok(value)
}

fn method_definition(
    ctx: &mut Ctx,
    info: &GenericInfo,
    defined: &[String],
    target: &[String],
    fun: SexpId,
    class: &str,
) -> EvalResult<SexpId> {
    if ctx.session().type_of(fun)? != SexpType::Clo {
        return Ok(fun);
    }
    let (formals, body, env) = ctx.session().closure_parts(fun)?;
    let fun = ctx.session_mut().alloc_closure(formals, body, env)?;
    let defined = signature_object(ctx, info, defined)?;
    let target = signature_object(ctx, info, target)?;
    let generic = package_string(ctx, &info.name, &info.package)?;
    for (key, value) in [
        ("target", target),
        ("defined", defined),
        ("generic", generic),
    ] {
        set_attr(ctx, fun, key, value)?;
    }
    set_class(ctx, fun, class)?;
    Ok(fun)
}

fn missing_defaults(ctx: &mut Ctx, generic: SexpId, definition: SexpId) -> EvalResult<SexpId> {
    let (_, body, env) = ctx.session().closure_parts(definition)?;
    let formal_pairs = formal_pairs(ctx, definition)?;
    let generic_formals = self::formal_pairs(ctx, generic)?;
    let mut new = SEXP_NIL;
    let mut changed = false;
    for (tag, mut default) in formal_pairs.into_iter().rev() {
        if default == SEXP_MISSING
            && let Some(&(_, inherited)) = generic_formals.iter().find(|&&(sym, _)| sym == tag)
        {
            changed |= inherited != SEXP_MISSING;
            default = inherited;
        }
        new = ctx.session_mut().cons(default, new, tag.as_sexp())?;
    }
    if changed {
        Ok(ctx.session_mut().alloc_closure(new, body, env)?)
    } else {
        Ok(definition)
    }
}

fn local_table(ctx: &mut Ctx, info: &mut GenericInfo, where_: SexpId) -> EvalResult<()> {
    let key = table_name(&info.name, &info.package);
    if binding(ctx, where_, &key)?.is_none() {
        let table = ctx.session_mut().alloc_env(EMPTY_ENV)?;
        for (sym, value) in ctx.session().attrs(info.table)? {
            ctx.session_mut().set_attrib(table, sym, value)?;
        }
        for (sym, value) in ctx.session().env_entries(info.table)? {
            ctx.session_mut().env_define_binding(table, sym, value)?;
        }
        bind(ctx, where_, &key, table)?;
        info.table = table;
    }
    Ok(())
}

fn activate(ctx: &mut Ctx, info: &GenericInfo) -> EvalResult<()> {
    let active = logical(ctx, true)?;
    set_attr(ctx, info.table, ".rx_active", active)
}

pub(super) fn set_method(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let name = text(ctx, args.get(0))?;
    let where_ = args.get(3);
    let sig_arg = args.get(1);
    let definition = args.get(2);
    let mut info = match lookup(ctx, &name, "", where_)? {
        Some(info) => info,
        None => {
            let pkg = ctx
                .namespace_name(where_)
                .unwrap_or_else(|| ".GlobalEnv".to_owned());
            let spec = GenericSpec {
                name: &name,
                package: &pkg,
                where_,
                def: SEXP_NIL,
                signature: SEXP_NIL,
                default: SEXP_NIL,
                group: SEXP_NIL,
                value_class: SEXP_NIL,
            };
            create(ctx, &spec, false)?
        }
    };
    let signature = normalize(ctx, &info, sig_arg)?;
    let generic = attr(ctx, info.table, "generic")?;
    let definition = missing_defaults(ctx, generic, definition)?;
    let definition = method_definition(
        ctx,
        &info,
        &signature,
        &signature,
        definition,
        "MethodDefinition",
    )?;
    if flag(ctx, args.get(5)) {
        let sealed = logical(ctx, true)?;
        set_attr(ctx, definition, "sealed", sealed)?;
    }
    local_table(ctx, &mut info, where_)?;
    let old = methods(ctx, &info)?;
    let width = old
        .iter()
        .map(|m| sig_width(&m.signature))
        .max()
        .unwrap_or(1);
    let new_width = width.max(sig_width(&signature));
    let key = method_key(&signature, width);
    if new_width == width
        && let Some(prior) = binding(ctx, info.table, &key)?
    {
        let sealed = attr(ctx, prior, "sealed")?;
        if flag(ctx, sealed) {
            return Err(ctx.stop(call.err_call(), "the method is sealed"));
        }
    }
    if width != new_width {
        for method in old {
            let previous = ctx
                .session_mut()
                .intern(&method_key(&method.signature, width))?;
            ctx.session_mut().env_remove(info.table, previous)?;
            bind(
                ctx,
                info.table,
                &method_key(&method.signature, new_width),
                method.fun,
            )?;
        }
    }
    bind(
        ctx,
        info.table,
        &method_key(&signature, new_width),
        definition,
    )?;
    activate(ctx, &info)?;
    let registry = ctx.ext::<MethodsExt>();
    registry
        .generics
        .insert((name.clone(), info.package.clone()), info);
    registry.bump_epoch();
    Ok(ctx.session_mut().scalar_str(&name)?)
}

pub(super) fn remove_method(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let name = text(ctx, args.get(0))?;
    let Some(info) = lookup(ctx, &name, "", args.get(2))? else {
        return logical(ctx, false);
    };
    let signature = normalize(ctx, &info, args.get(1))?;
    let old = methods(ctx, &info)?;
    let width = old
        .iter()
        .map(|m| sig_width(&m.signature))
        .max()
        .unwrap_or(1);
    if sig_width(&signature) > width {
        return logical(ctx, false);
    }
    let key = method_key(&signature, width);
    if let Some(prior) = binding(ctx, info.table, &key)? {
        let sealed = attr(ctx, prior, "sealed")?;
        if flag(ctx, sealed) {
            return Err(ctx.stop(call.err_call(), "the method is sealed"));
        }
    }
    let key = ctx.session_mut().intern(&key)?;
    let removed = ctx.session_mut().env_remove(info.table, key)?;
    if removed {
        let new_width = old
            .iter()
            .filter(|m| m.signature != signature)
            .map(|m| sig_width(&m.signature))
            .max()
            .unwrap_or(1);
        if new_width != width {
            for method in old.into_iter().filter(|m| m.signature != signature) {
                let previous = ctx
                    .session_mut()
                    .intern(&method_key(&method.signature, width))?;
                ctx.session_mut().env_remove(info.table, previous)?;
                bind(
                    ctx,
                    info.table,
                    &method_key(&method.signature, new_width),
                    method.fun,
                )?;
            }
        }
        ctx.ext::<MethodsExt>().bump_epoch();
    }
    logical(ctx, removed)
}

pub(super) fn by_definition(
    ctx: &mut Ctx,
    fun: SexpId,
    name: &str,
) -> EvalResult<Option<GenericInfo>> {
    let table = attr(ctx, fun, ".rx_table")?;
    if table == SEXP_NIL {
        return Ok(None);
    }
    let package = attr(ctx, fun, "package")?;
    let package = text(ctx, package)?;
    canonical(ctx, table, name, &package).map(Some)
}

pub(super) fn select_method(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let name = text(ctx, args.get(0))?;
    let optional = flag(ctx, args.get(3));
    let info = match by_definition(ctx, args.get(5), &name)? {
        Some(info) => Some(info),
        None => lookup(ctx, &name, "", args.get(2))?,
    };
    if let Some(mut info) = info {
        if ctx.session().type_of(args.get(7))? == SexpType::Env {
            info.table = args.get(7);
        }
        let signature = normalize(ctx, &info, args.get(1))?;
        let inherited = flag(ctx, args.get(4));
        let target = signature
            .iter()
            .cloned()
            .map(Class::unqualified)
            .collect::<Vec<_>>();
        if let Some(method) = resolver::select(
            ctx,
            &info,
            &target,
            resolver::Selection {
                inherited,
                next: false,
                cache: flag(ctx, args.get(6)),
            },
            call.err_call(),
        )? {
            if method.signature == signature {
                return Ok(method.fun);
            }
            ctx.protect(method.fun);
            return method_definition(
                ctx,
                &info,
                &method.signature,
                &signature,
                method.fun,
                "MethodDefinition",
            );
        }
    }
    if optional {
        return Ok(SEXP_NIL);
    }
    let signature = texts(ctx, args.get(1))?.join(", ");
    Err(ctx.stop(
        call.err_call(),
        format!("no method found for function ‘{name}’ and signature {signature}"),
    ))
}

struct MethodListing {
    entries: Vec<(Vec<String>, Method)>,
    width: usize,
}

fn listing(
    ctx: &mut Ctx,
    info: &GenericInfo,
    classes: SexpId,
    inherited: bool,
) -> EvalResult<MethodListing> {
    let classes = texts(ctx, classes)?;
    let mut entries = methods(ctx, info)?
        .into_iter()
        .map(|method| (method.signature.clone(), method))
        .collect::<Vec<_>>();
    if inherited {
        entries.extend(resolver::inherited(ctx, info));
    }
    let width = entries
        .iter()
        .map(|(target, _)| sig_width(target))
        .max()
        .unwrap_or(1);
    entries.retain(|(target, _)| {
        classes.is_empty() || target.iter().any(|class| classes.contains(class))
    });
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries.dedup_by(|a, b| a.0 == b.0);
    Ok(MethodListing { entries, width })
}

pub(super) fn find_methods(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let name = text(ctx, args.get(0))?;
    let package = texts(ctx, args.get(4))?
        .into_iter()
        .next()
        .unwrap_or_default();
    let Some(info) = lookup(ctx, &name, &package, args.get(1))? else {
        ctx.warn_now(
            call.err_call(),
            format!("non-generic function '{name}' given to findMethods()"),
        )?;
        return Ok(ctx.session_mut().alloc_list(0)?);
    };
    let inherited = flag(ctx, args.get(3));
    let MethodListing { entries, width } = listing(ctx, &info, args.get(2), inherited)?;
    let list = ctx.session_mut().alloc_list(entries.len())?;
    let mut names = Vec::new();
    for (i, (target, method)) in entries.into_iter().enumerate() {
        let fun = if target == method.signature {
            method.fun
        } else {
            method_definition(
                ctx,
                &info,
                &method.signature,
                &target,
                method.fun,
                "MethodDefinition",
            )?
        };
        ctx.session_mut().list_set(list, i, fun)?;
        names.push(method_key(&target, width));
    }
    let names = strings(ctx, &names)?;
    set_attr(ctx, list, "names", names)?;
    set_class(ctx, list, "listOfMethods")?;
    Ok(list)
}

fn describe(info: &GenericInfo, target: &[String], width: usize) -> String {
    info.signature
        .iter()
        .zip(target)
        .take(width)
        .map(|(arg, class)| format!("{arg}=\"{class}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn show_methods(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let mut names = texts(ctx, args.get(0))?;
    if names.is_empty() {
        names = ctx
            .ext::<MethodsExt>()
            .generics
            .keys()
            .map(|(name, _)| name.clone())
            .collect();
        names.sort();
        names.dedup();
    }
    let include_defs = flag(ctx, args.get(3));
    let inherited = flag(ctx, args.get(4));
    let show_empty = flag(ctx, args.get(5));
    let mut output = String::new();
    for name in names {
        let info = match by_definition(ctx, args.get(7), &name)? {
            Some(info) => Some(info),
            None => lookup(ctx, &name, "", args.get(1))?,
        };
        let Some(info) = info else {
            let _ = write!(
                output,
                "\nFunction \"{name}\":\n <not an S4 generic function>\n"
            );
            continue;
        };
        let MethodListing { entries, width } = listing(ctx, &info, args.get(2), inherited)?;
        if entries.is_empty() && !show_empty {
            continue;
        }
        let _ = writeln!(output, "Function: {name} (package {})", info.package);
        if entries.is_empty() {
            output.push_str("<No methods>\n");
        }
        for (target, method) in entries {
            let _ = writeln!(output, "{}", describe(&info, &target, width));
            if target != method.signature {
                let _ = writeln!(
                    output,
                    "    (inherited from: {})",
                    describe(&info, &method.signature, width)
                );
            }
            if include_defs {
                let fun = if ctx.session().type_of(method.fun)? == SexpType::Clo {
                    let (formals, body, env) = ctx.session().closure_parts(method.fun)?;
                    ctx.session_mut().alloc_closure(formals, body, env)?
                } else {
                    method.fun
                };
                let lines = ctx.deparse_lines(fun, Default::default())?;
                output.push_str(&lines.join("\n"));
                output.push_str("\n\n\n");
            }
        }
        output.push('\n');
    }
    if !output.is_empty() {
        let tag = ctx.session_mut().intern("file")?;
        let value = ctx.session_mut().scalar_str(&output)?;
        ctx.call_function("cat", &[(None, value), (Some(tag), args.get(6))], BASE_ENV)?;
    }
    Ok(SEXP_NIL)
}
