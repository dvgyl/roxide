//! Exercise foreign ALTREP methods from C and from the evaluator hooks.

use std::ffi::{c_int, c_void};
use std::ptr::{from_mut, null_mut};
use std::sync::Mutex;

use roxide_capi::abi::*;
use roxide_capi::api::{altrep::*, attrib::*, coerce::*, string::*, sym::Rf_install, vector::*};
use roxide_capi::{hooks, jump};
use roxide_eval::{AltOp, AltReply, Ctx, EvalResult};
use roxide_gc::{GcPolicy, SexpId};
use roxide_host::PureHost;
use roxide_value::{RComplex, RLogical, SEXP_NIL, SexpType, ValueApi};

#[link(name = "roxide_capi_altrep_fixture", kind = "static")]
unsafe extern "C" {
    fn R_init_f07_altrep(dll: *mut c_void);
    fn fixture_alt_new(data: Sexp, mode: Sexp) -> Sexp;
    fn fixture_alt_atomic(data: Sexp) -> Sexp;
    fn fixture_alt_restore(data: Sexp, extended: c_int) -> Sexp;
    fn fixture_alt_fail(data: Sexp) -> Sexp;
    fn fixture_alt_mutate(string: Sexp, list: Sexp) -> Sexp;
    fn fixture_alt_no_pointer(x: Sexp) -> Sexp;
}

static FIXTURE: Mutex<()> = Mutex::new(());

fn context() -> Ctx {
    let mut ctx = Ctx::new(Box::new(PureHost::new()), GcPolicy::Torture).unwrap();
    hooks::install(&mut ctx);
    // SAFETY: The linked C fixture accepts a null DLL handle for the embedding context.
    unsafe { jump::enter_trampoline(&mut ctx, R_init_f07_altrep, null_mut()) }.unwrap();
    ctx
}

fn entry<T>(ctx: &mut Ctx, fun: jump::VoidFn, input: &mut T) -> EvalResult<()> {
    // SAFETY: Each caller supplies the record type expected by the linked probe.
    unsafe { jump::enter_trampoline(ctx, fun, from_mut(input).cast()) }
}

fn int(ctx: &mut Ctx, vals: &[i32]) -> SexpId {
    let x = ctx.session_mut().alloc_int_from(vals).unwrap();
    ctx.protect(x);
    x
}

fn outcome(ctx: &mut Ctx, op: AltOp) -> AltReply {
    ctx.native_hooks().unwrap().altrep(ctx, op).unwrap()
}

#[repr(C)]
struct Probe {
    backing: Sexp,
    mode: Sexp,
    output: Sexp,
}

impl Probe {
    fn new(backing: SexpId, mode: SexpId) -> Self {
        Self {
            backing: Sexp::from_id(backing),
            mode: Sexp::from_id(mode),
            output: Sexp::NULL,
        }
    }

    fn id(&self) -> SexpId {
        self.output.to_id().unwrap()
    }
}

unsafe extern "C" fn make(data: *mut c_void) {
    // SAFETY: The record is a live Probe with protected input handles.
    unsafe {
        let data = &mut *data.cast::<Probe>();
        data.output = fixture_alt_new(data.backing, data.mode);
    }
}

unsafe extern "C" fn make_atomic(data: *mut c_void) {
    // SAFETY: The record is a live Probe with a protected atomic vector.
    unsafe {
        let data = &mut *data.cast::<Probe>();
        data.output = fixture_alt_atomic(data.backing);
    }
}

