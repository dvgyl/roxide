//! Exercise the Wave 1-C C ABI with both garbage collector policies.

use std::ffi::{c_int, c_void};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicUsize, Ordering};

use roxide_capi::abi::{EXTPTRSXP, INTSXP, Sexp};
use roxide_capi::api::{
    arith::*, extptr::*, memory::*, random::*, rmath::*, s4::*, utils::*, weakref::*,
};
use roxide_capi::api::{
    coerce::Rf_ScalarInteger, sym::Rf_install, unwind::R_ExecWithCleanup, vector::*,
};
use roxide_capi::globals::{R_GlobalEnv, R_NaReal, R_NilValue};
use roxide_capi::jump::{Rf_protect, Rf_unprotect, VoidFn, enter_trampoline};
use roxide_eval::{Ctx, EvalResult, Module, NativeHooks, Package};
use roxide_gc::{GcPolicy, SexpId};
use roxide_host::PureHost;
use roxide_math::rng::{Kinds, Rng};
use roxide_value::{GLOBAL_ENV, SEXP_NIL, ValueApi};

#[derive(Default)]
struct Check {
    line: u32,
    input: Option<SexpId>,
    draws: [f64; 6],
    finalizers: usize,
}

macro_rules! check {
    ($state:ident, $condition:expr) => {
        let passed = $condition;
        if !passed {
            $state.line = line!();
            return;
        }
    };
}

fn run(ctx: &mut Ctx, callback: VoidFn, state: &mut Check) {
    ctx.with_native_error_handler(
        |ctx| {
            // SAFETY: Each callback uses POD locals across calls that can jump.
            unsafe { enter_trampoline(ctx, callback, std::ptr::from_mut(state).cast()) }?;
            Ok(SEXP_NIL)
        },
        |ctx, condition| {
            let text = ctx.session().list_get(condition, 0)?;
            let ch = ctx.session_mut().str_get(text, 0)?;
            let bytes = ctx.session().char_bytes(ch)?;
            panic!("native call failed with {}", String::from_utf8_lossy(bytes))
        },
    )
    .unwrap();
    assert_eq!(state.line, 0, "C API check failed at line {}", state.line);
    assert_eq!(ctx.session().heap.protect_depth(), 0);
}

unsafe extern "C" fn memory(data: *mut c_void) {
    // SAFETY: run passes a live Check and enters a foreign trampoline.
    unsafe {
        let state = &mut *data.cast::<Check>();
        let x = Rf_ScalarInteger(93);
        R_PreserveObject(x);
        R_PreserveObject(x);
        R_gc();
        check!(state, INTEGER_ELT(x, 0) == 93);
        R_ReleaseObject(x);
        R_gc();
        check!(state, INTEGER_ELT(x, 0) == 93);

        let set = Rf_protect(R_NewPreciousMSet(0));
        R_PreserveInMSet(x, set);
        R_PreserveInMSet(x, set);
        R_ReleaseObject(x);
        R_ReleaseFromMSet(x, set);
        R_gc();
        check!(state, INTEGER_ELT(x, 0) == 93);
        R_ReleaseFromMSet(x, set);
        Rf_unprotect(1);
        state.input = x.to_id();

        let start = vmaxget();
        let bytes = S_alloc(2, 4).cast::<u32>();
        check!(state, bytes.read() == 0 && bytes.add(1).read() == 0);
        bytes.write(8);
        let keep = vmaxget();
        let copy = S_realloc(bytes.cast(), 4, 2, 4).cast::<u32>();
        check!(state, copy.read() == 8 && copy.add(3).read() == 0);
        let aligned = R_allocLD(2);
        check!(state, aligned.addr().is_multiple_of(align_of::<f64>()));
        let raw = R_alloc(7, 1);
        raw.write(15);
        R_gc();
        check!(state, raw.read() == 15 && bytes.read() == 8);
        vmaxset(keep);
        check!(state, vmaxget() == keep && bytes.read() == 8);
        vmaxset(start);
        check!(state, vmaxget() == start);
        R_alloc(32, 1);
        R_ExecWithCleanup(Some(transient_body), null_mut(), Some(restore_mark), start);
        check!(state, vmaxget() == start);
        R_CheckStack();
        R_CheckStack2(64);

        let heap = R_chk_calloc(2, size_of::<i32>()).cast::<i32>();
        check!(state, heap.read() == 0);
        heap.write(27);
        let heap = R_chk_realloc(heap.cast(), 5 * size_of::<i32>()).cast::<i32>();
        let first = heap.read();
        R_chk_free(heap.cast());
        check!(state, first == 27);
    }
}

