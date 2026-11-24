//! Test the native ABI stamp and namespace loader.

use std::path::Path;

use roxide_eval::{
    Ctx, CtxExt, EvalResult, Unwind,
    api::{CAPI_ABI_VERSION, NativeHooks},
};
use roxide_gc::{GcPolicy, SexpId, Trace, Tracer};
use roxide_host::{Host, PureHost};
use roxide_value::ValueApi;

const ROOT: &str = "/library/nativepkg";

const ON_LOAD: &str = r#"
.onLoad <- function(lib, pkg) {
    stopifnot(identical(C_registered_end$name, "registered"))
    stopifnot(identical(explicit$name, "lookup"))
    stopifnot(identical(getNamespaceInfo(pkg, "DLLs")$nativepkg$path,
                        "/library/nativepkg/libs/nativepkg.so"))
}
"#;

fn context(namespace: &str, stamp: Option<&str>, code: &str) -> Result<Ctx, Unwind> {
    let mut host = PureHost::new();
    for dir in ["/library", "/library/nativepkg/Meta"] {
        host.create_dir(Path::new(dir), true, 0o755)
            .map_err(|error| {
                Unwind::Internal(roxide_eval::InternalError {
                    message: error.to_string(),
                })
            })?;
    }
    for (path, data) in [
        ("DESCRIPTION", "Package: nativepkg\nVersion: 1.0\n"),
        ("NAMESPACE", namespace),
        ("libs/nativepkg.so", ""),
        ("libs/native.pkg.so", ""),
        ("libs/other.so", ""),
        ("other/native_pkg.so", ""),
        ("R/source.R", code),
    ] {
        host.insert_file(Path::new(ROOT).join(path), data.as_bytes());
    }
    if stamp == Some("bad data") {
        host.insert_file(Path::new(ROOT).join("Meta/Roxide.rds"), b"bad data");
    }
    let mut ctx = Ctx::new(Box::new(host), GcPolicy::Torture).map_err(Unwind::Internal)?;
    roxide_base::register_all(&mut ctx).map_err(Unwind::Internal)?;
    roxide_pkg::register(&mut ctx).map_err(Unwind::Internal)?;
    ctx.set_native_hooks(&Hooks);
    if let Some(value) = stamp.filter(|stamp| *stamp != "bad data") {
        ctx.run_global(
            "stamp",
            &format!("saveRDS({value}, '{ROOT}/Meta/Roxide.rds', compress = FALSE)"),
        )?;
    }
    ctx.run_global("library", ".libPaths('/library')")?;
    Ok(ctx)
}

fn valid_stamp() -> String {
    let revision = CAPI_ABI_VERSION
        .rsplit_once("-v")
        .map_or("NA_", |(_, revision)| revision);
    format!("list(capi_abi = {revision}L)")
}