#[test]
fn lazy_length_metadata_and_evaluator_operations() {
    let _fixture = FIXTURE.lock().unwrap();
    let ctx = &mut context();
    let backing = int(ctx, &[2, 3, 5]);
    let fallback = int(ctx, &[0]);
    let returns_nil = int(ctx, &[1]);
    let mut a = Probe::new(backing, fallback);
    let mut b = Probe::new(backing, returns_nil);
    entry(ctx, make, &mut a).unwrap();
    ctx.protect(a.id());
    entry(ctx, make, &mut b).unwrap();
    ctx.protect(b.id());
    assert_eq!(
        ctx.session().foreign_alt(a.id()).unwrap().unwrap().len,
        None
    );

    entry(ctx, integer_access, &mut a).unwrap();
    let storage = ctx.session().foreign_alt(a.id()).unwrap().unwrap();
    assert_eq!(storage.len, Some(3));
    assert!(!storage.owned);
    assert_eq!(
        ctx.altrep_sorted(a.id()).unwrap(),
        roxide_value::Sortedness::Increasing
    );
    assert!(ctx.altrep_no_na(a.id()).unwrap());
    assert_eq!(
        outcome(
            ctx,
            AltOp::Sum {
                x: a.id(),
                na_rm: false
            }
        ),
        AltReply::Value(None)
    );
    assert_eq!(
        outcome(
            ctx,
            AltOp::Sum {
                x: b.id(),
                na_rm: false
            }
        ),
        AltReply::Value(Some(SEXP_NIL))
    );

    let index = int(ctx, &[1, 3]);
    let result = ctx
        .altrep_method(AltOp::ExtractSubset {
            x: a.id(),
            indx: index,
            call: SEXP_NIL,
        })
        .unwrap()
        .unwrap();
    assert_eq!(
        ctx.session().int(result).unwrap().as_slice().unwrap(),
        &[101, 103]
    );
    assert_eq!(outcome(ctx, AltOp::Materialize(a.id())), AltReply::Unit);
    assert_eq!(
        ctx.session().int(a.id()).unwrap().as_slice().unwrap(),
        &[2, 3, 5]
    );

    let mut region_only = Probe::new(backing, SEXP_NIL);
    entry(ctx, make_atomic, &mut region_only).unwrap();
    ctx.protect(region_only.id());
    assert_eq!(
        outcome(ctx, AltOp::Materialize(region_only.id())),
        AltReply::Unit
    );
    assert_eq!(
        ctx.session()
            .int(region_only.id())
            .unwrap()
            .as_slice()
            .unwrap(),
        &[2, 3, 5]
    );
}

unsafe extern "C" fn integer_access(data: *mut c_void) {
    // SAFETY: The probe and returned pointer are live. The region buffer holds two integers.
    unsafe {
        let x = (*data.cast::<Probe>()).output;
        assert_eq!(Rf_xlength(x), 3);
        assert_eq!(LENGTH(x), 3);
        assert_eq!(ALTREP_LENGTH(x), 3);
        assert_eq!(ALTREP(x), 1);
        let class = R_altrep_class_t {
            ptr: ALTREP_CLASS(x),
        };
        assert_eq!(R_altrep_inherits(x, class), 1);
        assert_eq!(
            R_altrep_class_name(x),
            Rf_install(c"fixture_integer".as_ptr())
        );
        assert_eq!(R_altrep_class_package(x), Rf_install(c"fixture".as_ptr()));
        assert_eq!(INTEGER_ELT(x, 1), 3);
        let mut out = [0; 2];
        assert_eq!(INTEGER_GET_REGION(x, 1, 2, out.as_mut_ptr()), 2);
        assert_eq!(out, [3, 5]);
        assert_eq!(INTEGER_OR_NULL(x).read(), 2);
        let duplicate = Rf_duplicate(x);
        assert_eq!(INTEGER_ELT(duplicate, 0), 32);
    }
}

