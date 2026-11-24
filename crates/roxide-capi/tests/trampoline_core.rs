//! Exercise the native jump and exception paths under both GC policies.

use std::ffi::{c_char, c_int, c_void};
use std::ptr::{from_mut, null_mut};

use roxide_capi::abi::Sexp;
use roxide_capi::api::memory::{R_alloc, vmaxget};
use roxide_capi::cx::{with_ctx_entered, with_full, with_heap, with_session};
use roxide_capi::globals::{R_NilValue, R_TrueValue};
use roxide_capi::jump::{
    FrameKind, Rf_protect, Rf_unprotect, VoidFn, boundary, enter_trampoline, enter_trampoline_with,
    set_altrep_resolver,
};
use roxide_eval::{
    ArgEval, Args, Arity, Call, Ctx, Dispatch, EvalResult, Module, PrimDef, PrimKind, Visibility,
};
use roxide_gc::{GcPolicy, SexpId};
use roxide_host::PureHost;
use roxide_value::{AltNeed, GLOBAL_ENV, SEXP_NIL, SexpType, ValueApi};

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
    fn rx_test_make_cont(data: *mut c_void);
    fn rx_test_callback_keeps_protect(data: *mut c_void);
    fn rx_test_nested_token_reuse(data: *mut c_void);
    fn rx_test_warning(data: *mut c_void);
    fn rx_test_errorcall(data: *mut c_void);
    fn rx_test_invalid_continue(data: *mut c_void);
    fn rx_test_error_in_cleanup(data: *mut c_void);
    fn rx_test_utf8_long_error(data: *mut c_void);
    fn rx_test_big_print(data: *mut c_void);
    fn rx_test_double_continue(data: *mut c_void);
}

