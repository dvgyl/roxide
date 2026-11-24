//! Exercise the Wave 1-C C ABI with both garbage collector policies.

use std::ffi::{c_int, c_void};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicUsize, Ordering};

use roxide_capi::abi::{EXTPTRSXP, INTSXP, Sexp};
use roxide_capi::api::{
    arith::*,
    env::*,
    extptr::*,
    memory::*,
    parse::R_ParseEvalString,
    random::*,
    rmath::*,
    s4::*,
    string::{R_CHAR, STRING_ELT},
    utils::*,
    weakref::*,
};
use roxide_capi::api::{
    attrib::*,
    coerce::{R_isTRUE, Rf_ScalarInteger, Rf_ScalarLogical, Rf_asBool, Rf_asRboolean},
    pairlist::{CAR, CDR, Rf_allocList, SETCAR},
    sym::Rf_install,
    unwind::R_ExecWithCleanup,
    vector::*,
};
use roxide_capi::globals::{R_EmptyEnv, R_GlobalEnv, R_NaReal, R_NilValue};
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
        SETLEVELS(weak, 1 << 15);
        check!(state, R_WeakRefKey(weak) == key);
        R_RunWeakRefFinalizer(weak);
        SETLEVELS(weak, 0);
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

        let data_slot = Rf_install(c".Data".as_ptr());
        let xdata_slot = Rf_install(c".xData".as_ptr());
        check!(state, R_has_slot(object, data_slot) == 0);
        check!(state, R_do_slot(object, data_slot) == R_NilValue);
        let storage = Rf_protect(R_NewEnv(R_EmptyEnv, 0, 0));
        R_do_slot_assign(object, xdata_slot, storage);
        check!(state, R_has_slot(object, data_slot) == 1);
        check!(state, R_has_slot(object, xdata_slot) == 1);
        check!(state, R_do_slot(object, data_slot) == storage);
        check!(state, R_do_slot(object, xdata_slot) == storage);
        let typed = Rf_protect(Rf_ScalarInteger(19));
        SET_S4_OBJECT(typed);
        check!(state, R_has_slot(typed, data_slot) == 1);
        let payload = Rf_protect(R_do_slot(typed, data_slot));
        check!(
            state,
            INTEGER_ELT(payload, 0) == 19 && Rf_isS4(payload) == 0
        );
        check!(state, Rf_isS4(typed) != 0);
        SET_S4_OBJECT(storage);
        check!(state, R_has_slot(storage, data_slot) == 1);
        check!(
            state,
            R_do_slot(storage, data_slot) == storage && Rf_isS4(storage) == 0
        );
        Rf_unprotect(3);

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
        run(&mut ctx, bindings_and_dots, &mut state);
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

        run(&mut ctx, order_and_sort, &mut state);
        run(&mut ctx, vector_and_attrs, &mut state);

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

