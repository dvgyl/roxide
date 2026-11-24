#![deny(unsafe_op_in_unsafe_fn)]

//! Staged `libR` archive and launcher entry point (ADR-0020).
//!
//! Linked into `target/rhome/lib/libR.{dylib,so}` with an explicit manifest export list
//! so compiled packages resolve the supported C API symbols against Roxide.

use std::ffi::{CStr, c_char, c_int};

/// C-callable launcher entry point invoked by the staged `bin/R` / `bin/Rscript` wrapper.
///
/// # Safety
/// `argv` must either be null (when `argc <= 0`) or point to `argc` valid NUL-terminated C strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn roxide_main(argc: c_int, argv: *const *const c_char) -> c_int {
    let mut args = Vec::new();
    if argc > 0 && !argv.is_null() {
        for i in 0..(argc as isize) {
            // SAFETY: caller guarantees `argv` has at least `argc` valid entries.
            let ptr = unsafe { *argv.offset(i) };
            if ptr.is_null() {
                args.push(String::new());
            } else {
                // SAFETY: caller guarantees each non-null `argv[i]` is NUL-terminated.
                let bytes = unsafe { CStr::from_ptr(ptr) }.to_bytes();
                let decoded = match std::str::from_utf8(bytes) {
                    Ok(valid) => valid.to_owned(),
                    Err(_) => String::from_utf8_lossy(bytes).into_owned(),
                };
                args.push(decoded);
            }
        }
    }
    if args.is_empty() {
        args.push(String::from("roxide"));
    }
    std::panic::catch_unwind(|| roxide_cli::run_args_with(args, roxide_capi::hooks::install))
        .unwrap_or(1)
}