#[test]
fn materialize_each_vector_type() {
    let _fixture = FIXTURE.lock().unwrap();
    let ctx = &mut context();
    let s = ctx.session_mut();
    let logical = s.alloc_lgl_from(&[RLogical::TRUE, RLogical::NA]).unwrap();
    let raw = s.alloc_raw_from(&[4, 7]).unwrap();
    let complex = s
        .alloc_cplx_from(&[RComplex { re: 1.0, im: -2.0 }, RComplex::NA])
        .unwrap();
    let real = s.alloc_real_from(&[1.25, 2.5]).unwrap();
    let strings = s.scalar_str("original").unwrap();
    let list = s.alloc_list(1).unwrap();
    let inputs = [logical, raw, complex, real, strings, list];
    for x in inputs {
        ctx.protect(x);
    }
    let mut foreign = Vec::new();
    for (i, x) in inputs.into_iter().enumerate() {
        let mut args = Probe::new(x, SEXP_NIL);
        entry(ctx, if i < 3 { make_atomic } else { make }, &mut args).unwrap();
        ctx.protect(args.id());
        foreign.push(args.id());
    }

    let first = ctx.session_mut().str_get(strings, 0).unwrap();
    let two_strings = ctx.session_mut().alloc_str_from(&[first; 2]).unwrap();
    ctx.protect(two_strings);
    let mut str_args = Probe::new(two_strings, SEXP_NIL);
    entry(ctx, make, &mut str_args).unwrap();
    ctx.protect(str_args.id());
    let mut mutation = [str_args.output, Sexp::from_id(foreign[5])];
    entry(ctx, mutate, &mut mutation).unwrap();

    for x in foreign.iter().copied().chain([str_args.id()]) {
        assert_eq!(outcome(ctx, AltOp::Materialize(x)), AltReply::Unit);
        assert!(ctx.session().foreign_alt(x).unwrap().unwrap().owned);
    }
    let s = ctx.session();
    assert_eq!(
        s.lgl(foreign[0]).unwrap().as_slice().unwrap(),
        &[RLogical::TRUE, RLogical::NA]
    );
    assert_eq!(s.raw(foreign[1]).unwrap().as_slice().unwrap(), &[4, 7]);
    assert_eq!(
        s.cplx(foreign[2]).unwrap().as_slice().unwrap()[0],
        RComplex { re: 1.0, im: -2.0 }
    );
    assert_eq!(
        s.real(foreign[3]).unwrap().as_slice().unwrap(),
        &[1.25, 2.5]
    );
    let changed = s.str_slice(str_args.id()).unwrap()[1];
    assert_eq!(s.char_bytes(changed).unwrap(), b"changed");
    assert_eq!(s.list_get(foreign[5], 0).unwrap(), str_args.id());
}

unsafe extern "C" fn mutate(data: *mut c_void) {
    // SAFETY: The two protected input handles are a character ALTREP vector and a list.
    unsafe {
        let [string, list] = *data.cast::<[Sexp; 2]>();
        assert_ne!(fixture_alt_mutate(string, list), Sexp::NULL);
        assert_eq!(ALTSTRING_ELT(string, 1), STRING_ELT(string, 1));
    }
}

unsafe extern "C" fn restore_old(data: *mut c_void) {
    // SAFETY: The Probe holds a protected integer vector for the C fixture.
    unsafe {
        let data = &mut *data.cast::<Probe>();
        data.output = fixture_alt_restore(data.backing, 0);
    }
}

unsafe extern "C" fn restore_new(data: *mut c_void) {
    // SAFETY: The Probe holds a protected integer vector for the C fixture.
    unsafe {
        let data = &mut *data.cast::<Probe>();
        data.output = fixture_alt_restore(data.backing, 1);
    }
}

unsafe extern "C" fn restore_access(data: *mut c_void) {
    // SAFETY: The Probe holds a protected ALTREP vector of length three.
    unsafe {
        let x = (*data.cast::<Probe>()).output;
        assert_eq!(INTEGER_ELT(Rf_shallow_duplicate(x), 0), 12);
        assert_eq!(INTEGER_ELT(Rf_duplicate(x), 0), 42);
        assert_eq!(REAL_ELT(Rf_coerceVector(x, REALSXP), 0), 2.5);
        let ptr = ALTVEC_DATAPTR(x).cast::<c_int>();
        assert_eq!(ptr.read(), 3);
        assert_eq!(ALTVEC_DATAPTR_RO(x).cast::<c_int>(), ptr.cast_const());
        SET_INTEGER_ELT(x, 0, 10);
        assert_eq!(ptr.read(), 10);
    }
}