unsafe extern "C" fn allocation_overflow(_: *mut c_void) {
    // SAFETY: The trampoline catches the error.
    unsafe { R_alloc(usize::MAX, 2) };
}

static FINALIZERS: AtomicUsize = AtomicUsize::new(0);

struct Hooks;

struct Finalize {
    fun: unsafe extern "C" fn(Sexp),
    key: Sexp,
}

unsafe extern "C" fn invoke_finalizer(data: *mut c_void) {
    // SAFETY: Hooks passes a live callback record and enters a trampoline.
    unsafe {
        let call = &*data.cast::<Finalize>();
        (call.fun)(call.key);
    }
}

impl NativeHooks for Hooks {
    fn run_c_finalizer(&self, ctx: &mut Ctx, fun: usize, target: SexpId) -> EvalResult<()> {
        // SAFETY: Only the test's C finalizer callbacks enter this hook.
        let fun = unsafe { std::mem::transmute::<usize, unsafe extern "C" fn(Sexp)>(fun) };
        let mut call = Finalize {
            fun,
            key: Sexp::from_id(target),
        };
        // SAFETY: call lives until the callback returns or jumps.
        unsafe { enter_trampoline(ctx, invoke_finalizer, std::ptr::from_mut(&mut call).cast()) }
    }
}

unsafe extern "C" fn finalize(x: Sexp) {
    // SAFETY: The trampoline keeps the finalizer target live.
    if unsafe { TYPEOF(x) } == EXTPTRSXP as c_int {
        FINALIZERS.fetch_add(1, Ordering::Relaxed);
    }
}

unsafe extern "C" fn function() -> *mut c_void {
    null_mut()
}

unsafe extern "C" fn pointers(data: *mut c_void) {
    // SAFETY: run passes a live Check and enters a foreign trampoline.
    unsafe {
        let state = &mut *data.cast::<Check>();
        FINALIZERS.store(0, Ordering::Relaxed);
        let tag = Rf_protect(Rf_ScalarInteger(3));
        let nil = R_NilValue;
        let key = Rf_protect(R_MakeExternalPtr(data, tag, nil));
        check!(
            state,
            R_ExternalPtrAddr(key) == data && R_ExternalPtrTag(key) == tag
        );
        R_SetExternalPtrProtected(key, tag);
        R_SetExternalPtrTag(key, nil);
        check!(
            state,
            R_ExternalPtrProtected(key) == tag && R_ExternalPtrTag(key) == nil
        );
        R_ClearExternalPtr(key);
        check!(state, R_ExternalPtrAddr(key).is_null());
        R_SetExternalPtrAddr(key, data);
        check!(state, R_ExternalPtrAddr(key) == data);

        let f = Rf_protect(R_MakeExternalPtrFn(Some(function), nil, nil));
        let address = R_ExternalPtrAddrFn(f);
        check!(
            state,
            address.map(|f| f as usize) == Some(function as *const () as usize)
        );

        let value = Rf_ScalarInteger(7);
        let weak = Rf_protect(R_MakeWeakRefC(key, value, Some(finalize), 0));
        check!(
            state,
            R_WeakRefKey(weak) == key && R_WeakRefValue(weak) == value
        );
        R_RunWeakRefFinalizer(weak);
        R_RunWeakRefFinalizer(weak);
        check!(
            state,
            R_WeakRefKey(weak) == nil && R_WeakRefValue(weak) == nil
        );
        check!(state, FINALIZERS.load(Ordering::Relaxed) == 1);

        let weak = Rf_protect(R_MakeWeakRef(f, tag, nil, 0));
        R_RegisterCFinalizer(f, Some(finalize));
        roxide_capi::jump::Rf_unprotect_ptr(f);
        R_gc();
        check!(
            state,
            R_WeakRefKey(weak) == nil && R_WeakRefValue(weak) == nil
        );
        R_RunPendingFinalizers();
        R_RunPendingFinalizers();
        check!(state, FINALIZERS.load(Ordering::Relaxed) == 2);
        R_RegisterCFinalizerEx(key, Some(finalize), 1);
        R_RunExitFinalizers();
        R_RunExitFinalizers();
        state.finalizers = FINALIZERS.load(Ordering::Relaxed);
        Rf_unprotect(4);
    }
}