unsafe extern "C" {
    fn rox_capi_alt_fill_int_elt(data: *mut c_void);
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

unsafe extern "C" fn method_allocates(data: *mut c_void) {
    // SAFETY: The caller supplies writable pointer storage and an active method frame.
    unsafe {
        Rf_protect(R_TrueValue);
        let storage = R_alloc(1, 1);
        storage.write(42);
        data.cast::<*mut c_char>().write(storage);
    }
}

unsafe extern "C" fn method_caller(data: *mut c_void) {
    let mut storage = null_mut::<c_char>();
    // SAFETY: The caller supplies a native entry frame and a writable result byte.
    unsafe {
        Rf_protect(R_TrueValue);
        boundary((), |ctx| {
            let mark = ctx.session().heap.protect_depth();
            enter_trampoline_with(
                ctx,
                FrameKind::Method,
                method_allocates,
                from_mut(&mut storage).cast(),
            )?;
            assert_eq!(ctx.session().heap.protect_depth(), mark);
            Ok(())
        });
        // Check that the arena still owns the allocation before reading it.
        if !storage.is_null() && vmaxget() == storage.cast() {
            data.cast::<c_char>().write(storage.read());
        }
        Rf_unprotect(1);
    }
}

#[test]
fn method_restores_protections_silently_and_keeps_transient_storage() {
    each_policy(|ctx| {
        let mut saved: c_char = 0;
        let stderr = capture(ctx, |ctx| {
            // SAFETY: The result byte is writable for the duration of the call.
            unsafe { enter_trampoline(ctx, method_caller, from_mut(&mut saved).cast()) }.unwrap();
        });
        assert!(stderr.is_empty(), "{stderr}");
        assert_eq!(saved, 42);
    });
}

#[repr(C)]
struct FillIntElt {
    fun: extern "C" fn(Sexp, isize) -> c_int,
    x: Sexp,
    len: isize,
    out: *mut c_int,
}

extern "C" fn int_elt(x: Sexp, index: isize) -> c_int {
    if x == R_TrueValue {
        10 + index as c_int
    } else {
        0
    }
}

#[test]
fn bulk_int_elt_fills_the_buffer_in_one_method_frame() {
    each_policy(|ctx| {
        let mut out = [-1; 5];
        let mut call = FillIntElt {
            fun: int_elt,
            x: R_TrueValue,
            len: 4,
            out: out.as_mut_ptr(),
        };
        // SAFETY: The call record matches the C layout and the buffer has four writable elements.
        unsafe {
            enter_trampoline_with(
                ctx,
                FrameKind::Method,
                rox_capi_alt_fill_int_elt,
                from_mut(&mut call).cast(),
            )
        }
        .unwrap();
        assert_eq!(out, [10, 11, 12, 13, -1]);
    });
}

struct AltrepProbe {
    id: SexpId,
    need: AltNeed,
    attempts: usize,
    result: usize,
}

unsafe extern "C" fn probe_altrep(data: *mut c_void) {
    // SAFETY: The caller supplies a live probe and an active trampoline.
    unsafe {
        let result = boundary(0, |ctx| {
            let probe = &mut *data.cast::<AltrepProbe>();
            probe.attempts += 1;
            match probe.need {
                AltNeed::Length => Ok(ctx.session().xlength(probe.id)?),
                AltNeed::Payload => Ok(ctx.session().int(probe.id)?.iter().count()),
            }
        });
        (*data.cast::<AltrepProbe>()).result = result;
    }
}

fn seal_altrep_length(ctx: &mut Ctx, id: SexpId, _: AltNeed) -> EvalResult<()> {
    ctx.session_mut().seal_foreign_alt_len(id, 3)?;
    Ok(())
}

fn resolve_altrep_probe(ctx: &mut Ctx, id: SexpId, need: AltNeed) -> EvalResult<()> {
    match need {
        AltNeed::Length => seal_altrep_length(ctx, id, need),
        AltNeed::Payload => {
            let owned = ctx.session_mut().alloc_int_from(&[10, 11, 12])?;
            ctx.session_mut().adopt_foreign_alt_payload(id, owned)?;
            Ok(())
        }
    }
}

fn alloc_altrep_probe(ctx: &mut Ctx, need: AltNeed) -> AltrepProbe {
    let session = ctx.session_mut();
    let class = session.alloc_extptr(0, SEXP_NIL, SEXP_NIL).unwrap();
    session.heap.protect(class);
    let id = session
        .alloc_foreign_alt(class, SexpType::Int, SEXP_NIL, SEXP_NIL)
        .unwrap();
    session.heap.unprotect(1).unwrap();
    AltrepProbe {
        id,
        need,
        attempts: 0,
        result: 0,
    }
}

#[test]
fn boundary_retries_once_after_length_or_payload_becomes_available() {
    each_policy(|ctx| {
        set_altrep_resolver(ctx, Some(resolve_altrep_probe));
        for need in [AltNeed::Length, AltNeed::Payload] {
            let mut probe = alloc_altrep_probe(ctx, need);
            // SAFETY: The probe stays live during the call.
            unsafe { enter_trampoline(ctx, probe_altrep, from_mut(&mut probe).cast()) }.unwrap();
            assert_eq!(probe.attempts, 2);
            assert_eq!(probe.result, 3);
        }
    });
}

#[test]
fn boundary_signals_an_error_when_the_payload_stays_lazy() {
    each_policy(|ctx| {
        for resolver in [
            None,
            Some(seal_altrep_length as roxide_capi::jump::AltrepResolver),
        ] {
            set_altrep_resolver(ctx, resolver);
            let mut probe = alloc_altrep_probe(ctx, AltNeed::Payload);
            let msg = ctx
                .with_native_error_handler(
                    |ctx| {
                        // SAFETY: The probe stays live during the call.
                        unsafe {
                            enter_trampoline(ctx, probe_altrep, from_mut(&mut probe).cast())?
                        };
                        Ok(SEXP_NIL)
                    },
                    condition_message,
                )
                .unwrap();
            assert_eq!(probe.attempts, 1);
            assert_eq!(
                message(ctx, msg),
                "cannot access the elements of this ALTREP vector"
            );
        }
    });
}

unsafe extern "C" fn probe_altrep_length_then_payload(data: *mut c_void) {
    // SAFETY: The caller supplies a live probe and an active trampoline.
    unsafe {
        boundary((), |ctx| {
            let probe = &mut *data.cast::<AltrepProbe>();
            probe.attempts += 1;
            ctx.session().xlength(probe.id)?;
            ctx.session().int(probe.id)?;
            Ok(())
        });
    }
}

#[test]
fn boundary_resolves_length_and_payload_in_one_operation() {
    each_policy(|ctx| {
        set_altrep_resolver(ctx, Some(resolve_altrep_probe));
        let mut probe = alloc_altrep_probe(ctx, AltNeed::Length);
        // SAFETY: The probe stays live during the call.
        unsafe {
            enter_trampoline(
                ctx,
                probe_altrep_length_then_payload,
                from_mut(&mut probe).cast(),
            )
        }
        .unwrap();
        assert_eq!(probe.attempts, 3);
        assert_eq!(
            ctx.session().int(probe.id).unwrap().as_slice(),
            Some([10, 11, 12].as_slice())
        );
    });
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
        assert_eq!(result.len(), 999);
        assert!(result.bytes().all(|byte| byte == b'0'));
        assert_eq!(R_NilValue.to_id(), Some(SEXP_NIL));

        // UTF-8 boundary truncation: 4-byte emojis truncated to <= 999 -> 996 valid UTF-8 bytes.
        let utf8_msg = ctx
            .with_native_error_handler(
                |ctx| {
                    // SAFETY: Fixture ignores data.
                    unsafe { enter_trampoline(ctx, rx_test_utf8_long_error, null_mut())? };
                    Ok(SEXP_NIL)
                },
                condition_message,
            )
            .unwrap();
        let utf8_msg = message(ctx, utf8_msg);
        assert_eq!(utf8_msg.len(), 996);
        assert!(utf8_msg.chars().all(|c| c == '\u{1F600}'));

        // options(warning.length = 200) -> 199 bytes.
        ctx.run_script("wl", "options(warning.length = 200L)", GLOBAL_ENV, false)
            .unwrap();
        let short_msg = ctx
            .with_native_error_handler(
                |ctx| {
                    // SAFETY: Fixture ignores data.
                    unsafe { enter_trampoline(ctx, rx_test_long_error, null_mut())? };
                    Ok(SEXP_NIL)
                },
                condition_message,
            )
            .unwrap();
        assert_eq!(message(ctx, short_msg).len(), 199);

        // Large Rprintf (> 1 MiB) is not truncated.
        let (_, big) = ctx
            .capture_streams(true, false, false, |ctx| {
                // SAFETY: Fixture ignores data.
                unsafe { enter_trampoline(ctx, rx_test_big_print, null_mut()) }
            })
            .unwrap();
        assert_eq!(big.len(), 1200 * 1024);
    });
}

