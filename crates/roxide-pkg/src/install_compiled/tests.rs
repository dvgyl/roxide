use std::fs;
use std::os::unix::fs::PermissionsExt;

use roxide_gc::GcPolicy;
use roxide_host::{Host, NativeHost};

use super::*;

#[test]
fn compiled_package_runs_configure_make_and_install_hook() -> Result<(), String> {
    let host = NativeHost::new();
    let root = host
        .canonicalize(host.session_temp_dir())
        .map_err(|error| error.to_string())?
        .join("compiled-hook");
    let home = root.join("home");
    let source = root.join("fixture");
    let src = source.join("src");
    let lock = root.join("lock");
    let dest = lock.join("00new/fixture");
    let cwd = host.cwd().map_err(|error| error.to_string())?;
    let files = [
        (
            home.join("share/make/shlib.mk"),
            include_str!("../../../../share/make/shlib.mk"),
        ),
        (
            source.join("DESCRIPTION"),
            "Package: fixture\nVersion: 1.0\n",
        ),
        (
            source.join("configure"),
            "test \"$R_OSTYPE\" = unix && printf '#!/bin/sh\\nexit 0\\n' > src/aux && chmod 751 src/aux\n",
        ),
        (
            src.join("example.c"),
            "int fixture_value(void) { return 1; }\n",
        ),
        (
            src.join("Makevars"),
            "all: $(SHLIB) client.so\n\
             client.so: example.o\n\
             \ttest \"$$R_OSTYPE\" = unix\n\
             \t$(SHLIB_LINK) -o \"$@\" example.o\n",
        ),
        (
            src.join("install.libs.R"),
            r#"
stopifnot(identical(names(R_PACKAGE_NAME), "Package"))
stopifnot(identical(unname(R_PACKAGE_NAME), "fixture"), !WINDOWS)
stopifnot(identical(getwd(), file.path(R_PACKAGE_SOURCE, "src")))
stopifnot(identical(Sys.getenv("R_PACKAGE_DIR"), R_PACKAGE_DIR))
stopifnot(identical(Sys.getenv("R_PACKAGE_NAME"), "fixture"))
stopifnot(identical(Sys.getenv("R_OSTYPE"), "unix"))
stopifnot(identical(SHLIB_EXT, ".so"), identical(R_ARCH, ""))
dir.create(file.path(R_PACKAGE_DIR, "bin"))
stopifnot(dir.exists(file.path(R_PACKAGE_DIR, "libs")))
stopifnot(file.copy("aux", file.path(R_PACKAGE_DIR, "bin", "tool")))
stopifnot(file.copy(paste0("client", SHLIB_EXT), file.path(R_PACKAGE_DIR, "libs")))
"#,
        ),
    ];
    for (file, contents) in files {
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        fs::write(file, contents).map_err(|error| error.to_string())?;
    }
    fs::create_dir_all(dest.join("Meta")).map_err(|error| error.to_string())?;
    fs::create_dir_all(home.join("etc")).map_err(|error| error.to_string())?;
    let linker = if cfg!(target_os = "macos") {
        "-dynamiclib"
    } else {
        "-shared"
    };
    fs::write(
        home.join("etc/Makeconf"),
        format!(
            "CC = cc\nCPICFLAGS = -fPIC\nSHLIB_LD = cc\nSHLIB_LDFLAGS = {linker}\nSHLIB_EXT = .so\n"
        ),
    )
    .map_err(|error| error.to_string())?;

    let mut ctx = Ctx::new(Box::new(host), GcPolicy::Torture).map_err(|error| error.to_string())?;
    roxide_base::register_all(&mut ctx).map_err(|error| error.to_string())?;
    crate::register(&mut ctx).map_err(|error| error.to_string())?;
    ctx.host()
        .set_env_var("R_HOME", Some(home.as_os_str().as_encoded_bytes()))
        .map_err(|error| error.to_string())?;
    for (key, value) in [
        ("R_OSTYPE", "wrong"),
        ("R_ARCH", ""),
        ("R_MAKEVARS_USER", ""),
    ] {
        ctx.host()
            .set_env_var(key, Some(value.as_bytes()))
            .map_err(|error| error.to_string())?;
    }
    let package = CompiledPackage::prepare(&mut ctx, &source, &lock, &[], false)
        .map_err(|error| format!("{error:?}"))?;
    package
        .build(&mut ctx, &dest, "fixture", "fixture build")
        .map_err(|error| format!("{error:?}"))?;

    assert!(dest.join("libs/client.so").is_file());
    assert!(!dest.join("libs/fixture.so").exists());
    assert!(package.source.join("src/fixture.so").is_file());
    assert!(dest.join("Meta/Roxide.rds").is_file());
    let tool = fs::metadata(dest.join("bin/tool")).map_err(|error| error.to_string())?;
    assert_eq!(tool.permissions().mode() & 0o111, 0o111);
    assert_eq!(ctx.host().cwd().map_err(|error| error.to_string())?, cwd);
    assert_eq!(
        ctx.host().env_var("R_OSTYPE").as_deref(),
        Some(&b"wrong"[..])
    );
    ctx.host()
        .remove(&package.source.join("src/install.libs.R"), false)
        .map_err(|error| error.to_string())?;
    package
        .build(&mut ctx, &dest, "fixture", "fixture build")
        .map_err(|error| format!("{error:?}"))?;
    assert!(dest.join("libs/fixture.so").is_file());
    Ok(())
}