unsafe extern "C" fn closure_finalizers(data: *mut c_void) {
    // SAFETY: run passes a live Check and enters a foreign trampoline.
    unsafe {
        let state = &mut *data.cast::<Check>();
        let Some(closure) = state.input else {
            state.line = line!();
            return;
        };
        let closure = Sexp::from_id(closure);
        let first = roxide_capi::api::env::R_NewEnv(R_GlobalEnv, 0, 0);
        R_RegisterFinalizer(first, closure);
        R_gc();
        R_RunPendingFinalizers();

        let second = Rf_protect(roxide_capi::api::env::R_NewEnv(R_GlobalEnv, 0, 0));
        R_RegisterFinalizerEx(second, closure, 1);
        R_RunExitFinalizers();
        Rf_unprotect(1);
    }
}

unsafe extern "C" fn random_math(data: *mut c_void) {
    // SAFETY: run passes a live Check and enters a foreign trampoline.
    unsafe {
        let state = &mut *data.cast::<Check>();
        GetRNGstate();
        state.draws[0] = unif_rand();
        state.draws[1] = norm_rand();
        state.draws[2] = exp_rand();
        state.draws[3] = R_unif_index(17.0);
        state.draws[4] = Rf_rnorm(4.0, 3.0);
        PutRNGstate();
        GetRNGstate();
        state.draws[5] = Rf_rexp(2.0);
        PutRNGstate();

        let na = R_NaReal;
        check!(
            state,
            R_IsNA(na) == 1 && R_IsNaN(na) == 0 && R_isnancpp(na) == 1
        );
        check!(
            state,
            R_IsNaN(f64::NAN) == 1 && R_finite(1.0) == 1 && R_finite(f64::INFINITY) == 0
        );
        check!(state, R_pow(na, 0.0) == 1.0 && R_pow_di(-2.0, -3) == -0.125);
        check!(state, Rf_qnorm5(0.5, 0.0, 1.0, 1, 0) == 0.0);
        check!(state, Rf_pnorm5(0.0, 0.0, 1.0, 0, 1) == (0.5_f64).ln());
        check!(
            state,
            (Rf_dexp(4.0, 2.0, 1) + 2.0 + 2.0_f64.ln()).abs() < 1e-13
        );
        check!(state, (Rf_gammafn(5.0) - 24.0).abs() < 1e-13);
        check!(state, (Rf_bessel_j(0.0, 0.0) - 1.0).abs() < 1e-13);
        let mut sign = 0;
        Rf_lgammafn_sign(-0.5, &mut sign);
        check!(state, sign == -1);
        let logs = [-f64::INFINITY, -2.0, -2.0];
        check!(
            state,
            Rf_logspace_sum(logs.as_ptr(), 3) == -2.0 + 2.0_f64.ln()
        );
    }
}