#[repr(C)]
struct NestedReuse {
    cont: Sexp,
    depth: c_int,
    fail_inner: bool,
    cleanups: *mut c_int,
}

#[test]
fn unwind_cont_is_listsxp_callbacks_keep_protects_and_nested_reuse_works() {
    each_policy(|ctx| {
        // P0-1: R_MakeUnwindCont() returns a LISTSXP (TYPEOF == 2).
        let mut cont = Sexp::NULL;
        // SAFETY: Fixture writes a single Sexp.
        unsafe { enter_trampoline(ctx, rx_test_make_cont, from_mut(&mut cont).cast()) }.unwrap();
        assert_eq!(
            ctx.session().type_of(cont.to_id().unwrap()).unwrap(),
            SexpType::List
        );

        // P0-2: Callbacks keep protections on normal return with zero imbalance warnings.
        let stderr = capture(ctx, |ctx| {
            // SAFETY: Fixture ignores data.
            unsafe { enter_trampoline(ctx, rx_test_callback_keeps_protect, null_mut()) }.unwrap();
        });
        assert!(stderr.is_empty(), "{stderr}");

        // P1-2: Sequential and nested reuse of the exact same static token.
        let mut cleanups = 0 as c_int;
        let mut ok_state = NestedReuse {
            cont: Sexp::NULL,
            depth: 1,
            fail_inner: false,
            cleanups: from_mut(&mut cleanups),
        };
        // SAFETY: Fixture uses ok_state only during the call.
        unsafe {
            enter_trampoline(
                ctx,
                rx_test_nested_token_reuse,
                from_mut(&mut ok_state).cast(),
            )
        }
        .unwrap();
        assert_eq!(cleanups, 4); // 2 levels * 2 sequential calls

        let mut err_cleanups = 0 as c_int;
        let mut err_state = NestedReuse {
            cont: Sexp::NULL,
            depth: 1,
            fail_inner: true,
            cleanups: from_mut(&mut err_cleanups),
        };
        let msg = ctx
            .with_native_error_handler(
                |ctx| {
                    // SAFETY: Fixture uses err_state only during the call.
                    unsafe {
                        enter_trampoline(
                            ctx,
                            rx_test_nested_token_reuse,
                            from_mut(&mut err_state).cast(),
                        )?;
                    }
                    Ok(SEXP_NIL)
                },
                condition_message,
            )
            .unwrap();
        assert_eq!(message(ctx, msg), "nested reuse error");
        assert_eq!(err_cleanups, 20); // both inner (+10) and outer (+10) cleanups ran
    });
}

