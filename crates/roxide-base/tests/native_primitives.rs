#![forbid(unsafe_code)]

//! Native calls through the evaluator with a mock native hook.

use std::path::Path;
use std::process::Command;

use roxide_eval::{Args, Call, Ctx, CtxExt, DotCOpts, EvalResult, NativeCallKind, NativeHooks};
use roxide_gc::{GcPolicy, SexpId, Trace, Tracer};
use roxide_host::PureHost;
use roxide_value::{BASE_ENV, FALSE_SCALAR, GLOBAL_ENV, SEXP_NIL, TRUE_SCALAR, ValueApi};

struct Mock;
struct ControlMock;

#[derive(Default)]
struct Loaded(bool);

impl CtxExt for Loaded {}
impl Trace for Loaded {
    fn trace(&self, _: &mut Tracer<'_>) {}
}

fn binding(ctx: &mut Ctx, name: &str, env: SexpId) -> SexpId {
    let sym = ctx.session_mut().intern(name).expect("intern name");
    ctx.get_var(sym, env).expect("read binding").expect(name)
}

impl NativeHooks for Mock {
    fn call_native(
        &self,
        ctx: &mut Ctx,
        kind: NativeCallKind,
        symbol: SexpId,
        args: &Args,
        package: Option<&str>,
        dotc: Option<&DotCOpts>,
        op: SexpId,
        env: SexpId,
        call: &Call,
    ) -> EvalResult {
        assert_eq!(env, call.env);
        assert_eq!(binding(ctx, "marker", env), symbol);
        let mut names: Vec<_> = args
            .iter()
            .map(|(tag, _)| tag.map(|tag| ctx.session().sym_name(tag).expect("read tag")))
            .collect();
        let (expected_names, expected_package, expected_options) = match kind {
            NativeCallKind::Call | NativeCallKind::External | NativeCallKind::External2 => {
                assert_eq!(names.pop(), Some(Some(".NAME")));
                let name = ctx.session().str_slice(args.get(3))?;
                assert_eq!(ctx.session().char_bytes(name[0])?, b"other");
                (vec![Some("NAOK"), Some("PACK"), None], None, None)
            }
            NativeCallKind::C => (
                vec![Some("x"), None, Some(".NAME")],
                Some("demo"),
                Some(DotCOpts {
                    naok: true,
                    ..DotCOpts::default()
                }),
            ),
            NativeCallKind::Fortran => (
                vec![Some("x"), None, Some(".NAME")],
                None,
                Some(DotCOpts::default()),
            ),
        };
        assert_eq!(names, expected_names);
        for (i, value) in args.values().iter().take(names.len()).enumerate() {
            let values = ctx.session().int(*value).expect("read argument");
            assert_eq!(values.to_vec(), vec![i32::try_from(i).expect("index") + 1]);
        }
        assert_eq!(package, expected_package);
        assert_eq!(dotc, expected_options.as_ref());
        if kind == NativeCallKind::External2 {
            assert_eq!(op, binding(ctx, ".External2", BASE_ENV));
        } else {
            assert_eq!(op, SEXP_NIL);
        }
        Ok(symbol)
    }

    fn dyn_load(
        &self,
        ctx: &mut Ctx,
        path: &Path,
        local: bool,
        now: bool,
        _: SexpId,
    ) -> EvalResult {
        assert!(local);
        assert!(!now);
        assert_eq!(path, Path::new("/packages/demo/libs/demo.so"));
        ctx.ext::<Loaded>().0 = true;
        Ok(binding(ctx, "mock_dll", GLOBAL_ENV))
    }

    fn dyn_unload(&self, ctx: &mut Ctx, path: &Path, _: SexpId) -> EvalResult<()> {
        assert_eq!(path, Path::new("/packages/demo/libs/demo.so"));
        ctx.ext::<Loaded>().0 = false;
        Ok(())
    }

    fn is_loaded(
        &self,
        ctx: &mut Ctx,
        symbol: &str,
        package: Option<&str>,
        kind: Option<NativeCallKind>,
    ) -> EvalResult<bool> {
        Ok(ctx.ext::<Loaded>().0
            && symbol == "one"
            && matches!(package, None | Some("demo"))
            && matches!(kind, None | Some(NativeCallKind::Call)))
    }

