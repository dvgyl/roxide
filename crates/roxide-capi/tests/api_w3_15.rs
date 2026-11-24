//! Exercise the allocation, conversion, random, and numerical C entry points.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::io::{Error, ErrorKind};
use std::ptr::{from_mut, null_mut};
use std::slice;

use roxide_capi::api::{
    error::UNIMPLEMENTED, gc_alloc::*, iconv::*, optim_integrate::*, random::*,
};
use roxide_capi::jump::{VoidFn, enter_trampoline};
use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::PureHost;
use roxide_value::{GLOBAL_ENV, ValueApi};

unsafe extern "C" {
    fn free(ptr: *mut c_void);
    fn Rf_error(format: *const c_char, ...) -> !;
}

fn context() -> Ctx {
    Ctx::new(Box::new(PureHost::new()), GcPolicy::Torture).unwrap()
}

fn run<T>(ctx: &mut Ctx, fun: VoidFn, data: &mut T) {
    // SAFETY: The probe uses the live descriptor for this call only.
    unsafe { enter_trampoline(ctx, fun, from_mut(data).cast()) }.unwrap();
    assert_eq!(ctx.session().heap.protect_depth(), 0);
}

#[test]
fn allocation_and_failed_resize() {
    // SAFETY: All memory reads and writes stay inside live C allocations.
    unsafe {
        let ptr = R_malloc_gc(8).cast::<u8>();
        assert!(!ptr.is_null());
        ptr.write(42);
        assert!(R_realloc_gc(ptr.cast(), usize::MAX).is_null());
        assert_eq!(ptr.read(), 42);
        let grown = R_realloc_gc(ptr.cast(), 32).cast::<u8>();
        assert!(!grown.is_null());
        assert_eq!(grown.read(), 42);
        free(grown.cast());
        let zeroed = R_calloc_gc(4, 8).cast::<u8>();
        assert!(!zeroed.is_null());
        assert_eq!(slice::from_raw_parts(zeroed, 32), &[0; 32]);
        free(zeroed.cast());
        assert!(R_calloc_gc(usize::MAX, 2).is_null());
    }
}

unsafe extern "C" fn allocation_failure(data: *mut c_void) {
    // SAFETY: The probe receives a writable bool and uses a size malloc cannot serve.
    unsafe { data.cast::<bool>().write(R_malloc_gc(usize::MAX).is_null()) }
}

#[test]
fn allocation_failure_collects_the_active_session() {
    let mut ctx = context();
    let garbage = ctx.session_mut().alloc_int_from(&[42]).unwrap();
    assert!(ctx.session().heap.get(garbage).is_ok());
    let mut failed = false;
    run(&mut ctx, allocation_failure, &mut failed);
    assert!(failed);
    assert!(ctx.session().heap.get(garbage).is_err());
}