#[test]
fn warnings_errorcall_bad_continue_cleanup_error_and_labels() {
    each_policy(|ctx| {
        // Rf_warning normal return with warn = 1 (immediate).
        ctx.run_script("warn1", "options(warn = 1L)", GLOBAL_ENV, false)
            .unwrap();
        let stderr = capture(ctx, |ctx| {
            // SAFETY: Fixture ignores data.
            unsafe { enter_trampoline(ctx, rx_test_warning, null_mut()) }.unwrap();
        });
        assert!(stderr.contains("native warn 7"), "{stderr}");

        // Rf_warning upgrades to error under warn = 2.
        ctx.run_script("warn2", "options(warn = 2L)", GLOBAL_ENV, false)
            .unwrap();
        let promoted = ctx
            .with_native_error_handler(
                |ctx| {
                    // SAFETY: Fixture ignores data.
                    unsafe { enter_trampoline(ctx, rx_test_warning, null_mut())? };
                    Ok(SEXP_NIL)
                },
                condition_message,
            )
            .unwrap();
        assert!(message(ctx, promoted).contains("native warn 7"));
        ctx.run_script("warn0", "options(warn = 0L)", GLOBAL_ENV, false)
            .unwrap();

        // Rf_errorcall passes call object through.
        let sym = ctx.session_mut().intern("myfn").unwrap().as_sexp();
        let mut call_sexp = Sexp::from_id(ctx.session_mut().cons(sym, SEXP_NIL, SEXP_NIL).unwrap());
        ctx.session_mut().heap.protect(call_sexp.to_id().unwrap());
        let errcall_msg = ctx
            .with_native_error_handler(
                |ctx| {
                    // SAFETY: call_sexp stays live during the call.
                    unsafe {
                        enter_trampoline(ctx, rx_test_errorcall, from_mut(&mut call_sexp).cast())?;
                    }
                    Ok(SEXP_NIL)
                },
                condition_message,
            )
            .unwrap();
        assert_eq!(message(ctx, errcall_msg), "call err 9");
        ctx.session_mut().heap.unprotect(1).unwrap();

        // Invalid and double R_ContinueUnwind raise clean R errors.
        let inv_msg = ctx
            .with_native_error_handler(
                |ctx| {
                    // SAFETY: Fixture ignores data.
                    unsafe { enter_trampoline(ctx, rx_test_invalid_continue, null_mut())? };
                    Ok(SEXP_NIL)
                },
                condition_message,
            )
            .unwrap();
        assert!(
            message(ctx, inv_msg).contains("R_ContinueUnwind"),
            "{}",
            message(ctx, inv_msg)
        );
        #[repr(C)]
        struct DoubleContinue {
            cont: Sexp,
            phase: c_int,
        }
        let cont_id = ctx
            .session_mut()
            .cons(SEXP_NIL, SEXP_NIL, SEXP_NIL)
            .unwrap();
        ctx.session_mut().heap.protect(cont_id);
        let mut dc = DoubleContinue {
            cont: Sexp::from_id(cont_id),
            phase: 0,
        };
        let first_msg = ctx
            .with_native_error_handler(
                |ctx| {
                    // SAFETY: dc stays live for the duration of the call.
                    unsafe {
                        enter_trampoline(ctx, rx_test_double_continue, from_mut(&mut dc).cast())?;
                    }
                    Ok(SEXP_NIL)
                },
                condition_message,
            )
            .unwrap();
        assert_eq!(message(ctx, first_msg), "native error 42");
        dc.phase = 1;
        let second_msg = ctx
            .with_native_error_handler(
                |ctx| {
                    // SAFETY: dc.cont is protected and non-null.
                    unsafe {
                        enter_trampoline(ctx, rx_test_double_continue, from_mut(&mut dc).cast())?;
                    }
                    Ok(SEXP_NIL)
                },
                condition_message,
            )
            .unwrap();
        ctx.session_mut().heap.unprotect(1).unwrap();
        assert!(
            message(ctx, second_msg).contains("R_ContinueUnwind"),
            "{}",
            message(ctx, second_msg)
        );

        // Rf_error inside a jump=1 cleanup replaces the error cleanly.
        let clean_msg = ctx
            .with_native_error_handler(
                |ctx| {
                    // SAFETY: Fixture ignores data.
                    unsafe { enter_trampoline(ctx, rx_test_error_in_cleanup, null_mut())? };
                    Ok(SEXP_NIL)
                },
                condition_message,
            )
            .unwrap();
        assert_eq!(message(ctx, clean_msg), "cleanup error");

        // Custom entry label (e.g. .External) appears in stack-imbalance warning.
        let ext_stderr = capture(ctx, |ctx| {
            // SAFETY: Fixture ignores data.
            unsafe {
                enter_trampoline_with(
                    ctx,
                    FrameKind::Entry { label: ".External" },
                    rx_test_imbalance,
                    null_mut(),
                )
            }
            .unwrap();
        });
        assert_eq!(
            ext_stderr.trim(),
            "Warning: stack imbalance in '.External', 0 then 1"
        );
    });
}

