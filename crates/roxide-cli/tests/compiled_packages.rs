#![cfg(unix)]

//! Test compiled package builds through the command-line interface.

use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use roxide_host::{Host, NativeHost};

struct Fixture {
    home: PathBuf,
    work: PathBuf,
}

impl Fixture {
    fn new(root: &Path) -> io::Result<Self> {
        let home = root.join("staged home");
        let work = root.join("build");
        fs::create_dir_all(home.join("etc"))?;
        fs::create_dir_all(home.join("share/make"))?;
        fs::create_dir_all(home.join("bin"))?;
        fs::create_dir_all(&work)?;
        fs::write(
            home.join("share/make/shlib.mk"),
            include_str!("../../../share/make/shlib.mk"),
        )?;
        let ldflags = if cfg!(target_os = "macos") {
            "-dynamiclib"
        } else {
            "-shared"
        };
        fs::write(
            home.join("etc/Makeconf"),
            format!(
                "CC = clang\nCXX = clang++\nCXX17 = clang++\nCXX17STD = -std=c++17\n\
                 CPICFLAGS = -fPIC\nCXXPICFLAGS = -fPIC\nCXXFLAGS = -std=c++11\n\
                 CXX17FLAGS = -O2\nCXX17PICFLAGS = -fPIC\nSHLIB_EXT = .so\n\
                 SHLIB_LD = $(CC)\nSHLIB_CXXLD = $(CXX)\n\
                 SHLIB_LDFLAGS = {ldflags}\nSHLIB_CXXLDFLAGS = {ldflags}\n"
            ),
        )?;
        Ok(Self { home, work })
    }

    fn run(&self, args: &[&str]) -> io::Result<Output> {
        Command::new(env!("CARGO_BIN_EXE_roxide"))
            .args(args)
            .current_dir(&self.work)
            .env("R_HOME", &self.home)
            .env("R_MAKEVARS_USER", "")
            .env("MAKE", "make")
            .env_remove("MAKEFLAGS")
            .env_remove("MFLAGS")
            .output()
    }
}

fn success(output: Output) {
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn build_and_install_compiled_packages() -> io::Result<()> {
    let scratch = NativeHost::new();
    let fixture = Fixture::new(scratch.session_temp_dir())?;
    for subcommand in ["config", "SHLIB"] {
        let help = fixture.run(&["CMD", subcommand, "--help"])?;
        assert!(String::from_utf8_lossy(&help.stdout).contains("Usage: R CMD"));
        success(help);
    }
    let config = fixture.run(&["CMD", "config", "SHLIB_LD"])?;
    assert_eq!(config.stdout, b"clang\n");
    success(config);

    check_shlib(&fixture)?;
    check_install(&fixture)
}

fn check_shlib(fixture: &Fixture) -> io::Result<()> {
    fs::write(
        fixture.work.join("demo.c"),
        "int demo(void) { return 12; }\n",
    )?;
    let dry = fixture.run(&["CMD", "SHLIB", "-n", "demo.c"])?;
    let long = fixture.run(&["CMD", "SHLIB", "--dry-run", "demo.c"])?;
    let display = String::from_utf8_lossy(&dry.stdout);
    assert!(display.contains("make cmd is") && display.contains("make would use"));
    assert!(display.contains("clang") && display.contains("demo.o") && display.contains("demo.so"));
    assert!(dry.stderr.is_empty());
    assert_eq!(dry.stdout, long.stdout);
    success(dry);
    success(long);
    assert!(!fixture.work.join("demo.o").exists());
    assert!(!fixture.work.join("demo.so").exists());

    let missing = fixture.run(&["CMD", "SHLIB", "missing.c"])?;
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("missing.c"));

    success(fixture.run(&["CMD", "SHLIB", "-o", "custom.so", "demo.c"])?);
    assert!(fixture.work.join("custom.so").is_file());
    assert!(fixture.work.join("demo.o").is_file());

    fs::write(
        fixture.work.join("Makevars"),
        "PKG_CPPFLAGS = -DFROM_MAKEVARS=37\nCXX_STD = CXX17\n",
    )?;
    fs::write(
        fixture.work.join("demo cxx.cpp"),
        "#include <string>\nstatic_assert(__cplusplus >= 201703L);\n\
         std::string demo() { return std::to_string(FROM_MAKEVARS); }\n",
    )?;
    success(fixture.run(&[
        "CMD",
        "SHLIB",
        "-o",
        "out/custom.so",
        "--preclean",
        "--clean",
        "demo cxx.cpp",
    ])?);
    assert!(fixture.work.join("out/custom.so").is_file());
    let broken = fixture.run(&["CMD", "SHLIB", "CXX_STD=CXX11", "demo cxx.cpp"])?;
    assert_eq!(broken.status.code(), Some(1));
    assert!(!fixture.work.join("demo cxx.o").exists());
    Ok(())
}