unsafe extern "C" fn bindings_and_dots(data: *mut c_void) {
    // SAFETY: run passes a live Check and enters a foreign trampoline.
    unsafe {
        let state = &mut *data.cast::<Check>();
        let parent = Rf_protect(R_NewEnv(R_EmptyEnv, 0, 0));
        let child = Rf_protect(R_NewEnv(parent, 0, 0));
        let name = Rf_install(c"x".as_ptr());
        let hidden = Rf_install(c".missing".as_ptr());
        let expr = Rf_protect(Rf_ScalarInteger(11));
        let cached = Rf_protect(Rf_ScalarInteger(27));
        check!(state, Rf_topenv(parent, child) == parent);
        check!(state, Rf_topenv(R_NilValue, child) == R_GlobalEnv);
        check!(state, R_GetBindingType(name, parent) as u32 == 0);
        Rf_defineVar(name, cached, parent);
        check!(state, R_GetBindingType(name, parent) as u32 == 1);
        R_MakeMissingBinding(hidden, parent);
        check!(state, R_GetBindingType(hidden, parent) as u32 == 2);
        R_MakeDelayedBinding(name, expr, child, parent);
        check!(state, R_GetBindingType(name, parent) as u32 == 3);
        check!(state, R_GetBindingType(name, child) as u32 == 0);
        check!(state, R_DelayedBindingExpression(name, parent) == expr);
        check!(state, R_DelayedBindingEnvironment(name, parent) == child);
        check!(state, R_getVar(name, parent, 0) == expr);
        check!(state, R_GetBindingType(name, parent) as u32 == 4);
        check!(state, R_ForcedBindingExpression(name, parent) == expr);
        R_MakeForcedBinding(name, expr, cached, parent);
        check!(state, R_getVar(name, parent, 0) == cached);
        let symbols = Rf_protect(R_envSymbols(parent));
        check!(state, Rf_length(symbols) == 2);
        check!(state, VECTOR_ELT(symbols, 0) == hidden);
        check!(state, VECTOR_ELT(symbols, 1) == name);

        let dots = Rf_protect(R_ParseEvalString(
            c"(function(...) environment())(named = 12L, , 14L)".as_ptr(),
            R_GlobalEnv,
        ));
        let inner = Rf_protect(R_NewEnv(dots, 0, 0));
        check!(state, R_DotsExist(inner) == 0);
        check!(state, R_findDotsEnv(inner) == dots);
        Rf_defineVar(Rf_install(c"...".as_ptr()), cached, inner);
        check!(state, R_DotsExist(inner) == 0);
        check!(state, R_findDotsEnv(inner) == dots);
        check!(state, R_DotsExist(dots) == 1 && R_DotsLength(dots) == 3);
        let names = Rf_protect(R_DotsNames(dots));
        check!(state, Rf_length(names) == 3);
        check!(
            state,
            std::ffi::CStr::from_ptr(R_CHAR(STRING_ELT(names, 0))) == c"named"
        );
        check!(
            state,
            std::ffi::CStr::from_ptr(R_CHAR(STRING_ELT(names, 1))) == c""
        );
        check!(state, R_GetDotType(1, dots) as u32 == 2);
        check!(state, R_GetDotType(2, dots) as u32 == 1);
        check!(state, R_DotDelayedEnvironment(1, dots) == R_GlobalEnv);
        let dot_expr = R_DotDelayedExpression(1, dots);
        check!(state, INTEGER_ELT(dot_expr, 0) == 12);
        let dot = R_DotsElt(1, dots);
        check!(state, INTEGER_ELT(dot, 0) == 12);
        check!(state, R_GetDotType(1, dots) as u32 == 3);
        check!(state, R_DotForcedExpression(1, dots) == dot_expr);
        let empty = Rf_protect(R_ParseEvalString(
            c"(function(...) environment())()".as_ptr(),
            R_GlobalEnv,
        ));
        check!(state, R_DotsExist(empty) == 1 && R_DotsLength(empty) == 0);
        check!(state, R_DotsNames(empty) == R_NilValue);
        Rf_unprotect(9);
    }
}

unsafe extern "C" fn find_label(name: Sexp, value: Sexp, wanted: *mut c_void) -> Sexp {
    // SAFETY: vector_and_attrs passes a pointer to its label handle.
    if name == unsafe { wanted.cast::<Sexp>().read() } {
        value
    } else {
        Sexp::NULL
    }
}