#[test]
fn worker_thread_can_read_heap_while_main_thread_released() {
    each_policy(|ctx| {
        let val = ctx.session_mut().scalar_int(12345).unwrap();
        ctx.session_mut().heap.protect(val);
        with_ctx_entered(ctx, || {
            let handle = std::thread::spawn(move || {
                with_heap(|heap| heap.get(val).is_ok() && heap.protect_depth() == 1)
            });
            assert!(handle.join().unwrap());
        });
        ctx.session_mut().heap.unprotect(1).unwrap();
    });
}

unsafe extern "C" fn recurse_depth(data: *mut c_void) {
    // SAFETY: Invoked inside an active trampoline.
    unsafe {
        boundary((), |ctx| {
            let remaining = &mut *data.cast::<usize>();
            if *remaining == 0 {
                return Ok(());
            }
            *remaining -= 1;
            let mut next = *remaining;
            enter_trampoline(ctx, recurse_depth, from_mut(&mut next).cast())
        });
    }
}

#[test]
fn trampoline_depth_limit_raises_r_error() {
    let mut ctx = Ctx::new(Box::new(PureHost::new()), GcPolicy::default()).unwrap();
    let mut depth = 300usize;
    let msg = ctx
        .with_native_error_handler(
            |ctx| {
                // SAFETY: depth stays live during the call.
                unsafe { enter_trampoline(ctx, recurse_depth, from_mut(&mut depth).cast())? };
                Ok(SEXP_NIL)
            },
            condition_message,
        )
        .unwrap();
    assert_eq!(message(&ctx, msg), "native trampoline depth limit exceeded");
    assert_eq!(ctx.session().heap.protect_depth(), 0);
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
