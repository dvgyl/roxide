//! Exercise the native jump and exception paths under both GC policies.

use std::ffi::{c_int, c_void};
use std::ptr::{from_mut, null_mut};

use roxide_capi::abi::Sexp;
use roxide_capi::cx::{with_ctx_entered, with_full, with_heap, with_session};
use roxide_capi::globals::{R_NilValue, R_TrueValue};
use roxide_capi::jump::{VoidFn, boundary, enter_trampoline};
use roxide_eval::{
    ArgEval, Args, Arity, Call, Ctx, Dispatch, EvalResult, Module, PrimDef, PrimKind, Visibility,
};
use roxide_gc::GcPolicy;
use roxide_host::PureHost;
use roxide_value::{GLOBAL_ENV, SEXP_NIL, SexpType, ValueApi};

#[link(name = "roxide_capi_fixture", kind = "static")]
unsafe extern "C" {
    fn rx_test_error(data: *mut c_void);
    fn rx_test_nested(data: *mut c_void);
    fn rx_test_long_error(data: *mut c_void);
    fn rx_test_print(data: *mut c_void);
    fn rx_test_imbalance(data: *mut c_void);
    fn rx_test_boom(data: *mut c_void);
    fn rx_test_unknown_exception(data: *mut c_void);
    fn rx_test_overflow(data: *mut c_void);
    fn rx_test_protect_index(data: *mut c_void);
    fn rx_test_unprotect_missing(data: *mut c_void);
    fn rx_test_unwind_protect(data: *mut c_void);
    fn rx_test_catch(data: *mut c_void);
    fn rx_test_toplevel(data: *mut c_void);
}

fn each_policy(mut test: impl FnMut(&mut Ctx)) {
    for policy in [GcPolicy::default(), GcPolicy::Torture] {
        let mut ctx = Ctx::new(Box::new(PureHost::new()), policy).unwrap();
        test(&mut ctx);
        assert_eq!(ctx.session().heap.protect_depth(), 0);
    }
}

fn capture(ctx: &mut Ctx, f: impl FnOnce(&mut Ctx)) -> String {
    let (_, bytes) = ctx
        .capture_streams(false, true, false, |ctx| {
            f(ctx);
            Ok(())
        })
        .unwrap();
    String::from_utf8(bytes).unwrap()
}

#[repr(C)]
struct Bridge {
    fun: VoidFn,
    data: *mut c_void,
}

struct RecordDrop<'a>(&'a mut usize);

impl Drop for RecordDrop<'_> {
    fn drop(&mut self) {
        *self.0 += 1;
    }
}

struct Nesting {
    left: usize,
    dropped: usize,
}

unsafe extern "C" fn nested(data: *mut c_void) {
    // SAFETY: The C fixture calls back inside an active trampoline. The
    // boundary consumes and drops all Rust locals before a jump.
    unsafe {
        boundary((), |ctx| {
            let state = &mut *data.cast::<Nesting>();
            state.left -= 1;
            if state.left == 0 {
                let _drop = RecordDrop(&mut state.dropped);
                return enter_trampoline(ctx, rx_test_error, null_mut());
            }
            // Use a separate child state so no parent Rust reference aliases
            // the field the child updates during the callback.
            let mut child = Nesting {
                left: state.left,
                dropped: 0,
            };
            let mut bridge = Bridge {
                fun: nested,
                data: from_mut(&mut child).cast(),
            };
            let _drop = RecordDrop(&mut state.dropped);
            let result = enter_trampoline(ctx, rx_test_nested, from_mut(&mut bridge).cast());
            *_drop.0 += child.dropped;
            result
        });
    }
}

