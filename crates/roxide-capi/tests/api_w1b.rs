//! Check the Wave 1-B C operations under both garbage collection policies.

use std::ffi::{CStr, c_int, c_void};
use std::ptr::{from_mut, null_mut};

use roxide_capi::abi::Sexp;
use roxide_capi::api::{env::*, error::*, eval::*, parse::*, print::*, unwind::*};
use roxide_capi::globals::{R_BaseEnv, R_EmptyEnv, R_GlobalEnv, R_NilValue, R_UnboundValue};
use roxide_capi::jump::{Rf_protect, Rf_unprotect, VoidFn, enter_trampoline};
use roxide_eval::{
    ArgEval, Args, Arity, Call, Ctx, Dispatch, EvalResult, Module, PrimDef, PrimKind, Visibility,
};
use roxide_gc::{GcPolicy, SexpId};
use roxide_host::PureHost;
use roxide_value::{BASE_ENV, GLOBAL_ENV, SYM_SRCFILE, SYM_SRCREF, SexpType, ValueApi};

fn each_policy(mut test: impl FnMut(&mut Ctx)) {
    for policy in [GcPolicy::default(), GcPolicy::Torture] {
        let mut ctx = Ctx::new(Box::new(PureHost::new()), policy).unwrap();
        test(&mut ctx);
        assert_eq!(ctx.session().heap.protect_depth(), 0);
    }
}

fn handle(id: SexpId) -> Sexp {
    Sexp::from_id(id)
}

fn symbol(ctx: &mut Ctx, name: &str) -> Sexp {
    handle(ctx.session_mut().intern(name).unwrap().as_sexp())
}

fn text(ctx: &mut Ctx, value: &str) -> Sexp {
    handle(ctx.session_mut().scalar_str(value).unwrap())
}

fn int(ctx: &Ctx, value: Sexp) -> i32 {
    ctx.session().int(value.to_id().unwrap()).unwrap().get(0)
}

fn strings(ctx: &Ctx, value: Sexp) -> Vec<String> {
    ctx.session()
        .str_slice(value.to_id().unwrap())
        .unwrap()
        .iter()
        .map(|&s| {
            std::str::from_utf8(ctx.session().char_bytes(s).unwrap())
                .unwrap()
                .to_owned()
        })
        .collect()
}

fn call<T>(ctx: &mut Ctx, fun: VoidFn, data: &mut T) -> EvalResult<()> {
    // SAFETY: Each test callback uses its own fixture type. It holds no Rust
    // values that require destruction across a foreign jump.
    unsafe { enter_trampoline(ctx, fun, from_mut(data).cast()) }
}

struct EnvProbe {
    name: Sexp,
    active: Sexp,
    hidden: Sexp,
    value: Sexp,
    getter: Sexp,
    parent: Sexp,
    inherited: Sexp,
    fallback: Sexp,
    updated: Sexp,
    active_value: Sexp,
    active_fun: Sexp,
    names: Sexp,
    all_names: Sexp,
    exists_before: c_int,
    exists_after: c_int,
    binding_locked: c_int,
    binding_active: c_int,
    env_locked: c_int,
}

unsafe extern "C" fn env_probe(data: *mut c_void) {
    let p = unsafe { &mut *data.cast::<EnvProbe>() };
    unsafe {
        let env = Rf_protect(R_NewEnv(R_EmptyEnv, 0, 0));
        let child = Rf_protect(R_NewEnv(env, 1, 7));
        p.parent = R_ParentEnv(child);
        Rf_defineVar(p.name, p.value, env);
        Rf_defineVar(p.hidden, p.value, env);
        p.inherited = R_getVar(p.name, child, 1);
        p.fallback = R_getVarEx(p.name, child, 0, R_NilValue);
        Rf_setVar(p.name, R_NilValue, child);
        p.updated = R_getVar(p.name, env, 0);
        R_LockBinding(p.name, env);
        p.binding_locked = R_BindingIsLocked(p.name, env);
        R_unLockBinding(p.name, env);
        p.exists_before = R_existsVarInFrame(env, p.name);
        R_removeVarFromFrame(p.name, env);
        p.exists_after = R_existsVarInFrame(env, p.name);
        let getter = Rf_protect(R_ParseEvalString(c"function() 42L".as_ptr(), R_BaseEnv));
        R_MakeActiveBinding(p.active, getter, env);
        p.active_fun = R_ActiveBindingFunction(p.active, env);
        p.binding_active = R_BindingIsActive(p.active, env);
        p.active_value = Rf_protect(R_getVar(p.active, child, 1));
        p.names = Rf_protect(R_lsInternal3(env, 0, 1));
        p.all_names = R_lsInternal3(env, 1, 1);
        R_LockEnvironment(env, 1);
        p.env_locked = R_EnvironmentIsLocked(env);
        p.getter = getter;
        Rf_unprotect(5);
    }
}

