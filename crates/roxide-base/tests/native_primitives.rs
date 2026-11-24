#![forbid(unsafe_code)]

//! Native calls through the evaluator with a mock native hook.

use std::path::Path;

use roxide_eval::{Args, Call, Ctx, CtxExt, DotCOpts, EvalResult, NativeCallKind, NativeHooks};
use roxide_gc::{GcPolicy, SexpId, Trace, Tracer};
use roxide_host::PureHost;
use roxide_value::{BASE_ENV, GLOBAL_ENV, SEXP_NIL, ValueApi};

struct Mock;

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
        let names: Vec<_> = args
            .iter()
            .map(|(tag, _)| tag.map(|tag| ctx.session().sym_name(tag).expect("read tag")))
            .collect();
        let (expected_names, expected_package, expected_options) = match kind {
            NativeCallKind::Call | NativeCallKind::External | NativeCallKind::External2 => {
                (vec![Some("NAOK"), Some("PACK"), None], None, None)
            }
            NativeCallKind::C => (
                vec![Some("x"), None],
                Some("demo"),
                Some(DotCOpts {
                    naok: true,
                    dup: false,
                    encoding: Some("UTF-8".into()),
                }),
            ),
            NativeCallKind::Fortran => (vec![Some("x"), None], None, Some(DotCOpts::default())),
        };
        assert_eq!(names, expected_names);
        for (i, value) in args.values().iter().enumerate() {
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
            _ => Err(ctx.stop(call, "symbol is unavailable")),
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
    run(
        &mut context(),
        r#"
        check <- function() {
            marker <- new.env()
            stopifnot(identical(.Call(marker, NAOK=1L, PACK=2L, 3L, PACKAGE=""), marker))
            marker <- list(name="native", address=NULL)
            stopifnot(identical(.External(NAOK=1L, PACK=2L, 3L, .NAME=marker), marker))
            external2 <- .External2
            .External2 <- NULL
            stopifnot(identical(external2(marker, NAOK=1L, PACK=2L, 3L), marker))
            marker <- "native"
            stopifnot(identical(.C(marker, x=1L, 2L, DUP=0, NAOK="TRUE",
                                   ENCODING="UTF-8", PACKAGE="demo"), marker))
            stopifnot(identical(.Fortran(marker, x=1L, 2L), marker))
        }
        check()
        "#,
    );
}

#[test]
fn invalid_native_controls_raise_r_errors() {
    run(
        &mut context(),
        r#"
        fails <- function(x) inherits(tryCatch(x, error=identity), "error")
        stopifnot(fails(.Call()), fails(.Call(.NAME=)),
                  fails(.Call("one", PACKAGE=NA_character_)), fails(.Call("one", PACKAGE=)),
                  fails(.Call("one", PACKAGE=c("demo","other"))),
                  fails(.C("one", NAOK=NA)), fails(.C("one", DUP=integer())),
                  fails(.Fortran("one", ENCODING=c("UTF-8","ASCII"))),
                  fails(is.loaded("one", type="unknown")),
                  fails(getNativeSymbolInfo(NA_character_)))
        "#,
    );
}

#[test]
fn dll_wrappers_accessors_and_print_methods() {
    let mut ctx = context();
    run(
        &mut ctx,
        r#"
        path <- "/packages/demo/libs/demo.so"
        mock_dll <- structure(list(name="demo", path=path, dynamicLookup=FALSE), class="DLLInfo")
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
                  !is.loaded("one", "demo", "Fortran"))
        stopifnot(identical(getNativeSymbolInfo("one"), one),
                  identical(getNativeSymbolInfo("one", mock_dll, withRegistrationInfo=TRUE), registered_one),
                  identical(getNativeSymbolInfo(c("two","one"), "demo"), list(two=two, one=one)),
                  identical(getNativeSymbolInfo("one", "demo", unlist=FALSE), list(one=one)),
                  length(getNativeSymbolInfo(character())) == 0L)
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
        old <- withVisible(.dynLibs(list(mock_dll)))
        stopifnot(!old$visible, identical(old$value, empty), identical(.dynLibs()[[1L]], mock_dll))
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