#[test]
fn iconv_preserves_buffers_and_errno() {
    // SAFETY: Strings are terminated. Conversion buffers and counts are live.
    unsafe {
        let invalid = usize::MAX;
        let descriptor = Riconv_open(c"utf8".as_ptr(), c"latin1".as_ptr());
        assert_ne!(descriptor.addr(), invalid);
        let latin1 = [0xe9_u8];
        let mut utf8 = [0_u8; 2];
        let mut input = latin1.as_ptr().cast();
        let mut output = utf8.as_mut_ptr().cast();
        let (mut input_left, mut output_left) = (1, 1);
        let convert = Riconv(
            descriptor,
            &mut input,
            &mut input_left,
            &mut output,
            &mut output_left,
        );
        let errno = Error::last_os_error().raw_os_error();
        assert_eq!(convert, invalid);
        assert_eq!(errno, Some(7)); // E2BIG on Darwin and Linux.
        assert_eq!((input_left, output_left), (1, 1));
        assert_eq!(input, latin1.as_ptr().cast());
        assert_eq!(output, utf8.as_mut_ptr().cast());
        output_left = 2;
        assert_eq!(
            Riconv(
                descriptor,
                &mut input,
                &mut input_left,
                &mut output,
                &mut output_left
            ),
            0
        );
        assert_eq!(utf8, [0xc3, 0xa9]);
        assert_eq!((input_left, output_left), (0, 0));
        assert_eq!(input, latin1.as_ptr().add(1).cast());
        assert_eq!(output, utf8.as_mut_ptr().add(2).cast());
        assert_eq!(
            Riconv(descriptor, null_mut(), null_mut(), null_mut(), null_mut()),
            0
        );
        assert_eq!(Riconv_close(descriptor), 0);

        let descriptor = Riconv_open(c"".as_ptr(), c"UTF-8".as_ptr());
        assert_ne!(descriptor.addr(), invalid);
        for (byte, malformed) in [(0xff_u8, true), (0xc3, false)] {
            let mut input = (&byte as *const u8).cast();
            let mut output = utf8.as_mut_ptr().cast();
            let (mut input_left, mut output_left) = (1, 2);
            assert_eq!(
                Riconv(
                    descriptor,
                    &mut input,
                    &mut input_left,
                    &mut output,
                    &mut output_left
                ),
                invalid
            );
            let errno = Error::last_os_error();
            if !malformed {
                assert_eq!(errno.kind(), ErrorKind::InvalidInput);
            } else {
                let eilseq = if cfg!(target_os = "macos") { 92 } else { 84 };
                assert_eq!(errno.raw_os_error(), Some(eilseq));
            }
            assert_eq!((input_left, output_left), (1, 2));
        }
        assert_eq!(Riconv_close(descriptor), 0);
        for name in [c"not-an-encoding", c"bytes"] {
            let descriptor = Riconv_open(c"UTF-8".as_ptr(), name.as_ptr());
            let errno = Error::last_os_error().kind();
            assert_eq!(descriptor.addr(), invalid);
            assert_eq!(errno, ErrorKind::InvalidInput);
        }
    }
}

unsafe extern "C" fn query_sample_kind(data: *mut c_void) {
    // SAFETY: The probe receives a writable int and an active trampoline.
    unsafe { data.cast::<c_int>().write(R_sample_kind()) }
}

#[test]
fn sampling_kind_does_not_create_or_change_seed() {
    let mut ctx = context();
    let seed = ctx.session_mut().intern(".Random.seed").unwrap();
    let mut kind: c_int = -1;
    run(&mut ctx, query_sample_kind, &mut kind);
    assert_eq!(kind, 1);
    assert!(ctx.session().env_get(GLOBAL_ENV, seed).is_none());
    let value = ctx.session_mut().alloc_int_from(&[403]).unwrap();
    ctx.assign(seed, value, GLOBAL_ENV).unwrap();
    run(&mut ctx, query_sample_kind, &mut kind);
    assert_eq!(kind, 0);
    assert_eq!(ctx.get_local(seed, GLOBAL_ENV).unwrap(), Some(value));
    assert_eq!(ctx.session().int(value).unwrap().get(0), 403);
}

unsafe extern "C" fn unimplemented(_: *mut c_void) {
    // SAFETY: The probe runs under a foreign trampoline.
    unsafe { UNIMPLEMENTED(c"probe %s".as_ptr()) }
}

#[test]
fn unimplemented_uses_literal_message() {
    let mut ctx = context();
    // SAFETY: The probe accepts null data and the trampoline handles the jump.
    assert!(unsafe { enter_trampoline(&mut ctx, unimplemented, null_mut()) }.is_err());
    assert_eq!(
        ctx.last_error_message(),
        "Error: unimplemented feature in probe %s\n"
    );
    assert_eq!(ctx.session().heap.protect_depth(), 0);
}

unsafe extern "C-unwind" fn quadratic(n: c_int, x: *mut f64, _: *mut c_void) -> f64 {
    // SAFETY: The adapter provides n readable coordinates.
    unsafe {
        slice::from_raw_parts(x, n as usize)
            .iter()
            .map(|v| (v - 1.0).powi(2))
            .sum()
    }
}

unsafe extern "C-unwind" fn gradient(n: c_int, x: *mut f64, g: *mut f64, _: *mut c_void) {
    // SAFETY: The adapter provides n readable coordinates and writable derivatives.
    unsafe {
        for i in 0..n as usize {
            g.add(i).write(2.0 * (x.add(i).read() - 1.0));
        }
    }
}