    fn get_native_symbol_info(
        &self,
        ctx: &mut Ctx,
        name: &str,
        package: Option<&str>,
        with_registration: bool,
        call: SexpId,
    ) -> EvalResult {
        match (name, package, with_registration) {
            ("one", None | Some("demo"), false) => Ok(binding(ctx, "one", GLOBAL_ENV)),
            ("two", Some("demo"), false) => Ok(binding(ctx, "two", GLOBAL_ENV)),
            ("one", Some("demo"), true) => Ok(binding(ctx, "registered_one", GLOBAL_ENV)),
            _ => Err(ctx.stop(call, format!("no such symbol {name}"))),
        }
    }

    fn get_loaded_dlls(&self, ctx: &mut Ctx) -> EvalResult {
        let key = if ctx.ext::<Loaded>().0 {
            "loaded"
        } else {
            "empty"
        };
        Ok(binding(ctx, key, GLOBAL_ENV))
    }

    fn get_dll_registered_routines(
        &self,
        ctx: &mut Ctx,
        dll: SexpId,
        add_names: bool,
        _: SexpId,
    ) -> EvalResult {
        assert_eq!(dll, binding(ctx, "mock_dll", GLOBAL_ENV));
        let key = if add_names { "routines" } else { "unnamed" };
        Ok(binding(ctx, key, GLOBAL_ENV))
    }
}

impl NativeHooks for ControlMock {
    fn call_native(
        &self,
        ctx: &mut Ctx,
        kind: NativeCallKind,
        symbol: SexpId,
        args: &Args,
        package: Option<&str>,
        dotc: Option<&DotCOpts>,
        _: SexpId,
        _: SexpId,
        call: &Call,
    ) -> EvalResult {
        let name = ctx.session().str_slice(symbol)?;
        assert_eq!(name.len(), 1);
        assert_eq!(ctx.session().char_bytes(name[0])?, b"f");
        assert!(args.is_empty());
        match kind {
            NativeCallKind::Call => {
                assert_eq!(package, Some("NA"));
                assert!(dotc.is_none());
                Err(ctx.stop(
                    call.err_call(),
                    "\"f\" not available for .Call() for package \"NA\"",
                ))
            }
            NativeCallKind::C | NativeCallKind::Fortran => {
                assert_eq!(package, Some("demo"));
                let opts = dotc.expect("foreign call options");
                assert!(opts.dup);
                assert_eq!(opts.encoding, None);
                Ok(if opts.naok { TRUE_SCALAR } else { FALSE_SCALAR })
            }
            _ => panic!("unexpected native call"),
        }
    }
}

fn context() -> Ctx {
    let mut ctx = Ctx::new(Box::new(PureHost::new()), GcPolicy::Torture).expect("create context");
    roxide_base::register_all(&mut ctx).expect("register base");
    ctx.set_native_hooks(&Mock);
    ctx
}

fn run(ctx: &mut Ctx, script: &str) {
    let script = format!(
        "tryCatch({{ {script} }}, error=function(e) assign('failure', conditionMessage(e), envir=.GlobalEnv))"
    );
    ctx.run_script("native primitives", &script, GLOBAL_ENV, false)
        .expect("run script");
    let sym = ctx.session_mut().intern("failure").expect("intern failure");
    if let Some(value) = ctx.get_var(sym, GLOBAL_ENV).expect("read failure") {
        let chars = ctx.session().str_slice(value).expect("error message");
        let bytes = ctx.session().char_bytes(chars[0]).expect("message bytes");
        panic!("{}", String::from_utf8_lossy(bytes));
    }
}

#[test]
fn foreign_calls_forward_symbol_arguments_and_options() {
    let mut ctx = context();
    let sym = ctx.session_mut().intern("pointer").unwrap();
    let pointer = ctx
        .session_mut()
        .alloc_extptr(1, SEXP_NIL, SEXP_NIL)
        .unwrap();
    ctx.session_mut()
        .env_define(GLOBAL_ENV, sym, pointer)
        .unwrap();
    run(
        &mut ctx,
        r#"
        check <- function() {
            marker <- "native"
            stopifnot(identical(.Call(marker, NAOK=1L, PACK=2L, 3L, .NAME="other"), marker))
            marker <- structure(list(name="native", address=pointer), class="NativeSymbolInfo")
            stopifnot(identical(.External(marker, NAOK=1L, PACK=2L, 3L, .NAME="other",
                                         PACKAGE=""), marker))
            marker <- pointer
            external2 <- .External2
            .External2 <- NULL
            stopifnot(identical(external2(marker, NAOK=1L, PACK=2L, 3L, .NAME="other",
                                          PACKAGE=NA_character_), marker))
            marker <- "native"
            stopifnot(identical(.C(marker, x=1L, 2L, .NAME=3L, DUP=0, NAOK="TRUE",
                                   PACKAGE="demo"), marker))
            stopifnot(identical(.Fortran(marker, x=1L, 2L, .NAME=3L), marker))
        }
        check()
        "#,
    );
}

#[test]
fn invalid_native_controls_raise_r_errors() {
    let mut ctx = context();
    ctx.set_native_hooks(&ControlMock);
    run(
        &mut ctx,
        r#"
        messageOf <- function(x) tryCatch(x, error=conditionMessage)
        for (name in c(".Call", ".External", ".External2", ".C", ".Fortran")) {
            check <- function(args, message) {
                expr <- parse(text=paste0(name, "(", args, ")"))[[1L]]
                actual <- messageOf(eval(expr))
                if (!identical(actual, message)) stop(paste(name, args, actual))
            }
            check("", "'.NAME' is missing")
            check('.NAME="f"', "the first argument should not be named")
            check('x=1L, .NAME="f"', "the first argument should not be named")
            check(', 1L', "argument 1 is empty")
            check('.NAME=', "argument 1 is empty")
            check('"f", , 1L', "argument 2 is empty")
            check('"f", PACKAGE=', "argument 2 is empty")
            check('"f", PACKAGE=1L, ', "argument 3 is empty")
            check('"f", PACKAGE=c("demo", "other")', "PACKAGE argument must be a single character string")
            check('"f", PACKAGE=character()', "PACKAGE argument must be a single character string")
            check('"f", PACKAGE=1L', "PACKAGE argument must be a single character string")
            check('"f", PACKAGE=""', 'PACKAGE = "" is invalid')
        }
        stopifnot(identical(messageOf(.Call("f", PACKAGE=NA_character_)),
                            '"f" not available for .Call() for package "NA"'))
        for (foreign in list(.C, .Fortran)) {
            stopifnot(identical(messageOf(foreign("f", NAOK=NA)), "invalid 'naok' value"),
                      identical(messageOf(foreign("f", NAOK=integer())), "invalid 'naok' value"))
        }
        "#,
    );
}

#[test]
fn duplicate_controls_warn_and_defunct_controls_are_ignored() {
    let mut ctx = context();
    ctx.set_native_hooks(&ControlMock);
    run(
        &mut ctx,
        r#"
        warnings <- character()
        capture <- function(expr) withCallingHandlers(expr, warning=function(w) {
            warnings <<- c(warnings, conditionMessage(w))
            invokeRestart("muffleWarning")
        })
        for (foreign in list(.C, .Fortran)) {
            warnings <- character()
            for (dup in list(FALSE, NA, list(), integer())) {
                stopifnot(capture(foreign("f", PACKAGE="demo", NAOK=c(1L, NA_integer_), DUP=dup)))
            }
            stopifnot(!capture(foreign("f", PACKAGE="demo", NAOK=c(FALSE, TRUE))))
            stopifnot(length(warnings) == 0L)
            stopifnot(capture(foreign("f", PACKAGE="", PACKAGE="demo",
                                      NAOK=NA, NAOK=c("TRUE", "FALSE"), DUP=list(), DUP=NA,
                                      ENCODING=list(), ENCODING=integer())))
            stopifnot(length(warnings) == 5L,
                      sum(warnings == "'PACKAGE' used more than once") == 1L,
                      sum(warnings == "'NAOK' used more than once") == 1L,
                      sum(warnings == "'DUP' used more than once") == 1L,
                      sum(warnings == "ENCODING is defunct and will be ignored") == 2L)
        }
        "#,
    );
}

#[test]
fn dll_wrappers_accessors_and_print_methods() {
    let mut ctx = context();
    let sym = ctx.session_mut().intern("reference").unwrap();
    let reference = ctx
        .session_mut()
        .alloc_extptr(1, SEXP_NIL, SEXP_NIL)
        .unwrap();
    ctx.session_mut()
        .env_define(GLOBAL_ENV, sym, reference)
        .unwrap();
    run(
        &mut ctx,
        r#"
        path <- "/packages/demo/libs/demo.so"
        reference <- structure(reference, class="DLLInfoReference")
        mock_dll <- structure(list(name="demo", path=path, dynamicLookup=FALSE, info=reference),
                              class="DLLInfo")
        empty <- structure(list(), class="DLLInfoList")
        loaded <- structure(list(demo=mock_dll), class="DLLInfoList")
        one <- structure(list(name="one", dll=mock_dll, numParameters=1L), class="NativeSymbolInfo")
        two <- structure(list(name="two", dll=mock_dll), class="NativeSymbolInfo")
        registered_one <- structure(one, class=c("CallRoutine","NativeSymbolInfo"))
        routines <- structure(list(.Call=list(one=registered_one)), class="DLLRegisteredRoutines")
        unnamed <- structure(list(.Call=list(registered_one)), class="DLLRegisteredRoutines")
        stopifnot(identical(getLoadedDLLs(), empty))
        v <- withVisible(dyn.load(now=FALSE, x=path))
        stopifnot(!v$visible, identical(v$value, mock_dll), identical(getLoadedDLLs(), loaded))
        stopifnot(is.loaded("one"), is.loaded("one", "demo", "Call"),
                  !is.loaded("one", "demo", "External"), !is.loaded("one", "demo", "C"),
                  !is.loaded("one", "demo", "Fortran"), !is.loaded("one", type="bad"),
                  !is.loaded(c("one", "two")), !is.loaded("one", c("demo", "other")),
                  !is.loaded("one", type=c("Call", "C")))
        stopifnot(identical(getNativeSymbolInfo("one"), one),
                  identical(getNativeSymbolInfo("one", mock_dll, withRegistrationInfo=TRUE), registered_one),
                  identical(getNativeSymbolInfo(list("one"), mock_dll[["info"]]), one),
                  identical(getNativeSymbolInfo(list(a="two", b="one"), mock_dll[["info"]]),
                            list(two=two, one=one)),
                  identical(getNativeSymbolInfo(c("two","one"), "demo"), list(two=two, one=one)),
                  identical(getNativeSymbolInfo("one", "demo", unlist=FALSE), list(one=one)),
                  length(getNativeSymbolInfo(character())) == 0L,
                  length(getNativeSymbolInfo(list())) == 0L,
                  identical(tryCatch(getNativeSymbolInfo("missing", mock_dll[["info"]]),
                                     error=conditionMessage), "no such symbol missing"))
        stopifnot(identical(mock_dll$name, "demo"), identical(mock_dll[[2L]], path),
                  is.null(mock_dll[["na"]]), identical(mock_dll$one, one),
                  identical(loaded$de, mock_dll), is.null(loaded$missing))
        stopifnot(identical(getDLLRegisteredRoutines(mock_dll), routines),
                  identical(getDLLRegisteredRoutines("demo"), routines),
                  identical(getDLLRegisteredRoutines(path, addNames=FALSE), unnamed))
        con <- textConnection("printed", "w")
        sink(con)
        for (x in list(mock_dll, loaded, one, routines)) {
            v <- withVisible(print(x))
            stopifnot(!v$visible, identical(v$value, x))
        }
        sink()
        close(con)
        stopifnot(identical(printed[1:3], c("DLL name: demo", paste0("Filename: ", path),
                                          "Dynamic lookup: FALSE")),
                  any(grepl("Dynamic.Lookup", printed, fixed=TRUE)),
                  any(grepl("NativeSymbolInfo", printed, fixed=TRUE)),
                  any(grepl(".Call.numParameters", printed, fixed=TRUE)))
        printed <- character()
        con <- textConnection("printed", "w")
        sink(con)
        print(empty)
        sink()
        close(con)
        stopifnot(length(printed) == 0L)
        opt <- options(width=20L)
        con <- textConnection("printed", "w")
        sink(con)
        print(mock_dll)
        sink()
        close(con)
        options(opt)
        stopifnot(identical(printed, c("DLL name: demo", "Filename:", paste0("      ", path),
                                     "Dynamic lookup: FALSE")))
        v <- withVisible(dyn.unload(path))
        stopifnot(!v$visible, is.null(v$value), !is.loaded("one"), identical(getLoadedDLLs(), empty))
        "#,
    );
}

#[test]
fn package_loaders_update_the_session_list() {
    let mut ctx = context();
    ctx.run_script(
        "package lookup",
        r#"find.package <- function(package, lib.loc) file.path(lib.loc, package)"#,
        BASE_ENV,
        false,
    )
    .expect("register package lookup");
    run(
        &mut ctx,
        r#"
        path <- "/packages/demo/libs/demo.so"
        mock_dll <- structure(list(name="demo", path=path, dynamicLookup=TRUE), class="DLLInfo")
        empty <- structure(list(), class="DLLInfoList")
        stopifnot(identical(library.dynam(), empty))
        new <- withVisible(.dynLibs(list(mock_dll)))
        stopifnot(!new$visible, inherits(new$value, "DLLInfoList"),
                  identical(new$value, .dynLibs()), identical(new$value[[1L]], mock_dll))
        msg <- tryCatch(library.dynam("demo", "demo"), error=conditionMessage)
        stopifnot(grepl("lib.loc", msg, fixed=TRUE), grepl("is missing", msg, fixed=TRUE))
        .dynLibs(list())
        v <- withVisible(library.dynam("demo", "demo", "/packages", now=FALSE))
        stopifnot(!v$visible, identical(v$value, mock_dll), length(library.dynam()) == 1L)
        library.dynam("demo", "demo", "/packages")
        stopifnot(length(.dynLibs()) == 1L)
        v <- withVisible(library.dynam.unload("demo", "/packages/demo"))
        stopifnot(!v$visible, identical(v$value, mock_dll), identical(.dynLibs(), empty))
        "#,
    );
    run(&mut context(), r#"stopifnot(length(.dynLibs()) == 0L)"#);
}

#[test]
fn staging_preserves_linker_flags_and_detects_fortran_runtime() {
    let staging = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/stage_rhome.sh");
    let output = Command::new("python3")
        .arg("-c")
        .arg(
            r##"
import os
from pathlib import Path
import shlex
import subprocess
import sys
from tempfile import TemporaryDirectory

with TemporaryDirectory() as scratch:
    root = Path(scratch)

    def write(path, content):
        path = root / path
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)
        return path

    def executable(path, body):
        write(path, "#!/usr/bin/env bash\n" + body).chmod(0o755)

    script = write("scripts/stage_rhome.sh", Path(sys.argv[1]).read_text())
    flags = ["-lone", "-L", "/a directory", "-ltwo", "-lone"]
    note = "note: native-static-libs: " + shlex.join(flags)
    executable("scripts/cargo_slot.sh", "printf '%s\\n' " + shlex.quote(note))
    executable("bin/nm", "printf '0000 T _roxide_main\\n0000 T _Rf_allocVector\\n'")
    executable("bin/cc", "printf '%s\\n' \"$@\" >> \"$LINK_LOG\"")
    executable("bin/uname", "printf 'Darwin\\n'")
    runtime = write("GCC runtime/libgfortran.dylib", "")
    executable("bin/gfortran", "printf '%s\\n' " + shlex.quote(str(runtime)))
    write("tools/capi/manifest.json", '{"symbols":[{"name":"Rf_allocVector","apitype":"api"}]}')
    write("crates/roxide-capi/include/R.h", "")
    write("target/debug/libroxide_libr.a", "")
    env = dict(os.environ, PATH=f"{root}/bin:{os.environ['PATH']}",
               CC=str(root / "bin/cc"), LINK_LOG=str(root / "links"),
               CARGO_TARGET_DIR=str(root / "target"), ROXIDE_SKIP_BUILD="0")
    env.pop("FC", None)
    env.pop("FLIBS", None)

    def stage():
        subprocess.run(["bash", str(script)], env=env, check=True)
        return (root / "target/rhome/etc/Makeconf").read_text().splitlines()

    config = stage()
    assert f"FC = {root}/bin/gfortran" in config
    assert f'FLIBS = -L"{runtime.parent}" -lgfortran' in config
    linked = (root / "links").read_text().splitlines()
    first = linked.index(flags[0])
    assert linked[first:first + len(flags)] == flags, linked
    env["ROXIDE_SKIP_BUILD"] = "1"
    env["FLIBS"] = "-lcustom"
    assert "FLIBS = -lcustom" in stage()
    env["FLIBS"] = ""
    assert "FLIBS = " in stage()
    del env["FLIBS"]
    executable("bin/gfortran", "printf 'libgfortran.dylib\\n'")
    assert "FLIBS = " in stage()
            "##,
        )
        .arg(staging)
        .output()
        .expect("run staging test");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