unsafe extern "C" fn vector_and_attrs(data: *mut c_void) {
    // SAFETY: run passes a live Check and enters a foreign trampoline.
    unsafe {
        let state = &mut *data.cast::<Check>();
        let mut names = [c"a".as_ptr(), c"b".as_ptr(), c"".as_ptr()];
        let source = Rf_protect(Rf_mkNamed(INTSXP, names.as_mut_ptr()));
        SET_INTEGER_ELT(source, 0, 10);
        SET_INTEGER_ELT(source, 1, 20);
        let matrix = Rf_protect(Rf_allocMatrix(INTSXP, 2, 3));
        Rf_copyMatrix(matrix, source, 1);
        check!(state, R_nrow(matrix) == 2 && R_ncol(matrix) == 3);
        check!(
            state,
            (0..6)
                .map(|i| INTEGER_ELT(matrix, i))
                .eq([10, 20, 20, 10, 10, 20])
        );
        Rf_copyVector(matrix, source);
        check!(
            state,
            (0..6)
                .map(|i| INTEGER_ELT(matrix, i))
                .eq([10, 20, 10, 20, 10, 20])
        );

        let pairs = Rf_protect(Rf_allocList(4));
        let source_pairs = Rf_protect(Rf_allocList(2));
        SETCAR(source_pairs, Rf_ScalarInteger(10));
        SETCAR(CDR(source_pairs), Rf_ScalarInteger(20));
        let dim = Rf_install(c"dim".as_ptr());
        let square = Rf_protect(Rf_allocVector(INTSXP, 2));
        SET_INTEGER_ELT(square, 0, 2);
        SET_INTEGER_ELT(square, 1, 2);
        Rf_setAttrib(pairs, dim, square);
        Rf_copyListMatrix(pairs, source_pairs, 1);
        check!(state, INTEGER_ELT(CAR(pairs), 0) == 10);
        check!(state, INTEGER_ELT(CAR(CDR(pairs)), 0) == 20);
        check!(state, CAR(CDR(CDR(pairs))) == R_NilValue);
        Rf_copyListMatrix(pairs, source_pairs, 0);
        check!(state, INTEGER_ELT(CAR(CDR(CDR(pairs))), 0) == 10);
        Rf_unprotect(3);

        let name = Rf_install(c"names".as_ptr());
        let labels = Rf_getAttrib(source, name);
        check!(
            state,
            std::ffi::CStr::from_ptr(R_CHAR(STRING_ELT(labels, 1))) == c"b"
        );
        let with_attrs = Rf_protect(Rf_allocVector(INTSXP, 2));
        SHALLOW_DUPLICATE_ATTRIB(with_attrs, source);
        check!(state, Rf_getAttrib(with_attrs, name) == labels);
        DUPLICATE_ATTRIB(with_attrs, source);
        let duplicate = Rf_getAttrib(with_attrs, name);
        check!(
            state,
            duplicate != labels && R_compute_identical(duplicate, labels, 0) == 1
        );

        let class = Rf_protect(R_class(matrix));
        check!(
            state,
            std::ffi::CStr::from_ptr(R_CHAR(STRING_ELT(class, 0))) == c"matrix"
        );
        check!(
            state,
            std::ffi::CStr::from_ptr(R_CHAR(STRING_ELT(class, 1))) == c"array"
        );
        let mut wanted = name;
        check!(
            state,
            R_mapAttrib(
                with_attrs,
                Some(find_label),
                std::ptr::from_mut(&mut wanted).cast()
            ) == duplicate
        );
        let wrapped = Rf_protect(R_tryWrap(source));
        check!(state, wrapped != source && INTEGER_ELT(wrapped, 1) == 20);
        check!(state, Rf_getAttrib(wrapped, name) == labels);

        let yes = Rf_protect(Rf_ScalarLogical(1));
        check!(
            state,
            R_isTRUE(yes) && Rf_asBool(yes) && Rf_asRboolean(yes) == 1
        );
        check!(state, !R_isTRUE(source));
        Rf_unprotect(6);
    }
}

unsafe extern "C" fn c_normal(data: *mut c_void) {
    // SAFETY: The pointer comes from run and points to a live Check.
    unsafe {
        GetRNGstate();
        let value = norm_rand();
        PutRNGstate();
        (*data.cast::<Check>()).draws[0] = value;
    }
}

