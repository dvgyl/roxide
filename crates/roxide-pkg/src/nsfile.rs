//! Parse NAMESPACE expressions and interpret the selected directives.

use std::path::Path;

use roxide_diag::SrcFileId;
use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_syntax::{Arg, ArgName, Ast, Node, NodeId, Parser};
use roxide_value::lower::{KeepSource, lower};
use roxide_value::{BASE_ENV, DeparseOpts, EMPTY_ENV, FALSE_SCALAR, TRUE_SCALAR, ValueApi};

use crate::description::read_text;
use crate::loader::values::*;

type Mappings = Vec<(String, String)>;

#[derive(Clone, Debug)]
pub(crate) enum Selection {
    All(Vec<String>),
    Names(Mappings),
}

#[derive(Clone, Debug)]
pub(crate) struct Import {
    pub(crate) package: String,
    pub(crate) selection: Selection,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct S3Method {
    pub(crate) generic: String,
    pub(crate) class: String,
    pub(crate) method: Option<String>,
    pub(crate) provider: Option<String>,
}

#[derive(Clone, Debug)]
struct DynLib {
    name: String,
    alias: String,
    symbols: Mappings,
    registration: bool,
    fixes: [String; 2],
}

/// Parsed directives from a NAMESPACE file.
#[derive(Clone, Debug, Default)]
pub struct NsSpec {
    /// Explicit exported object names.
    pub exports: Vec<String>,
    /// Patterns for exported object names.
    pub export_patterns: Vec<String>,
    export_tags: Vec<String>,
    pub(crate) imports: Vec<Import>,
    pub(crate) import_classes: Vec<Import>,
    pub(crate) import_methods: Vec<Import>,
    pub(crate) export_classes: Vec<String>,
    pub(crate) export_class_patterns: Vec<String>,
    pub(crate) export_methods: Vec<String>,
    pub(crate) s3: Vec<S3Method>,
    dynlibs: Vec<DynLib>,
}

impl NsSpec {
    /// Read and interpret a NAMESPACE file through the session host.
    pub fn read(ctx: &mut Ctx, path: &Path) -> EvalResult<Self> {
        let text = read_text(ctx, path)?;
        Self::parse(ctx, &text)
    }

    /// Parse NAMESPACE text and evaluate platform and version conditions.
    pub fn parse(ctx: &mut Ctx, source: &str) -> EvalResult<Self> {
        let mut parser = Parser::new(source.trim_start_matches('\u{feff}'), SrcFileId(0));
        let expressions = parser
            .parse_all()
            .map_err(|error| ctx.stop(roxide_value::SEXP_NIL, error.to_string()))?;
        ctx.scoped(|ctx| {
            let mut spec = Self::default();
            let env = ctx.session_mut().alloc_env(EMPTY_ENV)?;
            ctx.protect(env);
            for node in expressions {
                spec.directive(ctx, parser.ast(), node, env, 0)?;
            }
            Ok(spec)
        })
    }