fn nested_fixture(ctx: &mut Ctx, _: &Call, _: &mut Args) -> EvalResult {
    let mut state = Nesting {
        left: 2,
        dropped: 0,
    };
    let mut bridge = Bridge {
        fun: nested,
        data: from_mut(&mut state).cast(),
    };
    let mark = ctx.session().heap.protect_depth();
    // SAFETY: The fixture uses the provided bridge during the call only.
    let result = unsafe { enter_trampoline(ctx, rx_test_nested, from_mut(&mut bridge).cast()) };
    assert_eq!(state.dropped, 2);
    assert_eq!(ctx.session().heap.protect_depth(), mark);
    result.map(|()| SEXP_NIL)
}

#[test]
fn nested_error_reaches_r_try_catch() {
    each_policy(|ctx| {
        ctx.register_module(&Module {
            prims: &[PrimDef {
                name: "nativeFixture",
                kind: PrimKind::Primitive,
                args: ArgEval::Eval,
                arity: Arity::Exact(0),
                dispatch: Dispatch::None,
                visibility: Visibility::On,
                fun: nested_fixture,
            }],
            prelude: "",
        })
        .unwrap();
        let before = ctx.collect_gc().live_objects;
        let stderr = capture(ctx, |ctx| {
            ctx.run_script(
                "native-try-catch",
                "stopifnot(identical(tryCatch(nativeFixture(), error = conditionMessage), 'native error 42'))",
                GLOBAL_ENV,
                false,
            )
            .unwrap();
        });
        assert!(stderr.is_empty(), "{stderr}");
        let after = ctx.collect_gc().live_objects;
        // Parsing can intern new names. The second execution reuses them.
        assert!(after >= before);
        ctx.run_script(
            "native-try-catch",
            "stopifnot(identical(tryCatch(nativeFixture(), error = conditionMessage), 'native error 42'))",
            GLOBAL_ENV,
            false,
        ).unwrap();
        assert_eq!(ctx.collect_gc().live_objects, after);
    });
}

#[repr(C)]
struct CleanupProbe {
    destructed: c_int,
    cleaned: c_int,
    fail: bool,
    cpp_unwind: c_int,
    collect: VoidFn,
    collect_data: *mut c_void,
}

unsafe extern "C" fn collect(_: *mut c_void) {
    with_full(|ctx| {
        ctx.collect_gc();
    });
}

fn condition_message(ctx: &mut Ctx, cond: roxide_gc::SexpId) -> EvalResult {
    let name = ctx.session_mut().intern("conditionMessage")?;
    let fun = ctx.find_fun(name, GLOBAL_ENV)?;
    ctx.apply(fun, &[(None, cond)], SEXP_NIL, GLOBAL_ENV)
}

fn message(ctx: &Ctx, value: roxide_gc::SexpId) -> String {
    let char_id = ctx.session().str_slice(value).unwrap()[0];
    String::from_utf8(ctx.session().char_bytes(char_id).unwrap().to_vec()).unwrap()
}

#[test]
fn cleanup_runs_and_preserves_the_suspended_error_during_gc() {
    each_policy(|ctx| {
        // Warm the condition symbols before checking for retained objects.
        let _ = capture(ctx, |ctx| {
            // SAFETY: The fixture ignores data.
            assert!(unsafe { enter_trampoline(ctx, rx_test_error, null_mut()) }.is_err());
        });
        let mut prior = None;
        for _ in 0..2 {
            for (fail, cpp_unwind, cleaned, destructed) in [
                (false, 1, 101, 1),
                (true, 1, 110, 1),
                (true, 2, 110, 1),
                (false, 0, 1, 0),
                (true, 0, 10, 0),
            ] {
                let mut probe = CleanupProbe {
                    fail,
                    cpp_unwind,
                    cleaned: 0,
                    destructed: 0,
                    collect,
                    collect_data: null_mut(),
                };
                let result = ctx
                    .with_native_error_handler(
                        |ctx| {
                            // SAFETY: The test probe stays live through the C++ callback.
                            unsafe {
                                enter_trampoline(
                                    ctx,
                                    rx_test_unwind_protect,
                                    from_mut(&mut probe).cast(),
                                )?;
                            }
                            Ok(SEXP_NIL)
                        },
                        condition_message,
                    )
                    .unwrap();
                if fail {
                    assert_eq!(message(ctx, result), "unwind body error");
                } else {
                    assert_eq!(result, SEXP_NIL);
                }
                assert_eq!(probe.cleaned, cleaned);
                assert_eq!(probe.destructed, destructed);
                assert_eq!(ctx.session().heap.protect_depth(), 0);
            }
            let count = ctx.collect_gc().live_objects;
            if let Some(prior) = prior {
                assert_eq!(count, prior);
            }
            prior = Some(count);
        }
    });
}