#[test]
fn restore_latest_class_and_keep_state_after_dataptr() {
    let _fixture = FIXTURE.lock().unwrap();
    let ctx = &mut context();
    let backing = int(ctx, &[2, 3, 5]);
    let mut old = Probe::new(backing, SEXP_NIL);
    entry(ctx, restore_old, &mut old).unwrap();
    ctx.protect(old.id());
    let (class, package) = ctx.session().alt_class_names(old.id()).unwrap().unwrap();
    let restore = AltOp::Unserialize {
        class,
        package,
        ty: SexpType::Int,
        state: backing,
        attrs: SEXP_NIL,
        objf: 1,
        levels: 17,
    };
    let AltReply::Value(Some(restored)) = outcome(ctx, restore) else {
        panic!("Expected restored ALTREP vector.");
    };
    ctx.protect(restored);
    assert_eq!(ctx.altrep_len(restored).unwrap(), 3);
    assert!(ctx.session().is_object(restored).unwrap());
    let old_class = ctx.session().foreign_alt(old.id()).unwrap().unwrap().class;
    assert_eq!(
        ctx.session().foreign_alt(restored).unwrap().unwrap().class,
        old_class
    );

    let mut new = Probe::new(backing, SEXP_NIL);
    entry(ctx, restore_new, &mut new).unwrap();
    ctx.protect(new.id());
    assert_ne!(
        ctx.session().foreign_alt(new.id()).unwrap().unwrap().class,
        old_class
    );
    assert_eq!(
        ctx.session().foreign_alt(old.id()).unwrap().unwrap().class,
        old_class
    );
    let AltReply::Value(Some(result)) = outcome(ctx, restore) else {
        panic!("Expected UnserializeEX result.");
    };
    assert_eq!(
        ctx.session().int(result).unwrap().as_slice().unwrap(),
        &[17]
    );

    for (op, expected) in [
        (
            AltOp::Min {
                x: old.id(),
                na_rm: true,
            },
            72,
        ),
        (
            AltOp::Max {
                x: old.id(),
                na_rm: false,
            },
            52,
        ),
    ] {
        let result = ctx.altrep_method(op).unwrap().unwrap();
        assert_eq!(
            ctx.session().int(result).unwrap().as_slice().unwrap(),
            &[expected]
        );
    }
    // SAFETY: The method accepts a null subtree callback.
    assert_eq!(
        unsafe { inspect(ctx, old.id(), 2, 3, 4, None) }.unwrap(),
        Some(1)
    );
    let depth = ctx.session().heap.protect_depth();
    let (_, stderr) = ctx
        .capture_streams(false, true, false, |ctx| {
            entry(ctx, restore_access, &mut old)
        })
        .unwrap();
    assert!(stderr.is_empty());
    assert_eq!(ctx.session().heap.protect_depth(), depth);
    assert_eq!(
        ctx.session().int(old.id()).unwrap().as_slice().unwrap(),
        &[10, 3, 5]
    );
    assert_eq!(
        outcome(ctx, AltOp::SerializedState(old.id())),
        AltReply::Value(Some(backing))
    );
    assert_eq!(
        ctx.session().int(backing).unwrap().as_slice().unwrap(),
        &[2, 3, 5]
    );
}

unsafe extern "C" fn fail_length(data: *mut c_void) {
    // SAFETY: The Probe holds a protected vector used as the callback's data slot.
    unsafe {
        let data = &mut *data.cast::<Probe>();
        data.output = jump::Rf_protect(fixture_alt_fail(data.backing));
        XLENGTH(data.output);
    }
}

#[test]
fn callback_error_restores_protect_stack() {
    let _fixture = FIXTURE.lock().unwrap();
    let ctx = &mut context();
    let backing = int(ctx, &[2, 3, 5]);
    let depth = ctx.session().heap.protect_depth();
    let mut probe = Probe::new(backing, SEXP_NIL);
    let (result, stderr) = ctx
        .capture_streams(false, true, false, |ctx| {
            Ok(entry(ctx, fail_length, &mut probe))
        })
        .unwrap();
    assert!(result.is_err());
    assert!(String::from_utf8_lossy(&stderr).contains("Foreign Length failed."));
    assert_eq!(ctx.session().heap.protect_depth(), depth);
    let mode = int(ctx, &[3]);
    let mut no_pointer = Probe::new(backing, mode);
    entry(ctx, make, &mut no_pointer).unwrap();
    ctx.protect(no_pointer.id());
    let (result, stderr) = ctx
        .capture_streams(false, true, false, |ctx| {
            Ok(entry(ctx, no_dataptr, &mut no_pointer))
        })
        .unwrap();
    assert!(result.is_err());
    assert!(String::from_utf8_lossy(&stderr).contains(
        "cannot access data pointer for this ALTVEC object [class: fixture_no_pointer, pkg: fixture]"
    ));
    assert!(
        ctx.session()
            .foreign_alt(no_pointer.id())
            .unwrap()
            .is_some_and(|info| !info.owned)
    );
}

unsafe extern "C" fn no_dataptr(data: *mut c_void) {
    // SAFETY: The Probe holds a protected ALTREP vector with no Dataptr method.
    unsafe {
        let x = (*data.cast::<Probe>()).output;
        fixture_alt_no_pointer(x);
    }
}