#[test]
fn box_muller_spare_survives_native_seed_sync() {
    let mut ctx = Ctx::new(Box::new(PureHost::default()), GcPolicy::Torture).unwrap();
    let kinds = Kinds {
        normal: roxide_math::rng::NormalKind::BoxMuller,
        ..Kinds::default()
    };
    let mut rng = Rng::seeded(123, kinds);
    assert!((rng.norm_rand() - (-0.161_343_055_045_079_2)).abs() < 1e-15);
    let mut words = Vec::new();
    rng.encode(&mut words);
    let symbol = ctx.session_mut().intern(".Random.seed").unwrap();
    let seed = ctx.session_mut().alloc_int_from(&words).unwrap();
    ctx.assign(symbol, seed, GLOBAL_ENV).unwrap();
    ctx.save_box_muller_spare(seed, words, rng.bm_saved);

    let mut state = Check::default();
    run(&mut ctx, c_normal, &mut state);
    assert!((state.draws[0] - 0.670_603_080_296_171_2).abs() < 1e-15);
    run(&mut ctx, c_normal, &mut state);
    assert!((state.draws[0] - (-0.419_440_279_774_721_2)).abs() < 1e-15);

    let seed = ctx.get_local(symbol, GLOBAL_ENV).unwrap().unwrap();
    let words = ctx.session().int(seed).unwrap().to_vec();
    assert!((ctx.box_muller_spare(seed, &words).unwrap() - 0.269_982_571_062_688_86).abs() < 1e-15);
    let replacement = ctx.session_mut().alloc_int_from(&words).unwrap();
    ctx.assign(symbol, replacement, GLOBAL_ENV).unwrap();
    run(&mut ctx, c_normal, &mut state);
    assert!((state.draws[0] - 2.313_615_980_439_986).abs() < 1e-15);
}

unsafe extern "C" fn order_and_sort(data: *mut c_void) {
    // SAFETY: run passes a live Check and enters a foreign trampoline.
    unsafe {
        let state = &mut *data.cast::<Check>();
        let x = Rf_protect(Rf_allocVector(INTSXP, 5));
        let y = Rf_protect(Rf_allocVector(INTSXP, 5));
        for (i, (a, b)) in [(1, 3), (1, 4), (c_int::MIN, 1), (c_int::MIN, 2), (2, 2)]
            .into_iter()
            .enumerate()
        {
            SET_INTEGER_ELT(x, i as isize, a);
            SET_INTEGER_ELT(y, i as isize, b);
        }
        let args = Rf_protect(roxide_capi::api::pairlist::Rf_lang2(x, y));
        let mut indexes = [std::mem::MaybeUninit::<c_int>::uninit(); 5];
        R_orderVector(indexes.as_mut_ptr().cast(), 5, args, 1, 1);
        check!(state, indexes.map(|i| i.assume_init()) == [4, 1, 0, 3, 2]);
        R_orderVector1(indexes.as_mut_ptr().cast(), 5, x, 0, 1);
        check!(state, indexes.map(|i| i.assume_init()) == [2, 3, 4, 0, 1]);
        Rf_unprotect(3);

        let mut integers = [c_int::MIN, 4, 2, c_int::MIN];
        R_isort(integers.as_mut_ptr(), 4);
        check!(state, integers == [2, 4, c_int::MIN, c_int::MIN]);

        let mut real = [9.0, 4.0, 1.0, 3.0, 2.0, 0.0];
        let mut index = [101, 102, 103, 104, 105, 106];
        qsort4_(real.as_mut_ptr(), index.as_mut_ptr(), &mut 2, &mut 5);
        check!(state, real == [9.0, 1.0, 2.0, 3.0, 4.0, 0.0]);
        check!(state, index == [101, 103, 105, 104, 102, 106]);
        Rf_revsort(real.as_mut_ptr(), index.as_mut_ptr(), 6);
        check!(state, real == [9.0, 4.0, 3.0, 2.0, 1.0, 0.0]);
        check!(state, index == [101, 102, 104, 105, 103, 106]);
        Rf_rPsort(real.as_mut_ptr(), 6, 2);
        check!(state, real[2] == 2.0);
        check!(state, real[..2].iter().all(|&value| value <= 2.0));
        check!(state, real[3..].iter().all(|&value| value >= 2.0));

        let mut cuts = [1.0, 1.0, 2.0, 3.0, 3.0];
        let mut flag = 90;
        check!(
            state,
            findInterval(cuts.as_mut_ptr(), 5, 3.0, 1, 0, 2, &mut flag) == 4
        );
        check!(state, flag == 1);
        check!(
            state,
            findInterval2(cuts.as_mut_ptr(), 5, 1.0, 1, 0, 1, 4, &mut flag) == 1
        );
        check!(state, flag == -1);
        let mut guess = 2;
        let index = interv_(
            cuts.as_mut_ptr(),
            &mut 5,
            &mut 0.0,
            &mut 0,
            &mut 1,
            &mut guess,
            &mut flag,
        );
        check!(state, index == 1 && flag == -1 && guess == 2);
        check!(
            state,
            d1mach_(&3) == f64::EPSILON / 2.0 && d1mach_(&6) == 0.0
        );
        check!(
            state,
            i1mach_(&14) == f64::MANTISSA_DIGITS as c_int && i1mach_(&17) == 0
        );
        rchkusr_();

        let mut matrix = [f64::NAN, 4.0, 2.0, 3.0, 4.0, 2.0];
        let mut maxima = [std::mem::MaybeUninit::<c_int>::uninit(); 3];
        R_max_col(
            matrix.as_mut_ptr(),
            &mut 3,
            &mut 2,
            maxima.as_mut_ptr().cast(),
            &mut 2,
        );
        check!(state, maxima.map(|x| x.assume_init()) == [c_int::MIN, 1, 1]);
        R_max_col(
            matrix.as_mut_ptr(),
            &mut 3,
            &mut 2,
            maxima.as_mut_ptr().cast(),
            &mut 3,
        );
        check!(state, maxima.map(|x| x.assume_init()) == [c_int::MIN, 2, 2]);

        let mut probabilities = [0.0, 1.0, 0.0];
        let mut counts = [std::mem::MaybeUninit::<c_int>::uninit(); 3];
        GetRNGstate();
        Rf_rmultinom(6, probabilities.as_mut_ptr(), 3, counts.as_mut_ptr().cast());
        PutRNGstate();
        check!(state, counts.map(|x| x.assume_init()) == [0, 6, 0]);

        let x = 2.0_f64.powi(-54);
        let expected = 1.742_135_243_472_579_4;
        check!(state, (pow1p(x, 1e16) - expected).abs() < 1e-14);
        check!(state, (Rf_pow1p(x, 1e16) - expected).abs() < 1e-14);
        check!(state, Rf_pow1p(-4.0, 3.0) == -27.0);
    }
}