const BASE_FIXTURE: Module = Module {
    prims: &[],
    prelude: r#"
indices <- function(x) 1L:length(x)
duplicated.default <- function(x, incomparables, fromLast) {
    out <- x != x
    for (i in indices(x)) {
        if (length(incomparables) > 0L) {
            if (x[i] == incomparables[1L]) next
        }
        for (j in indices(x))
            if (if (fromLast) i < j else i > j)
                if (x[i] == x[j]) out[i] <- TRUE
    }
    out
}
anyDuplicated.default <- function(x, incomparables, fromLast) {
    d <- duplicated.default(x, incomparables, fromLast)
    n <- length(d)
    if (fromLast) {
        while (n > 0L) {
            if (d[n]) return(n)
            n <- n - 1L
        }
        return(0L)
    }
    for (i in indices(d)) if (d[i]) return(i)
    0L
}
match <- function(x, table, nomatch) {
    out <- x
    for (i in indices(x)) {
        out[i] <- nomatch
        for (j in indices(table)) if (x[i] == table[j]) {
            out[i] <- j
            break
        }
    }
    out
}
"#,
};

const METHODS_FIXTURE: Module = Module {
    prims: &[],
    prelude: r#"
getClass <- function(Class, where) Class
getClassDef <- function(Class, where) Class
.s4_new <- function(Class, where) Class
"#,
};

unsafe extern "C" fn objects(data: *mut c_void) {
    // SAFETY: run passes a live Check and enters a foreign trampoline.
    unsafe {
        let state = &mut *data.cast::<Check>();
        let x = Rf_protect(Rf_allocVector(INTSXP, 3));
        SET_INTEGER_ELT(x, 0, 3);
        SET_INTEGER_ELT(x, 1, 1);
        SET_INTEGER_ELT(x, 2, 3);
        check!(state, Rf_isUnsorted(x, 0) == 1);
        check!(state, R_compute_identical(x, x, 0) == 1);
        check!(
            state,
            Rf_any_duplicated(x, 0) == 3 && Rf_any_duplicated(x, 1) == 1
        );
        let incomparable = Rf_protect(Rf_ScalarInteger(3));
        check!(state, Rf_any_duplicated3(x, incomparable, 0) == 0);
        let duplicates = Rf_protect(Rf_duplicated(x, 0));
        check!(
            state,
            LOGICAL_ELT(duplicates, 0) == 0 && LOGICAL_ELT(duplicates, 2) == 1
        );
        let matched = Rf_protect(Rf_match(incomparable, x, 0));
        check!(
            state,
            INTEGER_ELT(matched, 0) == 1
                && INTEGER_ELT(matched, 1) == 0
                && INTEGER_ELT(matched, 2) == 1
        );

        let object = Rf_protect(Rf_allocS4Object());
        check!(state, Rf_isS4(object) == 1);
        let slot = Rf_install(c"payload".as_ptr());
        check!(state, R_has_slot(object, slot) == 0);
        check!(state, R_do_slot_assign(object, slot, x) == object);
        check!(
            state,
            R_has_slot(object, slot) == 1 && R_do_slot(object, slot) == x
        );
        R_do_slot_assign(object, slot, R_NilValue);
        check!(
            state,
            R_has_slot(object, slot) == 1 && R_do_slot(object, slot) == R_NilValue
        );

        let definition = Rf_protect(R_do_MAKE_CLASS(c"Widget".as_ptr()));
        let optional = Rf_protect(R_getClassDef(c"Widget".as_ptr()));
        check!(state, R_compute_identical(definition, optional, 0) == 1);
        check!(state, R_do_new_object(definition) == definition);
        let class = Rf_install(c"class".as_ptr());
        R_do_slot_assign(object, class, definition);
        let mut classes = [c"Other".as_ptr(), c"Widget".as_ptr(), null()];
        check!(state, R_check_class_etc(object, classes.as_mut_ptr()) == 1);
        Rf_unprotect(7);
    }
}