struct Optimum {
    x: [f64; 4],
    value: f64,
    status: c_int,
    fn_count: c_int,
    gr_count: c_int,
    message: [c_char; 60],
}

impl Default for Optimum {
    fn default() -> Self {
        Self {
            x: [0.0; 4],
            value: f64::NAN,
            status: -1,
            fn_count: -1,
            gr_count: -1,
            message: [0; 60],
        }
    }
}

unsafe extern "C" fn minimize(data: *mut c_void) {
    // SAFETY: All callbacks and buffers meet Applic.h. The descriptor is live.
    unsafe {
        let out = &mut *data.cast::<[Optimum; 6]>();
        let mut initial = [6.0, 4.0, 3.0, 5.0];
        let first = &mut out[0];
        nmmin(
            2,
            initial.as_mut_ptr(),
            first.x.as_mut_ptr(),
            &mut first.value,
            Some(quadratic),
            &mut first.status,
            f64::NEG_INFINITY,
            1e-12,
            null_mut(),
            1.2,
            0.4,
            2.2,
            0,
            &mut first.fn_count,
            1000,
        );
        let second = &mut out[1];
        second.x = [6.0, 4.0, 3.0, 5.0];
        vmmin(
            2,
            second.x.as_mut_ptr(),
            &mut second.value,
            Some(quadratic),
            Some(gradient),
            200,
            0,
            [1, 0].as_mut_ptr(),
            f64::NEG_INFINITY,
            1e-12,
            1,
            null_mut(),
            &mut second.fn_count,
            &mut second.gr_count,
            &mut second.status,
        );
        for (formula, result) in out[2..5].iter_mut().enumerate() {
            initial = [6.0, 4.0, 3.0, 5.0];
            cgmin(
                2,
                initial.as_mut_ptr(),
                result.x.as_mut_ptr(),
                &mut result.value,
                Some(quadratic),
                Some(gradient),
                &mut result.status,
                f64::NEG_INFINITY,
                1e-10,
                null_mut(),
                formula as c_int + 1,
                0,
                &mut result.fn_count,
                &mut result.gr_count,
                200,
            );
        }
        let last = &mut out[5];
        last.x = initial;
        lbfgsb(
            4,
            5,
            last.x.as_mut_ptr(),
            [f64::NAN, 2.0, -2.0, f64::NAN].as_mut_ptr(),
            [f64::NAN, f64::NAN, 0.5, -1.0].as_mut_ptr(),
            [0, 1, 2, 3].as_mut_ptr(),
            &mut last.value,
            Some(quadratic),
            Some(gradient),
            &mut last.status,
            null_mut(),
            1e7,
            1e-9,
            &mut last.fn_count,
            &mut last.gr_count,
            100,
            last.message.as_mut_ptr(),
            0,
            1,
        );
    }
}

#[test]
fn deterministic_optimizers_honor_methods_masks_and_bounds() {
    let mut out: [Optimum; 6] = std::array::from_fn(|_| Optimum::default());
    run(&mut context(), minimize, &mut out);
    for (i, result) in out.iter().enumerate() {
        assert_eq!(result.status, 0, "method {i}");
        assert!(result.fn_count > 0);
        if i != 0 {
            assert!(result.gr_count > 0);
        }
        assert!(
            (result.x[0] - 1.0).abs() < 1e-3,
            "method {i}: {:?}",
            result.x
        );
        let (second, value) = match i {
            1 => (4.0, 9.0),
            5 => (2.0, 5.25),
            _ => (1.0, 0.0),
        };
        assert!((result.x[1] - second).abs() < 1e-3);
        assert!((result.value - value).abs() < 1e-8);
    }
    assert_eq!(out[5].x[2..], [0.5, -1.0]);
    // SAFETY: lbfgsb terminates its message in the 60-byte array.
    let message = unsafe { CStr::from_ptr(out[5].message.as_ptr()) };
    assert!(message.to_bytes().starts_with(b"CONVERGENCE"));
}

unsafe extern "C" fn anneal(data: *mut c_void) {
    // SAFETY: The descriptor holds two writable doubles and the callback accepts n=1.
    unsafe {
        let [x, minimum] = &mut *data.cast::<[f64; 2]>();
        samin(1, x, minimum, Some(quadratic), 8000, 50, 1.0, 0, null_mut());
    }
}

