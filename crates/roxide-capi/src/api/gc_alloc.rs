//! C heap allocation with one retry after garbage collection.

#![allow(non_snake_case)]

use std::ffi::c_void;
use std::ptr::null_mut;

use crate::cx;

unsafe extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    fn calloc(count: usize, size: usize) -> *mut c_void;
    fn realloc(ptr: *mut c_void, size: usize) -> *mut c_void;
}

fn allocate(size: usize, alloc: impl Fn() -> *mut c_void) -> *mut c_void {
    let ptr = alloc();
    if ptr.is_null() && size != 0 && cx::can_jump() {
        cx::with_full(|ctx| ctx.collect_gc());
        return alloc();
    }
    ptr
}

/// Allocate C heap storage. Release successful allocations with C `free`.
#[unsafe(no_mangle)]
pub extern "C" fn R_malloc_gc(size: usize) -> *mut c_void {
    // SAFETY: malloc accepts any size and reports failure with NULL.
    allocate(size, || unsafe { malloc(size) })
}

/// Allocate zeroed C heap storage. Release successful allocations with C `free`.
#[unsafe(no_mangle)]
pub extern "C" fn R_calloc_gc(count: usize, size: usize) -> *mut c_void {
    let Some(bytes) = count.checked_mul(size) else {
        return null_mut();
    };
    // SAFETY: The byte count fits in size_t.
    allocate(bytes, || unsafe { calloc(count, size) })
}

/// Resize a C heap allocation. A failed nonzero resize leaves `ptr` valid.
///
/// # Safety
/// `ptr` must be null or a live pointer from the C allocator.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_realloc_gc(ptr: *mut c_void, size: usize) -> *mut c_void {
    // SAFETY: The caller supplies a valid pointer. Only a failed nonzero
    // resize can retry, and realloc leaves the allocation live in that case.
    allocate(size, || unsafe { realloc(ptr, size) })
}