#[test]
fn wave_one_c_with_both_gc_policies() {
    for policy in [GcPolicy::default(), GcPolicy::Torture] {
        let mut ctx = Ctx::new(Box::new(PureHost::default()), policy).unwrap();
        ctx.set_native_hooks(&Hooks);

        let mut state = Check::default();
        run(&mut ctx, memory, &mut state);
        ctx.collect_gc();
        assert!(ctx.session().heap.get(state.input.unwrap()).is_err());
        // SAFETY: The callback makes one failing allocation inside the trampoline.
        let error = unsafe { enter_trampoline(&mut ctx, allocation_overflow, null_mut()) };
        assert!(error.is_err());
        assert_eq!(ctx.session().heap.protect_depth(), 0);

        run(&mut ctx, pointers, &mut state);
        assert_eq!(state.finalizers, 3);

        ctx.run_script(
            "<finalizer fixture>",
            "finalized <- 0L; finalizer <- function(target) finalized <<- finalized + 1L",
            GLOBAL_ENV,
            false,
        )
        .unwrap();
        let finalizer = ctx.session_mut().intern("finalizer").unwrap();
        state.input = ctx.get_local(finalizer, GLOBAL_ENV).unwrap();
        run(&mut ctx, closure_finalizers, &mut state);
        let finalized = ctx.session_mut().intern("finalized").unwrap();
        let count = ctx.get_local(finalized, GLOBAL_ENV).unwrap().unwrap();
        assert_eq!(ctx.session().int(count).unwrap().get(0), 2);

        let mut rng = Rng::seeded(2026, Kinds::default());
        let mut words = Vec::new();
        rng.encode(&mut words);
        let symbol = ctx.session_mut().intern(".Random.seed").unwrap();
        let seed = ctx.session_mut().alloc_int_from(&words).unwrap();
        ctx.assign(symbol, seed, GLOBAL_ENV).unwrap();
        run(&mut ctx, random_math, &mut state);
        assert_eq!(
            state.draws,
            [
                rng.unif_rand(),
                rng.norm_rand(),
                rng.exp_rand(),
                rng.unif_index(17.0),
                roxide_math::dist::normal::rnorm(&mut rng, 4.0, 3.0, &mut Default::default()),
                2.0 * rng.exp_rand(),
            ]
        );
        rng.encode(&mut words);
        let seed = ctx.get_local(symbol, GLOBAL_ENV).unwrap().unwrap();
        assert_eq!(
            ctx.session()
                .int(seed)
                .unwrap()
                .iter()
                .collect::<Vec<c_int>>(),
            words
        );

        ctx.register_module(&BASE_FIXTURE).unwrap();
        ctx.register_package(&Package {
            name: "methods",
            modules: &[&METHODS_FIXTURE],
            attach: false,
            exports: None,
        })
        .unwrap();
        run(&mut ctx, objects, &mut state);
    }
}

unsafe extern "C" fn transient_exit(data: *mut c_void) {
    // SAFETY: The caller supplies a live bool and enters a foreign trampoline.
    unsafe {
        R_alloc(32, 1).write(17);
        if *data.cast::<bool>() {
            R_alloc(1, -1);
        }
    }
}

unsafe extern "C" fn transient_body(_: *mut c_void) -> Sexp {
    // SAFETY: R_ExecWithCleanup enters a trampoline before it calls this body.
    unsafe { R_alloc(32, 1) };
    R_NilValue
}

unsafe extern "C" fn restore_mark(mark: *mut c_void) {
    // SAFETY: The caller supplies an arena mark. R_ExecWithCleanup enters a trampoline.
    unsafe { vmaxset(mark) };
}

unsafe extern "C" fn read_transient_mark(data: *mut c_void) {
    // SAFETY: The caller supplies writable pointer storage and a trampoline.
    unsafe { data.cast::<*mut c_void>().write(vmaxget()) };
}

#[test]
fn entry_releases_transient_allocations_on_return_and_unwind() {
    for policy in [GcPolicy::default(), GcPolicy::Torture] {
        let mut ctx = Ctx::new(Box::new(PureHost::default()), policy).unwrap();
        for mut fail in [false, true] {
            // SAFETY: Each callback receives a live value of its expected type.
            unsafe {
                let result = enter_trampoline(
                    &mut ctx,
                    transient_exit,
                    std::ptr::from_mut(&mut fail).cast(),
                );
                assert_eq!(result.is_err(), fail);
                let mut mark: *mut c_void = null_mut();
                enter_trampoline(
                    &mut ctx,
                    read_transient_mark,
                    std::ptr::from_mut(&mut mark).cast(),
                )
                .unwrap();
                assert!(mark.is_null());
            }
        }
    }
}
