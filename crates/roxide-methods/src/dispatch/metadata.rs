use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;

use roxide_eval::{Args, Call, Ctx, CtxExt, EvalResult};
use roxide_gc::{SexpId, Trace, Tracer};
use roxide_value::{
    BASE_ENV, CharId, EMPTY_ENV, Encoding, GLOBAL_ENV, RLogical, SEXP_MISSING, SEXP_NIL, SexpType,
    Sym, ValueApi,
};

use super::method_table::{
    ANY, Method, activate, local_table, method_definition, method_key, methods, normalize,
    sig_width,
};
use super::resolver::{self, Class};
use crate::registry::{GenericInfo, MethodsExt};

pub(crate) fn attr(ctx: &mut Ctx, value: SexpId, name: &str) -> EvalResult<SexpId> {
    let sym = ctx.session_mut().intern(name)?;
    Ok(ctx.session().attr(value, sym))
}

pub(crate) fn set_attr(ctx: &mut Ctx, value: SexpId, name: &str, data: SexpId) -> EvalResult<()> {
    let sym = ctx.session_mut().intern(name)?;
    Ok(ctx.session_mut().set_attrib(value, sym, data)?)
}

pub(crate) fn strings(ctx: &mut Ctx, values: &[String]) -> EvalResult<SexpId> {
    let chars = values
        .iter()
        .map(|value| {
            ctx.session_mut()
                .mk_char(value.as_bytes(), Encoding::Native)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ctx.session_mut().alloc_str_from(&chars)?)
}

pub(crate) fn texts(ctx: &mut Ctx, value: SexpId) -> EvalResult<Vec<String>> {
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

pub(super) fn text(ctx: &mut Ctx, value: SexpId) -> EvalResult<String> {
    match texts(ctx, value)?.as_slice() {
        [name] if !name.is_empty() => Ok(name.clone()),
        _ => Err(ctx.stop(SEXP_NIL, "expected one nonempty name")),
    }
}

pub(crate) fn flag(ctx: &Ctx, value: SexpId) -> bool {
    ctx.session()
        .lgl(value)
        .is_ok_and(|v| v.get(0) == RLogical::TRUE)
}

pub(super) fn logical(ctx: &mut Ctx, value: bool) -> EvalResult<SexpId> {
    Ok(ctx.session_mut().scalar_lgl(if value {
        RLogical::TRUE
    } else {
        RLogical::FALSE
    })?)
}

pub(crate) fn set_class(ctx: &mut Ctx, value: SexpId, name: &str) -> EvalResult<()> {
    let class = package_string(ctx, name, "methods")?;
    set_attr(ctx, value, "class", class)?;
    Ok(ctx.session_mut().set_s4_flag(value, true)?)
}

pub(crate) fn package_string(ctx: &mut Ctx, value: &str, pkg: &str) -> EvalResult<SexpId> {
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

pub(super) fn bind(ctx: &mut Ctx, env: SexpId, name: &str, value: SexpId) -> EvalResult<()> {
    let key = ctx.session_mut().intern(name)?;
    Ok(ctx.session_mut().env_define(env, key, value)?)
}

pub(super) fn binding(ctx: &mut Ctx, env: SexpId, name: &str) -> EvalResult<Option<SexpId>> {
    let key = ctx.session_mut().intern(name)?;
    Ok(ctx.session().env_get(env, key).map(|b| b.value))
}

pub(super) fn table_name(name: &str, package: &str) -> String {
    format!(".__T__{name}:{package}")
}

/// Read the signature and default for a canonical method table.
pub(crate) fn canonical(
    ctx: &mut Ctx,
    table: SexpId,
    name: &str,
    package: &str,
    env: SexpId,
) -> EvalResult<GenericInfo> {
    let mut signature = attr(ctx, table, "signature")?;
    let mut default_fun = attr(ctx, table, "default")?;
    if signature == SEXP_NIL {
        let definition = definition_in(ctx, name, env)?;
        signature = attr(ctx, definition, "signature")?;
        default_fun = attr(ctx, definition, "default")?;
    }
    let signature = if signature == SEXP_NIL {
        super::invoke::primitive_formals(name).map_or_else(Vec::new, |(_, signature, _, _)| {
            signature.iter().map(|param| (*param).to_owned()).collect()
        })
    } else {
        texts(ctx, signature)?
    };
    Ok(GenericInfo {
        name: name.to_owned(),
        package: package.to_owned(),
        signature,
        table,
        default_fun,
    })
}

fn definition_in(ctx: &mut Ctx, name: &str, env: SexpId) -> EvalResult<SexpId> {
    let sym = ctx.session_mut().intern(name)?;
    let fun = ctx.get_var(sym, env)?.unwrap_or(SEXP_NIL);
    if ctx.session().type_of(fun)? == SexpType::Clo && attr(ctx, fun, "generic")? != SEXP_NIL {
        Ok(fun)
    } else {
        Ok(SEXP_NIL)
    }
}

pub(super) fn generic_definition(
    ctx: &mut Ctx,
    info: &GenericInfo,
    env: SexpId,
) -> EvalResult<SexpId> {
    let fun = attr(ctx, info.table, "generic")?;
    if fun != SEXP_NIL {
        return Ok(fun);
    }
    let fun = definition_in(ctx, &info.name, env)?;
    if fun != SEXP_NIL || super::invoke::primitive_formals(&info.name).is_none() {
        return Ok(fun);
    }
    let ns = ctx.ns_lookup("methods").unwrap_or(GLOBAL_ENV);
    match lookup(ctx, &info.name, &info.package, ns)? {
        Some(prototype) => attr(ctx, prototype.table, "generic"),
        None => Ok(SEXP_NIL),
    }
}

pub(super) fn find_table(
    ctx: &mut Ctx,
    name: &str,
    package: &str,
    where_: SexpId,
) -> EvalResult<Option<GenericInfo>> {
    for env in environment_chain(ctx, where_)? {
        if let Some(info) = local_tables(ctx, name, package, env)?.into_iter().next() {
            return Ok(Some(info));
        }
    }
    Ok(None)
}

/// List the environments from the current scope to the end of its parent chain.
pub(crate) fn environment_chain(ctx: &Ctx, mut env: SexpId) -> EvalResult<Vec<SexpId>> {
    let mut seen = HashSet::new();
    let mut chain = Vec::new();
    while env != EMPTY_ENV && env != SEXP_NIL && seen.insert(env) {
        chain.push(env);
        env = ctx.session().env_enclos(env)?;
    }
    Ok(chain)
}

pub(super) fn local_tables(
    ctx: &mut Ctx,
    name: &str,
    package: &str,
    env: SexpId,
) -> EvalResult<Vec<GenericInfo>> {
    if !package.is_empty() {
        let sym = ctx.session_mut().intern(&table_name(name, package))?;
        return match ctx.get_local(sym, env)? {
            Some(table) if ctx.session().type_of(table)? == SexpType::Env => {
                Ok(vec![canonical(ctx, table, name, package, env)?])
            }
            _ => Ok(Vec::new()),
        };
    }
    let prefix = format!(".__T__{name}:");
    let mut tables = Vec::new();
    for (sym, _) in ctx.session().env_entries(env)? {
        let Some(pkg) = ctx.session().sym_name(sym)?.strip_prefix(&prefix) else {
            continue;
        };
        let pkg = pkg.to_owned();
        if let Some(table) = ctx.get_local(sym, env)?
            && ctx.session().type_of(table)? == SexpType::Env
        {
            tables.push(canonical(ctx, table, name, &pkg, env)?);
        }
    }
    Ok(tables)
}

pub(super) fn dispatch_tables(
    ctx: &mut Ctx,
    info: &GenericInfo,
    env: SexpId,
) -> EvalResult<Vec<GenericInfo>> {
    let mut environments = environment_chain(ctx, env)?;
    environments.extend(ctx.ns_records().iter().rev().map(|rec| rec.ns));
    let mut sources = Vec::new();
    let mut seen = HashSet::new();
    for env in environments {
        for table in local_tables(ctx, &info.name, &info.package, env)? {
            if seen.insert(table.table) {
                sources.push(table);
            }
        }
    }
    if seen.insert(info.table) {
        sources.push(info.clone());
    }
    Ok(sources)
}

#[derive(Eq, Hash, PartialEq)]
struct MergeKey {
    table: SexpId,
    sources: Vec<SexpId>,
    include_primitive_default: bool,
}

#[derive(Default)]
struct MergedTables {
    epoch: u64,
    tables: HashMap<MergeKey, SexpId>,
}

impl Trace for MergedTables {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        for &table in self.tables.values() {
            tracer.edge(table);
        }
    }
}

impl CtxExt for MergedTables {}

pub(super) fn canonical_generic_name(name: &str) -> &str {
    match name {
        "as.double" => "as.numeric",
        _ => name,
    }
}

pub(super) fn merge_tables(
    ctx: &mut Ctx,
    mut info: GenericInfo,
    sources: &[GenericInfo],
    include_primitive_default: bool,
) -> EvalResult<GenericInfo> {
    if let [only] = sources
        && (include_primitive_default || only.table == info.table)
    {
        return Ok(only.clone());
    }
    let epoch = ctx.ext::<MethodsExt>().epoch;
    let key = MergeKey {
        table: info.table,
        sources: sources.iter().map(|source| source.table).collect(),
        include_primitive_default,
    };
    let cache = ctx.ext::<MergedTables>();
    if cache.epoch != epoch {
        cache.epoch = epoch;
        cache.tables.clear();
    }
    if let Some(&table) = cache.tables.get(&key) {
        info.table = table;
        return Ok(info);
    }
    // Earlier tables win when they define the same signature. Rank distinct signatures together.
    let mut entries = BTreeMap::new();
    for source in sources {
        for method in methods(ctx, source)? {
            if !include_primitive_default && method.fun == source.default_fun {
                continue;
            }
            let mut signature = method.signature;
            signature.resize(info.signature.len(), ANY.to_owned());
            entries.entry(signature).or_insert(method.fun);
        }
    }
    let width = entries.keys().map(|sig| sig_width(sig)).max().unwrap_or(1);
    let table = ctx.session_mut().alloc_env(EMPTY_ENV)?;
    ctx.protect(table);
    for (sym, value) in ctx.session().attrs(info.table)? {
        ctx.session_mut().set_attrib(table, sym, value)?;
    }
    for (signature, fun) in entries {
        bind(ctx, table, &method_key(&signature, width), fun)?;
    }
    info.table = table;
    if ctx.ext::<MethodsExt>().epoch == epoch {
        ctx.ext::<MergedTables>().tables.insert(key, table);
    }
    Ok(info)
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
        return canonical(ctx, table, name, pkg, ns).map(Some);
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
    let group = if primitive {
        super::groups::group_names(ctx, name)?
    } else {
        group
    };
    let members = super::groups::members(name).filter(|_| primitive);
    set_class(
        ctx,
        fun,
        if members.is_some() {
            "groupGenericFunction"
        } else if ctx.session().type_of(body)? == SexpType::Lang
            && ctx.session().cons_parts(body)?.0 != standard.as_sexp()
        {
            "nonstandardGenericFunction"
        } else {
            "standardGeneric"
        },
    )?;
    if let Some(members) = members {
        let members = strings(
            ctx,
            &members
                .iter()
                .map(|member| (*member).to_owned())
                .collect::<Vec<_>>(),
        )?;
        set_attr(ctx, fun, "groupMembers", members)?;
    }
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
        Some(info) => generic_definition(ctx, &info, args.get(1)),
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

pub(super) fn lookup_or_create_generic(
    ctx: &mut Ctx,
    name: &str,
    where_: SexpId,
) -> EvalResult<GenericInfo> {
    match lookup(ctx, name, "", where_)? {
        Some(info) => Ok(info),
        None => {
            let pkg = ctx
                .namespace_name(where_)
                .unwrap_or_else(|| ".GlobalEnv".to_owned());
            let spec = GenericSpec {
                name,
                package: &pkg,
                where_,
                def: SEXP_NIL,
                signature: SEXP_NIL,
                default: SEXP_NIL,
                group: SEXP_NIL,
                value_class: SEXP_NIL,
            };
            create(ctx, &spec, false)
        }
    }
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
    canonical(ctx, table, name, &package, GLOBAL_ENV).map(Some)
}

pub(super) fn select_method(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let name = text(ctx, args.get(0))?;
    let name = canonical_generic_name(&name);
    let optional = flag(ctx, args.get(3));
    let info = match by_definition(ctx, args.get(5), name)? {
        Some(info) => Some(info),
        None => lookup(ctx, name, "", args.get(2))?,
    };
    if let Some(mut info) = info {
        let inherited = flag(ctx, args.get(4));
        if ctx.session().type_of(args.get(7))? == SexpType::Env {
            info.table = args.get(7);
        } else if inherited {
            let sources = super::invoke::primitive_sources(ctx, &info, args.get(2))?;
            info = merge_tables(ctx, info, &sources, true)?;
        }
        let signature = normalize(ctx, &info, args.get(1))?;
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
    entries.sort_by_cached_key(|entry| {
        entry
            .0
            .iter()
            .map(|name| name.to_lowercase())
            .collect::<Vec<_>>()
    });
    entries.dedup_by(|a, b| a.0 == b.0);
    Ok(MethodListing { entries, width })
}

pub(super) fn find_methods(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let name = text(ctx, args.get(0))?;
    let package = texts(ctx, args.get(4))?
        .into_iter()
        .next()
        .unwrap_or_default();
    crate::meta::clear_unloaded(ctx);
    let where_ = args.get(1);
    let scope = if where_ == SEXP_NIL {
        args.get(5)
    } else {
        where_
    };
    let Some(mut info) = lookup(ctx, &name, &package, scope)? else {
        ctx.warn_now(
            call.err_call(),
            format!("non-generic function '{name}' given to findMethods()"),
        )?;
        return Ok(ctx.session_mut().alloc_list(0)?);
    };
    let sources = if where_ == SEXP_NIL {
        dispatch_tables(ctx, &info, scope)?
    } else {
        local_tables(ctx, &name, &info.package, where_)?
    };
    info = merge_tables(ctx, info, &sources, true)?;
    let inherited = flag(ctx, args.get(3));
    let MethodListing { mut entries, width } = listing(ctx, &info, args.get(2), inherited)?;
    if where_ != SEXP_NIL && super::invoke::primitive_formals(&name).is_some() {
        entries.retain(|(_, method)| method.fun != info.default_fun);
    }
    let list = ctx.session_mut().alloc_list(entries.len())?;
    let signatures = ctx.session_mut().alloc_list(entries.len())?;
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
        let signature = strings(ctx, &target[..width.min(target.len())])?;
        ctx.session_mut().list_set(signatures, i, signature)?;
    }
    let names = strings(ctx, &names)?;
    let arguments = strings(ctx, &info.signature)?;
    let generic = generic_definition(ctx, &info, scope)?;
    set_attr(ctx, list, "names", names)?;
    set_attr(ctx, list, "arguments", arguments)?;
    set_attr(ctx, list, "signatures", signatures)?;
    set_attr(ctx, list, "generic", generic)?;
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
