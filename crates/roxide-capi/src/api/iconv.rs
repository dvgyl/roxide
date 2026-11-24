//! Encoding conversion through the system iconv library.

#![allow(non_snake_case)]

use std::ffi::{CStr, c_char, c_int, c_void};

#[cfg_attr(any(target_os = "macos", target_os = "ios"), link(name = "iconv"))]
unsafe extern "C" {
    fn iconv_open(to: *const c_char, from: *const c_char) -> *mut c_void;
    fn iconv(
        descriptor: *mut c_void,
        input: *mut *const c_char,
        input_left: *mut usize,
        output: *mut *mut c_char,
        output_left: *mut usize,
    ) -> usize;
    fn iconv_close(descriptor: *mut c_void) -> c_int;
}

fn encoding(name: &CStr) -> &CStr {
    let bytes = name.to_bytes();
    if bytes.is_empty()
        || bytes.eq_ignore_ascii_case(b"utf8")
        || bytes.eq_ignore_ascii_case(b"utf-8")
    {
        c"UTF-8"
    } else if bytes.eq_ignore_ascii_case(b"latin1") || bytes.eq_ignore_ascii_case(b"latin-1") {
        c"ISO-8859-1"
    } else if bytes.eq_ignore_ascii_case(b"ascii") {
        c"ASCII"
    } else {
        name
    }
}

/// Open a converter. Return `(void *)-1` and set `errno` on failure.
///
/// # Safety
/// Both names must point to NUL-terminated strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Riconv_open(to: *const c_char, from: *const c_char) -> *mut c_void {
    // SAFETY: The caller supplies strings. Normalized names live through the call.
    unsafe {
        iconv_open(
            encoding(CStr::from_ptr(to)).as_ptr(),
            encoding(CStr::from_ptr(from)).as_ptr(),
        )
    }
}

/// Convert bytes or reset the converter. Update the buffer pointers and counts.
///
/// # Safety
/// Pass a live converter and buffers that meet the system `iconv` contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Riconv(
    descriptor: *mut c_void,
    input: *mut *const c_char,
    input_left: *mut usize,
    output: *mut *mut c_char,
    output_left: *mut usize,
) -> usize {
    // SAFETY: The caller supplies the descriptor and buffers required by iconv.
    unsafe { iconv(descriptor, input, input_left, output, output_left) }
}

/// Close a converter and return the system status.
///
/// # Safety
/// Pass a live converter returned by `Riconv_open`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Riconv_close(descriptor: *mut c_void) -> c_int {
    // SAFETY: The caller gives up its live descriptor.
    unsafe { iconv_close(descriptor) }
}