#[test]
fn environment_operations() {
    each_policy(|ctx| {
        let mut p = EnvProbe {
            name: symbol(ctx, "ordinary"),
            active: symbol(ctx, "active"),
            hidden: symbol(ctx, ".hidden"),
            value: text(ctx, "bound"),
            getter: Sexp::NULL,
            parent: Sexp::NULL,
            inherited: Sexp::NULL,
            fallback: Sexp::NULL,
            updated: Sexp::NULL,
            active_value: Sexp::NULL,
            active_fun: Sexp::NULL,
            names: Sexp::NULL,
            all_names: Sexp::NULL,
            exists_before: -1,
            exists_after: -1,
            binding_locked: -1,
            binding_active: -1,
            env_locked: -1,
        };
        ctx.scoped(|ctx| {
            ctx.protect(p.value.to_id().unwrap());
            call(ctx, env_probe, &mut p)?;
            assert_eq!(
                ctx.session().type_of(p.parent.to_id().unwrap())?,
                SexpType::Env
            );
            assert_eq!(p.inherited, p.value);
            assert_eq!(p.fallback, R_NilValue);
            assert_eq!(p.updated, R_NilValue);
            assert_eq!(p.exists_before, 1);
            assert_eq!(p.exists_after, 0);
            assert_eq!(p.binding_locked, 1);
            assert_eq!(p.binding_active, 1);
            assert_eq!(p.env_locked, 1);
            assert_eq!(p.active_fun, p.getter);
            assert_eq!(int(ctx, p.active_value), 42);
            assert_eq!(strings(ctx, p.names), ["active"]);
            assert_eq!(strings(ctx, p.all_names), [".hidden", "active"]);
            Ok(())
        })
        .unwrap();
    });
}

unsafe extern "C" fn locked_write(data: *mut c_void) {
    let name = unsafe { *data.cast::<Sexp>() };
    unsafe {
        let env = Rf_protect(R_NewEnv(R_EmptyEnv, 0, 0));
        Rf_defineVar(name, R_NilValue, env);
        R_LockBinding(name, env);
        Rf_defineVar(name, R_UnboundValue, env);
        Rf_unprotect(1);
    }
}

unsafe extern "C" fn null_environment(data: *mut c_void) {
    let name = unsafe { *data.cast::<Sexp>() };
    unsafe { R_getVar(name, Sexp::NULL, 0) };
}

#[test]
fn invalid_environment_operations_jump() {
    each_policy(|ctx| {
        let mut name = symbol(ctx, "locked");
        let ((), stderr) = ctx
            .capture_streams(false, true, false, |ctx| {
                assert!(call(ctx, locked_write, &mut name).is_err());
                assert!(call(ctx, null_environment, &mut name).is_err());
                Ok(())
            })
            .unwrap();
        let stderr = String::from_utf8(stderr).unwrap();
        assert!(stderr.contains("locked binding"), "{stderr}");
        assert!(stderr.contains("invalid SEXP handle"), "{stderr}");
    });
}

struct EvalProbe {
    digits: Sexp,
    parts: [Sexp; 3],
    expected: [Sexp; 3],
    result: Sexp,
    forced: Sexp,
    option: Sexp,
    current: Sexp,
}

unsafe extern "C" fn eval_probe(data: *mut c_void) {
    let p = unsafe { &mut *data.cast::<EvalProbe>() };
    unsafe {
        let original = Rf_protect(R_ParseEvalString(
            c"function(x = 5L) x + 1L".as_ptr(),
            R_BaseEnv,
        ));
        p.expected = [
            R_ClosureFormals(original),
            R_ClosureBody(original),
            R_BaseEnv,
        ];
        let copy = Rf_protect(R_mkClosure(p.expected[0], p.expected[1], p.expected[2]));
        p.parts = [
            R_ClosureFormals(copy),
            R_ClosureExpr(copy),
            R_ClosureEnv(copy),
        ];
        let expression = Rf_protect(R_ParseString(c"2L + 3L".as_ptr()));
        p.result = Rf_protect(Rf_eval(expression, R_BaseEnv));
        Rf_PrintValue(p.result);
        R_FlushConsole();
        let forced = Rf_protect(R_ParseString(
            c"(function(x) 8L)(stop('force this'))".as_ptr(),
        ));
        let no_error = Rf_protect(R_forceAndCall(forced, 0, R_BaseEnv));
        p.forced = no_error;
        p.option = Rf_GetOption1(p.digits);
        p.current = R_GetCurrentEnv();
        Rf_unprotect(6);
    }
}