fn cpp_unwind_fixture(ctx: &mut Ctx, _: &Call, _: &mut Args) -> EvalResult {
    let mut probe = CleanupProbe {
        fail: true,
        cpp_unwind: 1,
        cleaned: 0,
        destructed: 0,
        collect,
        collect_data: null_mut(),
    };
    // SAFETY: The fixture uses the probe only during the call.
    let result =
        unsafe { enter_trampoline(ctx, rx_test_unwind_protect, from_mut(&mut probe).cast()) };
    assert_eq!(probe.cleaned, 110);
    assert_eq!(probe.destructed, 1);
    result.map(|()| SEXP_NIL)
}

#[test]
fn continued_unwind_reaches_r_try_catch() {
    each_policy(|ctx| {
        ctx.register_module(&Module {
            prims: &[PrimDef {
                name: "nativeUnwindFixture",
                kind: PrimKind::Primitive,
                args: ArgEval::Eval,
                arity: Arity::Exact(0),
                dispatch: Dispatch::None,
                visibility: Visibility::On,
                fun: cpp_unwind_fixture,
            }],
            prelude: "",
        })
        .unwrap();
        let stderr = capture(ctx, |ctx| {
            ctx.run_script(
                "native-unwind-try-catch",
                "stopifnot(identical(tryCatch(nativeUnwindFixture(), error = conditionMessage), 'unwind body error'))",
                GLOBAL_ENV,
                false,
            )
            .unwrap();
        });
        assert!(stderr.is_empty(), "{stderr}");
    });
}

#[test]
fn catches_cpp_exceptions() {
    each_policy(|ctx| {
        for (callback, expected) in [
            (rx_test_boom as VoidFn, "boom"),
            (rx_test_unknown_exception as VoidFn, "unknown C++ exception"),
        ] {
            let result = ctx
                .with_native_error_handler(
                    |ctx| {
                        // SAFETY: Both C++ fixtures ignore the data.
                        unsafe { enter_trampoline(ctx, callback, null_mut())? };
                        Ok(SEXP_NIL)
                    },
                    condition_message,
                )
                .unwrap();
            assert_eq!(
                message(ctx, result),
                format!("C++ exception escaped native routine: {expected}")
            );
        }
    });
}

#[test]
fn protect_limit_indexes_removal_and_balance() {
    each_policy(|ctx| {
        ctx.session_mut().heap.set_protect_limit(1);
        let stderr = capture(ctx, |ctx| {
            // SAFETY: The test callbacks ignore the data.
            assert!(unsafe { enter_trampoline(ctx, rx_test_overflow, null_mut()) }.is_err());
            assert_eq!(ctx.session().heap.protect_depth(), 0);
            assert!(
                unsafe { enter_trampoline(ctx, rx_test_unprotect_missing, null_mut()) }.is_err()
            );
            ctx.session_mut().heap.set_protect_limit(50_000);
            unsafe { enter_trampoline(ctx, rx_test_protect_index, null_mut()) }.unwrap();
            unsafe { enter_trampoline(ctx, rx_test_imbalance, null_mut()) }.unwrap();
        });
        assert_eq!(
            stderr.lines().collect::<Vec<_>>(),
            [
                "Error: protect(): protection stack overflow",
                "Error: unprotect_ptr: pointer not found",
                "Warning: stack imbalance in '.Call', 0 then 1",
            ]
        );
    });
}