#[test]
fn rejects_foreign_stamps_before_loading_a_dll() -> EvalResult<()> {
    for stamp in [
        None,
        Some("bad data"),
        Some("1L"),
        Some("list(capi_abi = 999L)"),
        Some("list(capi_abi = 1.0)"),
    ] {
        let mut ctx = context("useDynLib(nativepkg)", stamp, ON_LOAD)?;
        ctx.run_global("load", r#"
message <- tryCatch(loadNamespace("nativepkg"), error = conditionMessage)
stopifnot(identical(message, "package 'nativepkg' was installed by a different R implementation; reinstall it from source with Roxide"))
"#)?;
        assert!(ctx.ext::<Loaded>().0.is_empty());
    }
    Ok(())
}

#[test]
fn binds_symbols_before_on_load_and_unloads_the_dll() -> EvalResult<()> {
    let mut ctx = context(
        "useDynLib(nativepkg, explicit = lookup, .registration = TRUE, .fixes = c('C_', '_end'))",
        Some(&valid_stamp()),
        ON_LOAD,
    )?;
    ctx.run_global("load", "loadNamespace('nativepkg')")?;
    assert_eq!(
        ctx.ext::<Loaded>()
            .0
            .iter()
            .map(|(path, _)| path)
            .collect::<Vec<_>>(),
        ["/library/nativepkg/libs/nativepkg.so"]
    );
    ctx.run_global("unload", "unloadNamespace('nativepkg')")?;
    assert!(ctx.ext::<Loaded>().0.is_empty());
    assert!(ctx.ns_lookup("nativepkg").is_none());
    Ok(())
}

#[test]
fn binds_repeated_use_dynlib_directives_from_source_and_cache() -> EvalResult<()> {
    let namespace = r#"
useDynLib(primary = nativepkg, explicit = lookup)
useDynLib(secondary = nativepkg, dynamic, .fixes = "X_")
useDynLib(nativepkg, .registration = TRUE, .fixes = "Old_")
useDynLib(nativepkg, .registration = TRUE, .fixes = c("C_", "_end"))
"#;
    let legacy_cache = r#"
list(dynlibs = c(primary = "nativepkg", secondary = "nativepkg", "nativepkg", "nativepkg"),
     nativeRoutines = list(
         nativepkg = list(useRegistration = FALSE, symbolNames = c(explicit = "lookup")),
         nativepkg = list(useRegistration = FALSE, symbolNames = c(X_dynamic = "dynamic")),
         nativepkg = list(useRegistration = TRUE, registrationFixes = c("Old_", "")),
         nativepkg = list(useRegistration = TRUE, registrationFixes = c("C_", "_end"))))
"#;
    for cache in [None, Some("spec"), Some(legacy_cache)] {
        let mut ctx = context(
            namespace,
            Some(&valid_stamp()),
            r#"
.onLoad <- function(lib, pkg) {
    stopifnot(identical(explicit$name, "lookup"),
              identical(X_dynamic$name, "dynamic"),
              identical(C_registered_end$name, "registered"),
              identical(getNamespaceInfo(pkg, "DLLs"), list(primary = primary)))
}
"#,
        )?;
        ctx.run_global(
            "metadata",
            r#"
spec <- parseNamespaceFile("nativepkg", "/library")
stopifnot(identical(spec$dynlibs, c(primary = "nativepkg")),
          identical(spec$nativeRoutines$nativepkg$symbolNames,
                    c(explicit = "lookup", X_dynamic = "dynamic")))
"#,
        )?;
        if let Some(cache) = cache {
            ctx.run_global(
                "cache",
                &format!(
                    r#"
saveRDS({cache}, "/library/nativepkg/Meta/nsInfo.rds", compress = FALSE)
writeLines("unsupported()", "/library/nativepkg/NAMESPACE")
"#
                ),
            )?;
        }
        ctx.run_global("load", "loadNamespace('nativepkg')")?;
        assert_eq!(ctx.ext::<Loaded>().0.len(), 1);
    }
    Ok(())
}

#[test]
fn rolls_back_the_dll_if_namespace_loading_fails() -> EvalResult<()> {
    let mut ctx = context("useDynLib(nativepkg)", Some(&valid_stamp()), ON_LOAD)?;
    ctx.run_global(
        "load",
        r#"
message <- tryCatch(loadNamespace("nativepkg"), error = conditionMessage)
stopifnot(grepl(".onLoad failed", message, fixed = TRUE))
"#,
    )?;
    assert!(ctx.ext::<Loaded>().0.is_empty());
    assert!(ctx.ns_lookup("nativepkg").is_none());
    Ok(())
}

#[test]
fn rollback_preserves_a_dll_loaded_before_the_namespace() -> EvalResult<()> {
    let mut ctx = context(
        "useDynLib(nativepkg)\nuseDynLib(other)",
        Some(&valid_stamp()),
        ON_LOAD,
    )?;
    ctx.run_global(
        "load",
        r#"
existing <- dyn.load("/library/nativepkg/libs/nativepkg.so")
message <- tryCatch(loadNamespace("nativepkg"), error = conditionMessage)
stopifnot(grepl(".onLoad failed", message, fixed = TRUE))
stopifnot(identical(unclass(getLoadedDLLs()), list(existing)))
"#,
    )?;
    assert!(ctx.ns_lookup("nativepkg").is_none());
    Ok(())
}

#[test]
fn binds_explicit_symbols_from_the_requested_dotted_dll() -> EvalResult<()> {
    let mut ctx = context(
        "useDynLib(native.pkg, explicit = lookup, dynamic)",
        Some(&valid_stamp()),
        r#"
.onLoad <- function(lib, pkg) {
    dll <- getNamespaceInfo(pkg, "DLLs")[["native.pkg"]]
    stopifnot(identical(explicit$dll, dll), identical(dynamic$dll, dll))
    stopifnot(identical(dll$path, "/library/nativepkg/libs/native.pkg.so"))
}
"#,
    )?;
    ctx.run_global(
        "load",
        r#"
dyn.load("/library/nativepkg/libs/native.pkg.so")
dyn.load("/library/nativepkg/other/native_pkg.so")
loadNamespace("nativepkg")
"#,
    )?;
    Ok(())
}

#[derive(Default)]
struct Loaded(Vec<(String, SexpId)>);

impl Trace for Loaded {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        for (_, dll) in &self.0 {
            tracer.edge(*dll);
        }
    }
}