unsafe extern "C" fn current_env_probe(data: *mut c_void) {
    unsafe { data.cast::<Sexp>().write(R_GetCurrentEnv()) };
}

fn native_current_env(ctx: &mut Ctx, _: &Call, _: &mut Args) -> EvalResult {
    let mut result = Sexp::NULL;
    call(ctx, current_env_probe, &mut result)?;
    Ok(result.to_id().unwrap())
}

#[test]
fn evaluation_options_print_and_current_environment() {
    each_policy(|ctx| {
        let mut p = EvalProbe {
            digits: symbol(ctx, "digits"),
            parts: [Sexp::NULL; 3],
            expected: [Sexp::NULL; 3],
            result: Sexp::NULL,
            forced: Sexp::NULL,
            option: Sexp::NULL,
            current: Sexp::NULL,
        };
        let ((), out) = ctx
            .capture_streams(true, false, false, |ctx| call(ctx, eval_probe, &mut p))
            .unwrap();
        assert_eq!(p.parts, p.expected);
        assert_eq!(int(ctx, p.result), 5);
        assert_eq!(int(ctx, p.forced), 8);
        assert_eq!(int(ctx, p.option), ctx.opts().digits as i32);
        assert_eq!(p.current, R_GlobalEnv);
        assert_eq!(out, b"[1] 5\n");

        ctx.register_module(&Module {
            prims: &[PrimDef {
                name: "nativeCurrentEnv",
                kind: PrimKind::Primitive,
                args: ArgEval::Eval,
                arity: Arity::Exact(0),
                dispatch: Dispatch::None,
                visibility: Visibility::On,
                fun: native_current_env,
            }],
            prelude: "",
        })
        .unwrap();
        ctx.run_script(
            "current-env",
            "(function() stopifnot(identical(environment(), nativeCurrentEnv())))()",
            GLOBAL_ENV,
            false,
        )
        .unwrap();
    });
}

struct ParseProbe {
    text: Sexp,
    count: c_int,
    srcfile: Sexp,
    status: c_int,
    result: Sexp,
}

unsafe extern "C" fn parse_probe(data: *mut c_void) {
    let p = unsafe { &mut *data.cast::<ParseProbe>() };
    p.result = unsafe { R_ParseVector(p.text, p.count, &mut p.status, p.srcfile) };
}

#[test]
fn parser_status_counts_and_source_refs() {
    each_policy(|ctx| {
        for (source, count, status, length) in [
            ("1L; 2L\n3L", -1, 1, 3),
            ("1L; 2L; 3 +", 2, 1, 2),
            ("1L; (", 0, 1, 0),
            ("", -1, 1, 0),
            ("1 +", -1, 2, 0),
            ("1 )", -1, 3, 0),
        ] {
            let mut p = ParseProbe {
                text: text(ctx, source),
                count,
                srcfile: R_NilValue,
                status: -1,
                result: Sexp::NULL,
            };
            call(ctx, parse_probe, &mut p).unwrap();
            assert_eq!(p.status, status, "{source:?}");
            if status == 1 {
                let id = p.result.to_id().unwrap();
                assert_eq!(ctx.session().type_of(id).unwrap(), SexpType::Expr);
                assert_eq!(ctx.session().xlength(id).unwrap(), length);
            } else {
                assert_eq!(p.result, R_NilValue);
            }
        }
        let env = ctx.session_mut().alloc_env(BASE_ENV).unwrap();
        let mut p = ParseProbe {
            text: text(ctx, "1L\n2L"),
            count: -1,
            srcfile: handle(env),
            status: -1,
            result: Sexp::NULL,
        };
        call(ctx, parse_probe, &mut p).unwrap();
        let result = p.result.to_id().unwrap();
        assert_eq!(p.status, 1);
        assert_eq!(ctx.session().attr(result, SYM_SRCFILE), env);
        let refs = ctx.session().attr(result, SYM_SRCREF);
        assert_eq!(ctx.session().xlength(refs).unwrap(), 2);
        let second = ctx.session().list_get(refs, 1).unwrap();
        assert_eq!(ctx.session().int(second).unwrap().get(0), 2);
        assert_eq!(ctx.session().attr(second, SYM_SRCFILE), env);
    });
}

struct TryProbe {
    silent: bool,
    successes: [Sexp; 2],
    errors: [c_int; 2],
    diagnostic: bool,
}