#[test]
fn native_toplevel_isolates_handlers_and_native_try_catch_gets_condition() {
    each_policy(|ctx| {
        let mut flags = [-1 as c_int; 2];
        let stderr = capture(ctx, |ctx| {
            // The outer handler must not see the error from R_ToplevelExec.
            let result = ctx.with_native_error_handler(
                |ctx| {
                    // SAFETY: The fixture writes exactly two C ints.
                    unsafe { enter_trampoline(ctx, rx_test_toplevel, flags.as_mut_ptr().cast())? };
                    Ok(SEXP_NIL)
                },
                |_, _| Ok(R_TrueValue.to_id().unwrap()),
            );
            assert_eq!(result, Ok(SEXP_NIL));
        });
        assert_eq!(flags, [0, 1]);
        assert_eq!(stderr, "Error: native error 42\n");

        let mut values = [Sexp::NULL; 2];
        // SAFETY: The fixture writes a return value and condition to this buffer.
        unsafe { enter_trampoline(ctx, rx_test_catch, values.as_mut_ptr().cast()) }.unwrap();
        assert_eq!(values[0], R_TrueValue);
        let result = condition_message(ctx, values[1].to_id().unwrap()).unwrap();
        assert_eq!(message(ctx, result), "native error 42");
    });
}

#[test]
fn reborrows_context_and_formats_long_messages() {
    each_policy(|ctx| {
        with_ctx_entered(ctx, || {
            with_heap(|heap| assert!(heap.get(SEXP_NIL).is_ok()));
            with_session(|session| assert_eq!(session.type_of(SEXP_NIL).unwrap(), SexpType::Nil));
            with_full(|ctx| {
                with_ctx_entered(ctx, || {
                    with_heap(|heap| assert_eq!(heap.protect_depth(), 0));
                })
            });
        });
        let (_, printed) = ctx
            .capture_streams(true, true, false, |ctx| {
                // SAFETY: The fixture ignores its data.
                unsafe { enter_trampoline(ctx, rx_test_print, null_mut()) }
            })
            .unwrap();
        assert_eq!(printed.len(), 9000 + "output 42".len());
        assert!(printed.ends_with(b"1output 42"));

        let result = ctx
            .with_native_error_handler(
                |ctx| {
                    // SAFETY: The fixture ignores its data.
                    unsafe { enter_trampoline(ctx, rx_test_long_error, null_mut())? };
                    Ok(SEXP_NIL)
                },
                condition_message,
            )
            .unwrap();
        let result = message(ctx, result);
        assert_eq!(result.len(), 8191);
        assert!(result.bytes().all(|byte| byte == b'0'));
        assert_eq!(R_NilValue.to_id(), Some(SEXP_NIL));
    });
}

unsafe extern "C" fn panic_in_api(_: *mut c_void) {
    // SAFETY: The fixture calls this through an active trampoline.
    unsafe { boundary((), |_| panic!("panic in native API")) }
}

#[test]
fn rust_panic_becomes_internal_unwind() {
    each_policy(|ctx| {
        let mut bridge = Bridge {
            fun: panic_in_api,
            data: null_mut(),
        };
        // SAFETY: The C fixture uses this bridge during the call only.
        let result = unsafe { enter_trampoline(ctx, rx_test_nested, from_mut(&mut bridge).cast()) };
        assert!(matches!(
            result,
            Err(roxide_eval::Unwind::Internal(error))
                if error.message == "internal error in Roxide C API: panic in native API"
        ));
        assert_eq!(ctx.session().heap.protect_depth(), 0);
        // SAFETY: The test callback ignores its data.
        assert!(unsafe { enter_trampoline(ctx, rx_test_print, null_mut()) }.is_ok());
    });
}