    fn directive(
        &mut self,
        ctx: &mut Ctx,
        ast: &Ast,
        node: NodeId,
        env: SexpId,
        depth: usize,
    ) -> EvalResult<()> {
        if depth >= 128 {
            return Err(ctx.stop(
                roxide_value::SEXP_NIL,
                "namespace directive is too deeply nested",
            ));
        }
        let Some((op, args)) = call(ast, node) else {
            let text = node_name(ctx, ast, node)?;
            return Err(ctx.stop(
                roxide_value::SEXP_NIL,
                format!("unknown namespace directive: {text}"),
            ));
        };
        match op {
            "{" => {
                for arg in args {
                    let node = arg_node(ctx, arg)?;
                    self.directive(ctx, ast, node, env, depth + 1)?;
                }
            }
            "if" => {
                let [test, yes, rest @ ..] = args else {
                    return Err(ctx.stop(roxide_value::SEXP_NIL, "invalid namespace condition"));
                };
                let test = arg_node(ctx, test)?;
                let value = eval_condition(ctx, ast, test, env)?;
                let invalid_length = match ctx.session().xlength(value)? {
                    0 => Some("argument is of length zero"),
                    1 => None,
                    _ => Some("the condition has length > 1"),
                };
                if let Some(message) = invalid_length {
                    return Err(ctx.stop(roxide_value::SEXP_NIL, message));
                }
                let branch = if flag(ctx, value)? {
                    Some(yes)
                } else {
                    rest.first()
                };
                if let Some(branch) = branch {
                    let node = arg_node(ctx, branch)?;
                    self.directive(ctx, ast, node, env, depth + 1)?;
                }
            }
            "export" => {
                self.exports.extend(arg_names(ctx, ast, args)?);
                self.export_tags
                    .extend(args.iter().map(|arg| tag(arg).to_owned()));
            }
            "exportPattern" => self.export_patterns.extend(arg_names(ctx, ast, args)?),
            "exportClasses" => self.export_classes.extend(arg_names(ctx, ast, args)?),
            "exportClassPattern" => self
                .export_class_patterns
                .extend(arg_names(ctx, ast, args)?),
            "exportMethods" => self.export_methods.extend(arg_names(ctx, ast, args)?),
            "import" => {
                let mut packages = Vec::new();
                let mut except = Vec::new();
                for arg in args {
                    let node = arg_node(ctx, arg)?;
                    if tag(arg) == "except" {
                        except = name_vector(ctx, ast, node)?;
                    } else {
                        packages.push(node_name(ctx, ast, node)?);
                    }
                }
                self.imports
                    .extend(packages.into_iter().map(|package| Import {
                        package,
                        selection: Selection::All(except.clone()),
                    }));
            }
            "importFrom" | "importClassesFrom" | "importMethodsFrom" => {
                let Some((package, items)) = args.split_first() else {
                    return Err(ctx.stop(
                        roxide_value::SEXP_NIL,
                        "missing package in namespace directive",
                    ));
                };
                let package = arg_name(ctx, ast, package)?;
                let names = arg_names(ctx, ast, items)?;
                let selection = Selection::Names(
                    items
                        .iter()
                        .zip(names)
                        .map(|(arg, name)| (tag(arg).to_owned(), name))
                        .collect(),
                );
                let target = match op {
                    "importClassesFrom" => &mut self.import_classes,
                    "importMethodsFrom" => &mut self.import_methods,
                    _ => &mut self.imports,
                };
                target.push(Import { package, selection });
            }
            "S3method" => {
                let [generic, class, method @ ..] = args else {
                    return Err(ctx.stop(roxide_value::SEXP_NIL, "bad 'S3method' directive"));
                };
                if method.len() > 1 {
                    return Err(ctx.stop(roxide_value::SEXP_NIL, "bad 'S3method' directive"));
                }
                let generic = arg_node(ctx, generic)?;
                let (provider, generic) = match call(ast, generic) {
                    Some(("::", [provider, generic])) => (
                        Some(arg_name(ctx, ast, provider)?),
                        arg_name(ctx, ast, generic)?,
                    ),
                    _ => (None, node_name(ctx, ast, generic)?),
                };
                let class = arg_name(ctx, ast, class)?;
                let method = method.first().map(|m| arg_name(ctx, ast, m)).transpose()?;
                let entry = S3Method {
                    generic,
                    class,
                    method,
                    provider,
                };
                if !self.s3.contains(&entry) {
                    self.s3.push(entry);
                }
            }
            "useDynLib" => self.dynlibs.push(dynlib(ctx, ast, args, env)?),
            _ => {
                let directive = node_name(ctx, ast, node)?;
                return Err(ctx.stop(
                    roxide_value::SEXP_NIL,
                    format!("unknown namespace directive: {directive}"),
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn s3_matrix(&self, ctx: &mut Ctx) -> EvalResult<SexpId> {
        let rows = &self.s3;
        let value = optional_strings(
            ctx,
            rows.iter()
                .map(|m| Some(m.generic.as_str()))
                .chain(rows.iter().map(|m| Some(m.class.as_str())))
                .chain(rows.iter().map(|m| m.method.as_deref()))
                .chain(rows.iter().map(|m| m.provider.as_deref())),
        )?;
        let count = i32::try_from(rows.len())
            .map_err(|_| ctx.stop(roxide_value::SEXP_NIL, "too many S3 methods"))?;
        let dim = ctx.session_mut().alloc_int_from(&[count, 4])?;
        set_attr(ctx, value, "dim", dim)?;
        Ok(value)
    }

    pub(crate) fn dynlib_names(&self, ctx: &mut Ctx) -> EvalResult<SexpId> {
        let result = strings(ctx, self.dynlibs.iter().map(|lib| lib.name.as_str()))?;
        if !self.dynlibs.is_empty() {
            named(
                ctx,
                result,
                &self
                    .dynlibs
                    .iter()
                    .map(|lib| lib.alias.as_str())
                    .collect::<Vec<_>>(),
            )?;
        }
        Ok(result)
    }

    fn as_list(&self, ctx: &mut Ctx) -> EvalResult<SexpId> {
        let imports = imports_list(ctx, &self.imports)?;
        let exports = mappings_value(
            ctx,
            self.export_tags
                .iter()
                .zip(&self.exports)
                .map(|(tag, value)| (tag.as_str(), value.as_str())),
        )?;
        let export_patterns = strings(ctx, self.export_patterns.iter().map(String::as_str))?;
        let import_classes = imports_list(ctx, &self.import_classes)?;
        let import_methods = imports_list(ctx, &self.import_methods)?;
        let export_classes = strings(ctx, self.export_classes.iter().map(String::as_str))?;
        let export_methods = strings(ctx, self.export_methods.iter().map(String::as_str))?;
        let class_patterns = strings(ctx, self.export_class_patterns.iter().map(String::as_str))?;
        let dynlibs = self.dynlib_names(ctx)?;
        let native_routines = self.native_routines(ctx)?;
        let s3 = self.s3_matrix(ctx)?;
        let result = list(
            ctx,
            &[
                imports,
                exports,
                export_patterns,
                import_classes,
                import_methods,
                export_classes,
                export_methods,
                class_patterns,
                dynlibs,
                native_routines,
                s3,
            ],
        )?;
        named(
            ctx,
            result,
            &[
                "imports",
                "exports",
                "exportPatterns",
                "importClasses",
                "importMethods",
                "exportClasses",
                "exportMethods",
                "exportClassPatterns",
                "dynlibs",
                "nativeRoutines",
                "S3methods",
            ],
        )
    }

    pub(crate) fn native_routines(&self, ctx: &mut Ctx) -> EvalResult<SexpId> {
        let mut keys = Vec::new();
        let mut routines = Vec::new();
        for lib in &self.dynlibs {
            if lib.symbols.is_empty() && !lib.registration {
                continue;
            }
            let symbols = mappings_value(
                ctx,
                lib.symbols
                    .iter()
                    .map(|(tag, name)| (tag.as_str(), name.as_str())),
            )?;
            let mut elements = vec![
                if lib.registration {
                    TRUE_SCALAR
                } else {
                    FALSE_SCALAR
                },
                symbols,
            ];
            let mut fields = vec!["useRegistration", "symbolNames"];
            if lib.registration {
                elements.push(strings(ctx, lib.fixes.iter().map(String::as_str))?);
                fields.push("registrationFixes");
            }
            let map = list(ctx, &elements)?;
            named(ctx, map, &fields)?;
            let class = ctx.session_mut().scalar_str("NativeRoutineMap")?;
            set_attr(ctx, map, "class", class)?;
            keys.push(lib.name.as_str());
            routines.push(map);
        }
        let result = list(ctx, &routines)?;
        if !keys.is_empty() {
            named(ctx, result, &keys)?;
        }
        Ok(result)
    }
}

fn call(ast: &Ast, node: NodeId) -> Option<(&str, &[Arg])> {
    let Node::Call { func, args } = ast.node(node) else {
        return None;
    };
    let Node::Sym(name) = ast.node(*func) else {
        return None;
    };
    Some((name, ast.args(args.clone())))
}

fn tag(arg: &Arg) -> &str {
    match &arg.name {
        Some(ArgName::Sym(name) | ArgName::Str(name)) => name,
        Some(ArgName::Null) => "NULL",
        None => "",
    }
}

fn arg_node(ctx: &mut Ctx, arg: &Arg) -> EvalResult<NodeId> {
    arg.value.ok_or_else(|| {
        ctx.stop(
            roxide_value::SEXP_NIL,
            "missing namespace directive argument",
        )
    })
}

fn node_name(ctx: &mut Ctx, ast: &Ast, node: NodeId) -> EvalResult<String> {
    match ast.node(node) {
        Node::Sym(name) => Ok(name.to_string()),
        Node::Const(roxide_syntax::Const::Str { bytes, .. }) => {
            Ok(String::from_utf8_lossy(bytes).into_owned())
        }
        _ => {
            let expr = lower(ast, node, ctx.session_mut(), KeepSource::No)?;
            Ok(ctx.deparse_lines(expr, DeparseOpts::default())?.join(" "))
        }
    }
}

fn arg_name(ctx: &mut Ctx, ast: &Ast, arg: &Arg) -> EvalResult<String> {
    let node = arg_node(ctx, arg)?;
    node_name(ctx, ast, node)
}

fn arg_names(ctx: &mut Ctx, ast: &Ast, args: &[Arg]) -> EvalResult<Vec<String>> {
    args.iter().map(|arg| arg_name(ctx, ast, arg)).collect()
}

fn name_vector(ctx: &mut Ctx, ast: &Ast, node: NodeId) -> EvalResult<Vec<String>> {
    if let Some(("c", args)) = call(ast, node) {
        arg_names(ctx, ast, args)
    } else {
        Ok(vec![node_name(ctx, ast, node)?])
    }
}

fn eval_condition(ctx: &mut Ctx, ast: &Ast, node: NodeId, env: SexpId) -> EvalResult<SexpId> {
    fn bind(ctx: &mut Ctx, ast: &Ast, node: NodeId, env: SexpId, depth: usize) -> EvalResult<()> {
        if depth >= 128 {
            return Err(ctx.stop(
                roxide_value::SEXP_NIL,
                "namespace condition is too deeply nested",
            ));
        }
        match ast.node(node) {
            Node::Const(_) => return Ok(()),
            Node::Sym(name)
                if matches!(
                    name.as_ref(),
                    ".Platform" | "R.version" | "R.version.string"
                ) =>
            {
                if let Some(value) = local(ctx, BASE_ENV, name)? {
                    return define(ctx, env, name, value);
                }
            }
            Node::Call { .. } => {
                if let Some((op, args)) = call(ast, node)
                    && matches!(
                        op,
                        "(" | "!"
                            | "&"
                            | "&&"
                            | "|"
                            | "||"
                            | "=="
                            | "!="
                            | "<"
                            | "<="
                            | ">"
                            | ">="
                            | "%in%"
                            | "["
                            | "[["
                            | "$"
                            | "c"
                            | "getRversion"
                            | "package_version"
                            | "numeric_version"
                            | "Sys.info"
                            | "grepl"
                            | "isTRUE"
                            | "isFALSE"
                            | "identical"
                            | "nzchar"
                    )
                {
                    let binding = if op == "%in%" { ".rx_namespace_in" } else { op };
                    let key = ctx.session_mut().intern(binding)?;
                    let fun = ctx.find_fun(key, BASE_ENV)?;
                    define(ctx, env, op, fun)?;
                    for (i, arg) in args.iter().enumerate() {
                        let item = arg_node(ctx, arg)?;
                        if op == "$" && i == 1 && matches!(ast.node(item), Node::Sym(_)) {
                            continue;
                        }
                        bind(ctx, ast, item, env, depth + 1)?;
                    }
                    return Ok(());
                }
            }
            _ => {}
        }
        Err(ctx.stop(
            roxide_value::SEXP_NIL,
            "unsupported expression in namespace condition",
        ))
    }
    bind(ctx, ast, node, env, 0)?;
    let expr = lower(ast, node, ctx.session_mut(), KeepSource::No)?;
    ctx.eval(expr, env)
}

fn dynlib(ctx: &mut Ctx, ast: &Ast, args: &[Arg], env: SexpId) -> EvalResult<DynLib> {
    let Some((lib, args)) = args.split_first() else {
        return Err(ctx.stop(roxide_value::SEXP_NIL, "bad 'useDynLib' directive"));
    };
    let name = arg_name(ctx, ast, lib)?;
    let mut result = DynLib {
        name,
        alias: tag(lib).to_owned(),
        symbols: Vec::new(),
        registration: false,
        fixes: [String::new(), String::new()],
    };
    for arg in args {
        let node = arg_node(ctx, arg)?;
        match tag(arg) {
            ".registration" => {
                let value = eval_condition(ctx, ast, node, env)?;
                result.registration = flag(ctx, value)?;
            }
            ".fixes" => {
                let value = eval_condition(ctx, ast, node, env)?;
                let items = chars(ctx, value)?;
                if !(1..=2).contains(&items.len()) {
                    return Err(ctx.stop(roxide_value::SEXP_NIL, "invalid '.fixes' in 'useDynLib'"));
                }
                for (target, item) in result.fixes.iter_mut().zip(items) {
                    *target = item;
                }
            }
            alias => {
                let name = node_name(ctx, ast, node)?;
                let binding = if alias.is_empty() {
                    name.clone()
                } else {
                    alias.to_owned()
                };
                result.symbols.push((binding, name));
            }
        }
    }
    if !result.registration {
        if result.symbols.is_empty() && result.fixes.iter().any(|fix| !fix.is_empty()) {
            return Err(ctx.stop(
                roxide_value::SEXP_NIL,
                "'names' attribute [1] must be the same length as the vector [0]",
            ));
        }
        for (alias, _) in &mut result.symbols {
            *alias = format!("{}{alias}{}", result.fixes[0], result.fixes[1]);
        }
    }
    Ok(result)
}

fn mappings_value<'a>(
    ctx: &mut Ctx,
    mappings: impl Iterator<Item = (&'a str, &'a str)>,
) -> EvalResult<SexpId> {
    let (keys, values): (Vec<_>, Vec<_>) = mappings.unzip();
    let value = strings(ctx, values)?;
    if keys.iter().any(|key| !key.is_empty()) {
        named(ctx, value, &keys)?;
    }
    Ok(value)
}

fn imports_list(ctx: &mut Ctx, imports: &[Import]) -> EvalResult<SexpId> {
    let mut result = Vec::with_capacity(imports.len());
    for import in imports {
        let package = ctx.session_mut().scalar_str(&import.package)?;
        let item = match &import.selection {
            Selection::All(except) if except.is_empty() => package,
            Selection::All(except) => {
                let except = strings(ctx, except.iter().map(String::as_str))?;
                let item = list(ctx, &[package, except])?;
                named(ctx, item, &["", "except"])?
            }
            Selection::Names(mappings) => {
                let selected = mappings_value(
                    ctx,
                    mappings
                        .iter()
                        .map(|(tag, name)| (tag.as_str(), name.as_str())),
                )?;
                list(ctx, &[package, selected])?
            }
        };
        result.push(item);
    }
    list(ctx, &result)
}

pub(crate) fn parse_namespace_file(
    ctx: &mut Ctx,
    call: &Call,
    args: &mut Args,
) -> EvalResult<SexpId> {
    let package = string(ctx, args.get(0))?;
    if !crate::description::valid_package(&package) {
        return Err(ctx.stop(call.err_call(), "invalid package name"));
    }
    let library = string(ctx, args.get(1))?;
    let path = Path::new(&library).join(&package).join("NAMESPACE");
    let spec = if ctx.host().metadata(&path, true).is_ok() {
        NsSpec::read(ctx, &path)?
    } else if flag(ctx, args.get(2))? {
        return Err(ctx.stop(
            call.err_call(),
            format!("package ‘{package}’ does not have a namespace"),
        ));
    } else {
        NsSpec::default()
    };
    spec.as_list(ctx)
}

impl NsSpec {
    pub(crate) fn from_cache(ctx: &mut Ctx, path: &Path) -> EvalResult<Self> {
        ctx.scoped(|ctx| {
            let path = ctx.session_mut().scalar_str(&path.to_string_lossy())?;
            ctx.protect(path);
            let cache = base_call(ctx, "readRDS", &[path])?;
            ctx.protect(cache);
            let (export_tags, exports): (Vec<_>, Vec<_>) =
                cache_mappings(ctx, cache, "exports")?.into_iter().unzip();
            let methods = field(ctx, cache, "S3methods")?.unwrap_or(roxide_value::SEXP_NIL);
            let mut s3 = Vec::new();
            if methods != roxide_value::SEXP_NIL {
                let columns = names(ctx, methods)?;
                if columns.len() % 4 != 0 {
                    return Err(ctx.stop(roxide_value::SEXP_NIL, "invalid cached S3methods matrix"));
                }
                let count = columns.len() / 4;
                for row in 0..count {
                    let (Some(generic), Some(class)) = (&columns[row], &columns[row + count])
                    else {
                        return Err(
                            ctx.stop(roxide_value::SEXP_NIL, "invalid cached S3methods entry")
                        );
                    };
                    s3.push(S3Method {
                        generic: generic.clone(),
                        class: class.clone(),
                        method: columns[row + 2 * count].clone(),
                        provider: columns[row + 3 * count].clone(),
                    });
                }
            }
            let libs = field(ctx, cache, "dynlibs")?.unwrap_or(roxide_value::SEXP_NIL);
            let natives = field(ctx, cache, "nativeRoutines")?.unwrap_or(roxide_value::SEXP_NIL);
            let mut dynlibs = Vec::new();
            for (alias, name) in value_mappings(ctx, libs)? {
                let mut lib = DynLib {
                    name: name.clone(),
                    alias,
                    symbols: Vec::new(),
                    registration: false,
                    fixes: [String::new(), String::new()],
                };
                if let Some(map) = field(ctx, natives, &name)? {
                    if let Some(value) = field(ctx, map, "useRegistration")? {
                        lib.registration = flag(ctx, value)?;
                    }
                    lib.symbols = cache_mappings(ctx, map, "symbolNames")?;
                    if let Some(value) = field(ctx, map, "registrationFixes")? {
                        for (fix, part) in lib.fixes.iter_mut().zip(chars(ctx, value)?) {
                            *fix = part;
                        }
                    }
                }
                dynlibs.push(lib);
            }
            Ok(Self {
                exports,
                export_tags,
                s3,
                dynlibs,
                imports: cache_imports(ctx, cache, "imports")?,
                import_classes: cache_imports(ctx, cache, "importClasses")?,
                import_methods: cache_imports(ctx, cache, "importMethods")?,
                export_patterns: cache_strings(ctx, cache, "exportPatterns")?,
                export_classes: cache_strings(ctx, cache, "exportClasses")?,
                export_class_patterns: cache_strings(ctx, cache, "exportClassPatterns")?,
                export_methods: cache_strings(ctx, cache, "exportMethods")?,
            })
        })
    }
}

fn cache_strings(ctx: &mut Ctx, cache: SexpId, key: &str) -> EvalResult<Vec<String>> {
    let value = field(ctx, cache, key)?.unwrap_or(roxide_value::SEXP_NIL);
    chars(ctx, value)
}

fn cache_mappings(ctx: &mut Ctx, cache: SexpId, key: &str) -> EvalResult<Mappings> {
    let value = field(ctx, cache, key)?.unwrap_or(roxide_value::SEXP_NIL);
    value_mappings(ctx, value)
}

fn value_mappings(ctx: &mut Ctx, value: SexpId) -> EvalResult<Mappings> {
    let values = chars(ctx, value)?;
    let labels = attr(ctx, value, "names")?;
    let labels = chars(ctx, labels)?;
    Ok(values
        .into_iter()
        .enumerate()
        .map(|(i, name)| (labels.get(i).cloned().unwrap_or_default(), name))
        .collect())
}

fn cache_imports(ctx: &mut Ctx, cache: SexpId, key: &str) -> EvalResult<Vec<Import>> {
    let Some(imports) = field(ctx, cache, key)? else {
        return Ok(Vec::new());
    };
    let entries = ctx.session().list_slice(imports)?.to_vec();
    entries
        .into_iter()
        .map(|entry| {
            let (package, selection) =
                if ctx.session().type_of(entry)? == roxide_value::SexpType::Vec {
                    let package = ctx.session().list_get(entry, 0)?;
                    let items = ctx.session().list_get(entry, 1)?;
                    let selection = if field(ctx, entry, "except")?.is_some() {
                        Selection::All(chars(ctx, items)?)
                    } else {
                        Selection::Names(value_mappings(ctx, items)?)
                    };
                    (package, selection)
                } else {
                    (entry, Selection::All(Vec::new()))
                };
            Ok(Import {
                package: string(ctx, package)?,
                selection,
            })
        })
        .collect()
}