fn check_install(fixture: &Fixture) -> io::Result<()> {
    let source = fixture.work.join("nativepkg");
    fs::create_dir_all(source.join("src"))?;
    fs::create_dir_all(source.join("R"))?;
    fs::create_dir_all(fixture.work.join("library/headers/include"))?;
    fs::write(
        fixture.work.join("library/headers/DESCRIPTION"),
        "Package: headers\nVersion: 1.0\n",
    )?;
    fs::write(
        fixture.work.join("library/headers/include/fixture.h"),
        "#define HEADER_VALUE 10\n",
    )?;
    fs::write(
        source.join("DESCRIPTION"),
        "Package: nativepkg\nVersion: 1.0\nNeedsCompilation: yes\nLinkingTo: headers (>= 1.0)\n",
    )?;
    fs::write(source.join("NAMESPACE"), "useDynLib(nativepkg)\n")?;
    fs::write(
        source.join("R/hooks.R"),
        ".onLoad <- function(...) stop('child phase')\n",
    )?;
    fs::write(
        source.join("configure"),
        "printf '#define CONFIGURED 32\\n' > src/configured.h\n",
    )?;
    fs::write(
        source.join("src/init.c"),
        "#include <fixture.h>\n#include \"configured.h\"\n\
         int native_value(void) { return HEADER_VALUE + CONFIGURED; }\n\
         void R_init_nativepkg(void *dll) { (void)dll; }\n",
    )?;
    let child = fixture.home.join("bin/R");
    fs::write(
        &child,
        r#"#!/bin/sh
test "$1" = --vanilla && test "$2" = --slave && test "$3" = -e || exit 20
test "$5" = --args && test "$6" = nativepkg || exit 21
test -f "$7/nativepkg/libs/nativepkg.so" && test -f "$7/nativepkg/Meta/Roxide.rds" || exit 22
printf '%s\n' "$7" >> load-paths
"#,
    )?;
    fs::set_permissions(&child, fs::Permissions::from_mode(0o755))?;
    success(fixture.run(&["CMD", "INSTALL", "-l", "library", "nativepkg"])?);
    assert!(
        fixture
            .work
            .join("library/nativepkg/libs/nativepkg.so")
            .is_file()
    );
    assert!(!source.join("src/configured.h").exists());
    assert!(!source.join("src/init.o").exists());
    let paths = fs::read_to_string(fixture.work.join("load-paths"))?;
    let paths: Vec<_> = paths.lines().collect();
    assert_eq!(paths.len(), 2);
    assert!(paths[0].ends_with("/library/00LOCK-nativepkg/00new"));
    assert!(paths[1].ends_with("/library"));

    let abi = roxide_eval::api::CAPI_ABI_VERSION
        .rsplit_once("-v")
        .ok_or_else(|| io::Error::other("ABI version has no revision"))?
        .1;
    let script = format!(
        "stamp <- readRDS('library/nativepkg/Meta/Roxide.rds'); \
         stopifnot(identical(stamp$capi_abi, {abi}L), nzchar(stamp$roxide_version), nzchar(stamp$built)); \
         info <- readRDS('library/nativepkg/Meta/package.rds'); \
         stopifnot(identical(names(info$LinkingTo), 'headers'))"
    );
    success(fixture.run(&["--vanilla", "--slave", "-e", &script])?);
    let output = fixture.run(&[
        "--vanilla",
        "--slave",
        "-e",
        ".libPaths('library'); loadNamespace('nativepkg')",
    ])?;
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("compiled code requires the staged Roxide R_HOME")
    );

    let previous = fs::read(fixture.work.join("library/nativepkg/libs/nativepkg.so"))?;
    fs::write(
        source.join("src/Makefile"),
        "all:\n\t@test -n \"$$R_HOME\" && test -n \"$$R_INCLUDE_DIR\" && test -n \"$$R_SHARE_DIR\" && test ! -f configured.h\n\
         \tclang -fPIC $(PKG_CPPFLAGS) -c custom.c -o custom.o\n\
         \tclang $(LINK_MODE) custom.o -o nativepkg$(SHLIB_EXT)\n",
    )?;
    fs::write(
        source.join("src/custom.c"),
        "#include <fixture.h>\nint custom(void) { return HEADER_VALUE; }\n",
    )?;
    let makefile = source.join("src/Makefile");
    let recipe = fs::read_to_string(&makefile)?;
    let ldflags = if cfg!(target_os = "macos") {
        "-dynamiclib"
    } else {
        "-shared"
    };
    fs::write(makefile, recipe.replace("$(LINK_MODE)", ldflags))?;

    fs::write(
        &child,
        "#!/bin/sh\ncase \"$7\" in *00new) exit 0;; *) exit 4;; esac\n",
    )?;
    let failed = fixture.run(&[
        "CMD",
        "INSTALL",
        "--no-configure",
        "-l",
        "library",
        "nativepkg",
    ])?;
    assert_eq!(failed.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&failed.stderr)
            .contains("test load failed for package 'nativepkg'")
    );
    assert_eq!(
        fs::read(fixture.work.join("library/nativepkg/libs/nativepkg.so"))?,
        previous
    );
    assert!(!fixture.work.join("library/00LOCK-nativepkg").exists());
    success(fixture.run(&[
        "CMD",
        "INSTALL",
        "--no-configure",
        "--no-test-load",
        "-l",
        "library",
        "nativepkg",
    ])?);
    assert_ne!(
        fs::read(fixture.work.join("library/nativepkg/libs/nativepkg.so"))?,
        previous
    );
    assert!(!source.join("src/custom.o").exists());
    Ok(())
}