unsafe extern "C" fn try_probe(data: *mut c_void) {
    let p = unsafe { &mut *data.cast::<TryProbe>() };
    unsafe {
        let expr = Rf_protect(R_ParseString(c"9L".as_ptr()));
        let fail = Rf_protect(R_ParseString(
            c"{ message('test message'); stop('native test error') }".as_ptr(),
        ));
        let eval = if p.silent { R_tryEvalSilent } else { R_tryEval };
        p.successes[0] = Rf_protect(eval(expr, R_BaseEnv, &mut p.errors[0]));
        p.successes[1] = eval(fail, R_BaseEnv, &mut p.errors[1]);
        let diagnostic = CStr::from_ptr(R_curErrorBuf()).to_bytes();
        p.diagnostic = diagnostic
            .windows(b"native test error".len())
            .any(|s| s == b"native test error");
        Rf_unprotect(3);
    }
}

#[test]
fn try_eval_success_and_diagnostics() {
    each_policy(|ctx| {
        for silent in [false, true] {
            let mut p = TryProbe {
                silent,
                successes: [Sexp::NULL; 2],
                errors: [-1; 2],
                diagnostic: false,
            };
            let ((), stderr) = ctx
                .capture_streams(false, true, false, |ctx| call(ctx, try_probe, &mut p))
                .unwrap();
            let stderr = String::from_utf8(stderr).unwrap();
            assert_eq!(p.errors, [0, 1]);
            assert_eq!(int(ctx, p.successes[0]), 9);
            assert_eq!(p.successes[1], Sexp::NULL);
            assert!(p.diagnostic);
            assert_eq!(stderr.contains("native test error"), !silent, "{stderr}");
            assert!(stderr.contains("test message"), "{stderr}");
        }
    });
}

struct CleanupProbe {
    mode: u8,
    cleaned: u8,
    handled: u8,
    result: Sexp,
}

unsafe extern "C" fn return_value(_: *mut c_void) -> Sexp {
    unsafe { R_ParseEvalString(c"123L".as_ptr(), R_BaseEnv) }
}

unsafe extern "C" fn raise_error(_: *mut c_void) -> Sexp {
    unsafe { R_ParseEvalString(c"stop('body error')".as_ptr(), R_BaseEnv) }
}

unsafe extern "C" fn raise_warning(_: *mut c_void) -> Sexp {
    unsafe { R_ParseEvalString(c"{ warning('body warning'); 87L }".as_ptr(), R_BaseEnv) }
}

unsafe extern "C" fn clean(data: *mut c_void) {
    unsafe {
        (*data.cast::<CleanupProbe>()).cleaned += 1;
        R_ParseEvalString(c"invisible(789L)".as_ptr(), R_BaseEnv);
    }
}

unsafe extern "C" fn record_condition(cond: Sexp, data: *mut c_void) -> Sexp {
    unsafe {
        (*data.cast::<CleanupProbe>()).handled += 1;
        cond
    }
}

unsafe extern "C" fn cleanup_probe(data: *mut c_void) {
    unsafe {
        let mode = (*data.cast::<CleanupProbe>()).mode;
        let result = match mode {
            0 => R_ExecWithCleanup(Some(return_value), null_mut(), Some(clean), data),
            1 => R_ExecWithCleanup(Some(raise_error), null_mut(), Some(clean), data),
            2 => {
                let classes = Rf_protect(R_ParseEvalString(c"'warning'".as_ptr(), R_BaseEnv));
                let result = R_tryCatch(
                    Some(raise_warning),
                    null_mut(),
                    classes,
                    Some(record_condition),
                    data,
                    Some(clean),
                    data,
                );
                Rf_unprotect(1);
                result
            }
            _ => R_withCallingErrorHandler(
                Some(raise_error),
                null_mut(),
                Some(record_condition),
                data,
            ),
        };
        (*data.cast::<CleanupProbe>()).result = result;
    }
}

#[test]
fn callback_cleanup_and_condition_handlers() {
    each_policy(|ctx| {
        for (mode, cleaned, handled, failed) in [
            (0, 1, 0, false),
            (1, 1, 0, true),
            (2, 1, 1, false),
            (3, 0, 1, true),
        ] {
            let mut p = CleanupProbe {
                mode,
                cleaned: 0,
                handled: 0,
                result: Sexp::NULL,
            };
            let ((), stderr) = ctx
                .capture_streams(false, true, false, |ctx| {
                    assert_eq!(
                        call(ctx, cleanup_probe, &mut p).is_err(),
                        failed,
                        "mode {mode}"
                    );
                    Ok(())
                })
                .unwrap();
            assert_eq!(p.cleaned, cleaned, "mode {mode}");
            assert_eq!(p.handled, handled, "mode {mode}");
            if mode == 0 {
                assert_eq!(int(ctx, p.result), 123);
            } else if mode == 2 {
                assert_eq!(
                    ctx.session().type_of(p.result.to_id().unwrap()).unwrap(),
                    SexpType::Vec
                );
            }
            assert_eq!(
                String::from_utf8(stderr).unwrap().contains("body error"),
                failed
            );
        }
    });
}