unsafe extern "C" fn max_col_random(data: *mut c_void) {
    // SAFETY: run passes a live Check and enters a foreign trampoline.
    unsafe {
        let state = &mut *data.cast::<Check>();
        let mut matrix = [
            f64::NAN,
            3.0,
            3.0,
            f64::INFINITY,
            f64::NEG_INFINITY,
            0.0,
            1.0,
            5.0,
            3.0 + 1e-7,
            3.0 + 1e-4,
            f64::INFINITY,
            f64::NEG_INFINITY,
            0.0,
            5.0,
            8.0,
            1.0,
            1.0,
            0.0,
            f64::NEG_INFINITY,
            0.0,
            1.0,
        ];
        let mut maxima = [std::mem::MaybeUninit::<c_int>::uninit(); 7];
        R_max_col(
            matrix.as_mut_ptr(),
            &mut 7,
            &mut 3,
            maxima.as_mut_ptr().cast(),
            &mut 1,
        );
        check!(
            state,
            maxima.map(|x| x.assume_init()) == [c_int::MIN, 2, 2, 1, 2, 2, 2]
        );
        GetRNGstate();
        let next = unif_rand();
        PutRNGstate();
        check!(state, next == 0.206_887_663_574_889_3);
    }
}

#[test]
fn max_col_random_updates_the_global_seed() {
    for policy in [GcPolicy::default(), GcPolicy::Torture] {
        let mut ctx = Ctx::new(Box::new(PureHost::default()), policy).unwrap();
        ctx.set_native_hooks(&Hooks);
        let mut words = Vec::new();
        Rng::seeded(17, Kinds::default()).encode(&mut words);
        let symbol = ctx.session_mut().intern(".Random.seed").unwrap();
        let seed = ctx.session_mut().alloc_int_from(&words).unwrap();
        ctx.assign(symbol, seed, GLOBAL_ENV).unwrap();
        run(&mut ctx, max_col_random, &mut Check::default());
    }
}
