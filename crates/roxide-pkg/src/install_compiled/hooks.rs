use std::path::Path;

use roxide_eval::{Args, Call, Ctx, EvalResult, Module, Visibility};
use roxide_value::{FALSE_SCALAR, GLOBAL_ENV, SEXP_NIL, TRUE_SCALAR, ValueApi};

use super::preserve_executable_bits;
use crate::description::read_text;
use crate::loader::values::{define, internal, named, names};
use crate::makeconf::env_text;
use crate::shlib::os_type;

pub(crate) const MODULE: Module = Module {
    prims: &[internal(
        "rox_install_copy_modes",
        3,
        copy_modes,
        Visibility::On,
    )],
    prelude: "",
};

const FILE_COPY: &str = r#"
file.copy <- function(from, to, overwrite = recursive, recursive = FALSE,
                      copy.mode = TRUE, copy.date = FALSE) {
    copied <- base::file.copy(from, to, overwrite, recursive, copy.mode, copy.date)
    if (isTRUE(copy.mode) && any(copied))
        .Internal(rox_install_copy_modes(path.expand(as.character(from)),
                                         path.expand(as.character(to)), copied))
    copied
}
"#;

pub(super) fn install(
    ctx: &mut Ctx,
    hook: &Path,
    source: &Path,
    dest: &Path,
    package: &str,
    extension: &str,
) -> EvalResult<()> {
    let code = read_text(ctx, hook)?;
    let cwd = ctx
        .host()
        .cwd()
        .map_err(|error| ctx.stop(SEXP_NIL, error.to_string()))?;
    let os = os_type(ctx.host());
    let windows = os == "windows";
    let dest = dest.to_string_lossy();
    let source_path = source.to_string_lossy();
    let arch = env_text(ctx.host(), "R_ARCH")
        .map_err(|error| ctx.stop(SEXP_NIL, error))?
        .unwrap_or_default();
    let environment = [
        ("R_PACKAGE_DIR", dest.as_ref()),
        ("R_PACKAGE_NAME", package),
        ("R_OSTYPE", os),
    ];
    let previous = environment.map(|(key, _)| (key, ctx.host().env_var(key)));
    let result = ctx.scoped(|ctx| {
        for (key, value) in environment {
            ctx.host()
                .set_env_var(key, Some(value.as_bytes()))
                .map_err(|error| ctx.stop(SEXP_NIL, error.to_string()))?;
        }
        ctx.host()
            .set_cwd(&source.join("src"))
            .map_err(|error| ctx.stop(SEXP_NIL, error.to_string()))?;
        let parent = if windows {
            GLOBAL_ENV
        } else {
            let env = ctx.session_mut().alloc_env(GLOBAL_ENV)?;
            ctx.protect(env);
            ctx.run_script("<install.libs helpers>", FILE_COPY, env, false)?;
            env
        };
        let env = ctx.session_mut().alloc_env(parent)?;
        ctx.protect(env);
        for (key, text) in [
            ("R_PACKAGE_DIR", dest.as_ref()),
            ("R_PACKAGE_NAME", package),
            ("R_PACKAGE_SOURCE", source_path.as_ref()),
            ("R_ARCH", &arch),
            ("SHLIB_EXT", extension),
        ] {
            let value = ctx.session_mut().scalar_str(text)?;
            ctx.protect(value);
            if key == "R_PACKAGE_NAME" {
                named(ctx, value, &["Package"])?;
            }
            define(ctx, env, key, value)?;
        }
        define(
            ctx,
            env,
            "WINDOWS",
            if windows { TRUE_SCALAR } else { FALSE_SCALAR },
        )?;
        ctx.run_script(&hook.to_string_lossy(), &code, env, false)
    });
    let mut restored = ctx.host().set_cwd(&cwd);
    for (key, value) in previous {
        restored = restored.and(ctx.host().set_env_var(key, value.as_deref()));
    }
    result?;
    restored.map_err(|error| ctx.stop(SEXP_NIL, error.to_string()))
}

fn copy_modes(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let from = names(ctx, args.get(0))?;
    let to = names(ctx, args.get(1))?;
    let copied = ctx.session().lgl(args.get(2))?.to_vec();
    let cwd = ctx
        .host()
        .cwd()
        .map_err(|error| ctx.stop(call.err_call(), error.to_string()))?;
    let directory = to.len() == 1
        && to[0].as_deref().is_some_and(|path| {
            ctx.host()
                .metadata(Path::new(path), true)
                .is_ok_and(|meta| meta.is_dir)
        });
    for (index, from) in from.iter().enumerate() {
        let target = to.get(if to.len() == 1 { 0 } else { index });
        if let (Some(from), Some(Some(to)), Some(true)) = (
            from,
            target,
            copied.get(index).and_then(|value| value.get()),
        ) {
            let from = cwd.join(from);
            let mut to = cwd.join(to);
            if directory && let Some(name) = from.file_name() {
                to.push(name);
            }
            preserve_executable_bits(&from, &to, None, ctx.host())
                .map_err(|error| ctx.stop(call.err_call(), error))?;
        }
    }
    Ok(SEXP_NIL)
}