#[test]
fn annealing_advances_and_saves_the_seed() {
    let mut ctx = context();
    let name = ctx.session_mut().intern(".Random.seed").unwrap();
    let rng = roxide_math::rng::Rng::seeded(42, roxide_math::rng::Kinds::default());
    let mut words = Vec::new();
    rng.encode(&mut words);
    let seed = ctx.session_mut().alloc_int_from(&words).unwrap();
    ctx.assign(name, seed, GLOBAL_ENV).unwrap();
    let mut out = [2.0_f64, 1.0];
    run(&mut ctx, anneal, &mut out);
    assert!((out[0] - 1.0).abs() < 0.01);
    assert!(out[1] < 1e-4);
    assert_ne!(ctx.get_local(name, GLOBAL_ENV).unwrap(), Some(seed));
}

const LIMIT: usize = 50;

struct Integral {
    result: f64,
    error: f64,
    count: c_int,
    status: c_int,
    last: c_int,
    order: [c_int; LIMIT],
    work: [f64; 4 * LIMIT],
}

impl Default for Integral {
    fn default() -> Self {
        Self {
            result: f64::NAN,
            error: f64::NAN,
            count: -1,
            status: -1,
            last: -1,
            order: [-1; LIMIT],
            work: [f64::NAN; 4 * LIMIT],
        }
    }
}

unsafe extern "C-unwind" fn square(x: *mut f64, n: c_int, _: *mut c_void) {
    // SAFETY: The integrator supplies n writable samples.
    for x in unsafe { slice::from_raw_parts_mut(x, n as usize) } {
        *x *= *x;
    }
}

unsafe extern "C-unwind" fn exponential(x: *mut f64, n: c_int, data: *mut c_void) {
    // SAFETY: The integrator supplies n writable samples and a live counter.
    unsafe {
        *data.cast::<c_int>() += n;
        for x in slice::from_raw_parts_mut(x, n as usize) {
            *x = (-x.abs()).exp();
        }
    }
}

unsafe extern "C" fn quadrature(data: *mut c_void) {
    // SAFETY: The descriptor and the quadrature work arrays are live.
    unsafe {
        let [finite, semi, whole, invalid] = &mut *data.cast::<[Integral; 4]>();
        let limit = LIMIT as c_int;
        let lenw = 4 * limit;
        Rdqags(
            Some(square),
            null_mut(),
            &0.0,
            &1.0,
            &0.0,
            &1e-10,
            &mut finite.result,
            &mut finite.error,
            &mut finite.count,
            &mut finite.status,
            &limit,
            &lenw,
            &mut finite.last,
            finite.order.as_mut_ptr(),
            finite.work.as_mut_ptr(),
        );
        for (direction, out) in [(1, semi), (2, whole)] {
            let mut actual_count: c_int = 0;
            Rdqagi(
                Some(exponential),
                from_mut(&mut actual_count).cast(),
                &0.0,
                &direction,
                &0.0,
                &1e-10,
                &mut out.result,
                &mut out.error,
                &mut out.count,
                &mut out.status,
                &limit,
                &lenw,
                &mut out.last,
                out.order.as_mut_ptr(),
                out.work.as_mut_ptr(),
            );
            assert_eq!(actual_count, out.count);
        }
        Rdqags(
            Some(square),
            null_mut(),
            &0.0,
            &1.0,
            &0.0,
            &1e-10,
            &mut invalid.result,
            &mut invalid.error,
            &mut invalid.count,
            &mut invalid.status,
            &limit,
            &(lenw - 1),
            &mut invalid.last,
            invalid.order.as_mut_ptr(),
            invalid.work.as_mut_ptr(),
        );
    }
}

