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

fn context(namespace: &str, stamp: Option<&str>) -> Result<Ctx, Unwind> {
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
        (
            "R/source.R",
            r#"
.onLoad <- function(lib, pkg) {
    stopifnot(identical(C_registered_end$name, "registered"))
    stopifnot(identical(explicit$name, "lookup"))
    stopifnot(identical(getNamespaceInfo(pkg, "DLLs")$nativepkg$path,
                        "/library/nativepkg/libs/nativepkg.so"))
}
"#,
        ),
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
        let mut ctx = context("useDynLib(nativepkg)", stamp)?;
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
    )?;
    ctx.run_global("load", "loadNamespace('nativepkg')")?;
    assert_eq!(
        ctx.ext::<Loaded>().0,
        ["/library/nativepkg/libs/nativepkg.so"]
    );
    ctx.run_global("unload", "unloadNamespace('nativepkg')")?;
    assert!(ctx.ext::<Loaded>().0.is_empty());
    assert!(ctx.ns_lookup("nativepkg").is_none());
    Ok(())
}

#[test]
fn rolls_back_the_dll_if_namespace_loading_fails() -> EvalResult<()> {
    let mut ctx = context("useDynLib(nativepkg)", Some(&valid_stamp()))?;
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

#[derive(Default)]
struct Loaded(Vec<String>);

impl Trace for Loaded {
    fn trace(&self, _: &mut Tracer<'_>) {}
}

impl CtxExt for Loaded {}

struct Hooks;

impl NativeHooks for Hooks {
    fn dyn_load(&self, ctx: &mut Ctx, file: &Path, _: bool, _: bool, _: SexpId) -> EvalResult {
        let file = file.to_string_lossy().into_owned();
        ctx.ext::<Loaded>().0.push(file.clone());
        scalar_field(ctx, "path", &file)
    }

    fn dyn_unload(&self, ctx: &mut Ctx, file: &Path, _: SexpId) -> EvalResult<()> {
        ctx.ext::<Loaded>().0.retain(|path| Path::new(path) != file);
        Ok(())
    }

    fn get_loaded_dlls(&self, ctx: &mut Ctx) -> EvalResult {
        let paths = ctx.ext::<Loaded>().0.clone();
        let dlls = ctx.session_mut().alloc_list(paths.len())?;
        ctx.protect(dlls);
        for (index, path) in paths.iter().enumerate() {
            let dll = scalar_field(ctx, "path", path)?;
            ctx.session_mut().list_set(dlls, index, dll)?;
        }
        Ok(dlls)
    }

    fn get_native_symbol_info(
        &self,
        ctx: &mut Ctx,
        name: &str,
        _: Option<&str>,
        _: bool,
        _: SexpId,
    ) -> EvalResult {
        scalar_field(ctx, "name", name)
    }

    fn get_dll_registered_routines(
        &self,
        ctx: &mut Ctx,
        _: SexpId,
        names: bool,
        _: SexpId,
    ) -> EvalResult {
        assert!(names);
        let symbol = scalar_field(ctx, "name", "registered")?;
        let routines = named_list(ctx, "registered", symbol)?;
        named_list(ctx, ".Call", routines)
    }
}

fn scalar_field(ctx: &mut Ctx, key: &str, value: &str) -> EvalResult {
    let value = ctx.session_mut().scalar_str(value)?;
    named_list(ctx, key, value)
}

fn named_list(ctx: &mut Ctx, key: &str, value: SexpId) -> EvalResult {
    ctx.protect(value);
    let list = ctx.session_mut().alloc_list(1)?;
    ctx.protect(list);
    ctx.session_mut().list_set(list, 0, value)?;
    let key = ctx.session_mut().scalar_str(key)?;
    let names = ctx.session_mut().intern("names")?;
    ctx.session_mut().set_attrib(list, names, key)?;
    Ok(list)
}