impl CtxExt for Loaded {}

struct Hooks;

impl NativeHooks for Hooks {
    fn dyn_load(&self, ctx: &mut Ctx, file: &Path, _: bool, _: bool, _: SexpId) -> EvalResult {
        let file = file.to_string_lossy().into_owned();
        let name = ctx.session_mut().scalar_str(&dll_name(&file))?;
        ctx.protect(name);
        let path = ctx.session_mut().scalar_str(&file)?;
        let dll = named_list(ctx, &[("name", name), ("path", path)])?;
        ctx.ext::<Loaded>().0.push((file, dll));
        Ok(dll)
    }

    fn dyn_unload(&self, ctx: &mut Ctx, file: &Path, _: SexpId) -> EvalResult<()> {
        ctx.ext::<Loaded>()
            .0
            .retain(|(path, _)| Path::new(path) != file);
        Ok(())
    }

    fn get_loaded_dlls(&self, ctx: &mut Ctx) -> EvalResult {
        let paths = ctx.ext::<Loaded>().0.clone();
        let dlls = ctx.session_mut().alloc_list(paths.len())?;
        ctx.protect(dlls);
        for (index, (_, dll)) in paths.into_iter().enumerate() {
            ctx.session_mut().list_set(dlls, index, dll)?;
        }
        Ok(dlls)
    }

    fn get_native_symbol_info(
        &self,
        ctx: &mut Ctx,
        name: &str,
        package: Option<&str>,
        _: bool,
        _: SexpId,
    ) -> EvalResult {
        let entries = ctx.ext::<Loaded>().0.clone();
        for (path, dll) in entries.into_iter().rev() {
            if package == Some(dll_name(&path).as_str())
                && (name != "dynamic" || !path.ends_with("/other/native_pkg.so"))
            {
                return symbol(ctx, name, dll);
            }
        }
        Err(ctx.stop(roxide_value::SEXP_NIL, "symbol not found"))
    }

    fn get_dll_registered_routines(
        &self,
        ctx: &mut Ctx,
        dll: SexpId,
        names: bool,
        _: SexpId,
    ) -> EvalResult {
        assert!(names);
        let entries = &ctx.ext::<Loaded>().0;
        let name = if entries
            .iter()
            .any(|(path, id)| *id == dll && dll_name(path) == "native_pkg")
        {
            "lookup"
        } else {
            "registered"
        };
        let symbol = symbol(ctx, name, dll)?;
        let routines = named_list(ctx, &[(name, symbol)])?;
        named_list(ctx, &[(".Call", routines)])
    }
}

fn dll_name(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default()
        .replace('.', "_")
}

fn symbol(ctx: &mut Ctx, name: &str, dll: SexpId) -> EvalResult {
    let name = ctx.session_mut().scalar_str(name)?;
    named_list(ctx, &[("name", name), ("dll", dll)])
}

fn named_list(ctx: &mut Ctx, fields: &[(&str, SexpId)]) -> EvalResult {
    for &(_, value) in fields {
        ctx.protect(value);
    }
    let list = ctx.session_mut().alloc_list(fields.len())?;
    ctx.protect(list);
    let mut labels = Vec::with_capacity(fields.len());
    for (index, &(key, value)) in fields.iter().enumerate() {
        ctx.session_mut().list_set(list, index, value)?;
        labels.push(
            ctx.session_mut()
                .mk_char(key.as_bytes(), roxide_value::Encoding::Native)?,
        );
    }
    let labels = ctx.session_mut().alloc_str_from(&labels)?;
    ctx.protect(labels);
    let names = ctx.session_mut().intern("names")?;
    ctx.session_mut().set_attrib(list, names, labels)?;
    Ok(list)
}