#[test]
fn quadrature_writes_results_counts_and_workspace() {
    let mut out: [Integral; 4] = std::array::from_fn(|_| Integral::default());
    run(&mut context(), quadrature, &mut out);
    for (i, result) in out[..3].iter().enumerate() {
        let expected = [1.0 / 3.0, 1.0, 2.0][i];
        assert_eq!(result.status, 0);
        assert!((result.result - expected).abs() < 1e-10);
        assert!((0.0..1e-9).contains(&result.error));
        assert_eq!(result.count, [21, 15, 30][i] * (2 * result.last - 1));
        let n = result.last as usize;
        let panel_sum = result.work[2 * LIMIT..2 * LIMIT + n].iter().sum::<f64>();
        assert!((panel_sum - expected).abs() < 1e-10);
        let mut intervals: Vec<_> = (0..n)
            .map(|i| (result.work[i], result.work[LIMIT + i]))
            .collect();
        intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert_eq!(intervals[0].0, 0.0);
        assert_eq!(intervals[n - 1].1, 1.0);
        assert!(intervals.windows(2).all(|pair| pair[0].1 == pair[1].0));
        let mut order = result.order[..n].to_vec();
        order.sort_unstable();
        assert_eq!(order, (1..=n as c_int).collect::<Vec<_>>());
    }
    assert_eq!(out[0].last, 1);
    assert!(out[1].last > 1);
    let invalid = &out[3];
    assert_eq!(
        (
            invalid.result,
            invalid.error,
            invalid.count,
            invalid.last,
            invalid.status
        ),
        (0.0, 0.0, 0, 0, 6)
    );
    assert!(invalid.work.iter().all(|x| x.is_nan()));
}

unsafe extern "C-unwind" fn bad_integrand(_: *mut f64, _: c_int, _: *mut c_void) {
    // SAFETY: The adapter provides a nested foreign trampoline.
    unsafe { Rf_error(c"integrand failed".as_ptr()) }
}

unsafe extern "C-unwind" fn bad_objective(_: c_int, _: *mut f64, _: *mut c_void) -> f64 {
    // SAFETY: The adapter provides a nested foreign trampoline.
    unsafe { Rf_error(c"objective failed".as_ptr()) }
}

unsafe extern "C-unwind" fn bad_gradient(_: c_int, _: *mut f64, _: *mut f64, _: *mut c_void) {
    // SAFETY: The adapter provides a nested foreign trampoline.
    unsafe { Rf_error(c"gradient failed".as_ptr()) }
}

unsafe extern "C" fn fail_callback(data: *mut c_void) {
    // SAFETY: The descriptor selects a callback. All work arrays live until
    // this function returns or the outer foreign trampoline receives the jump.
    unsafe {
        let kind = data.cast::<c_int>().read();
        let mut out = Integral::default();
        let mut x = 3.0;
        match kind {
            0 => Rdqags(
                Some(bad_integrand),
                null_mut(),
                &0.0,
                &1.0,
                &1e-8,
                &1e-8,
                &mut out.result,
                &mut out.error,
                &mut out.count,
                &mut out.status,
                &(LIMIT as c_int),
                &(4 * LIMIT as c_int),
                &mut out.last,
                out.order.as_mut_ptr(),
                out.work.as_mut_ptr(),
            ),
            1 => nmmin(
                1,
                &mut x,
                out.work.as_mut_ptr(),
                &mut out.result,
                Some(bad_objective),
                &mut out.status,
                0.0,
                1e-8,
                null_mut(),
                1.0,
                0.5,
                2.0,
                0,
                &mut out.count,
                10,
            ),
            _ => vmmin(
                1,
                &mut x,
                &mut out.result,
                Some(quadratic),
                Some(bad_gradient),
                10,
                0,
                &mut 1,
                0.0,
                1e-8,
                1,
                null_mut(),
                &mut out.count,
                &mut out.last,
                &mut out.status,
            ),
        }
    }
}

#[test]
fn callback_errors_return_through_the_foreign_trampoline() {
    let mut ctx = context();
    for (mut kind, message) in [
        (0, "integrand failed"),
        (1, "objective failed"),
        (2, "gradient failed"),
    ] {
        // SAFETY: The selector is live until the foreign trampoline returns.
        let result =
            unsafe { enter_trampoline(&mut ctx, fail_callback, from_mut(&mut kind).cast()) };
        assert!(result.is_err());
        assert_eq!(ctx.last_error_message(), format!("Error: {message}\n"));
        assert_eq!(ctx.session().heap.protect_depth(), 0);
    }
    let mut kind = -1;
    run(&mut ctx, query_sample_kind, &mut kind);
    assert_eq!(kind, 1);
}
