//! Object roots, transient storage, and C heap allocation.

use std::ffi::{c_char, c_int, c_long, c_void};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::ptr::{NonNull, null_mut};

use roxide_eval::{Ctx, CtxExt, EvalResult};
use roxide_gc::{SexpId, Trace, Tracer};
use roxide_host::OpenMode;
use roxide_value::{SEXP_NIL, SexpType, ValueApi};

use super::string::c_text;
use super::vector::{alloc_vector, allocating, api, id, require_type};
use crate::{abi::Sexp, cx};

unsafe extern "C" {
    fn calloc(count: usize, size: usize) -> *mut c_void;
    fn realloc(ptr: *mut c_void, size: usize) -> *mut c_void;
    fn free(ptr: *mut c_void);
}

struct Allocation {
    ptr: NonNull<c_void>,
    bytes: usize,
}

impl Drop for Allocation {
    fn drop(&mut self) {
        // SAFETY: This allocation owns a live pointer from calloc.
        unsafe { free(self.ptr.as_ptr()) };
    }
}

#[derive(Default)]
struct Arena {
    allocations: Vec<Allocation>,
    entry_marks: Vec<usize>,
}

impl Trace for Arena {
    fn trace(&self, _: &mut Tracer<'_>) {}
}

impl CtxExt for Arena {}

const PATH_BUFFER_SIZE: usize = if cfg!(target_os = "macos") {
    1024
} else {
    4096
};

struct ExpandedPath([u8; PATH_BUFFER_SIZE]);

impl Default for ExpandedPath {
    fn default() -> Self {
        Self([0; PATH_BUFFER_SIZE])
    }
}

impl Trace for ExpandedPath {
    fn trace(&self, _: &mut Tracer<'_>) {}
}

impl CtxExt for ExpandedPath {}

pub(crate) fn save_transient_frame(ctx: &mut Ctx) {
    let arena = ctx.ext::<Arena>();
    arena.entry_marks.push(arena.allocations.len());
}

pub(crate) fn release_transient_frame(ctx: &mut Ctx) {
    let arena = ctx.ext::<Arena>();
    if let Some(mark) = arena.entry_marks.pop() {
        arena.allocations.truncate(mark);
    }
}

fn bytes(ctx: &mut Ctx, count: usize, size: usize) -> EvalResult<usize> {
    count
        .checked_mul(size)
        .filter(|&bytes| bytes <= isize::MAX as usize)
        .ok_or_else(|| ctx.stop(SEXP_NIL, "allocation size is too large"))
}

fn array_bytes(ctx: &mut Ctx, count: usize, size: c_int) -> EvalResult<usize> {
    let size =
        usize::try_from(size).map_err(|_| ctx.stop(SEXP_NIL, "invalid allocation element size"))?;
    bytes(ctx, count, size)
}

fn nonnegative_count(ctx: &mut Ctx, count: c_long) -> EvalResult<usize> {
    usize::try_from(count).map_err(|_| ctx.stop(SEXP_NIL, "invalid allocation count"))
}

fn checked_pointer(ctx: &mut Ctx, ptr: *mut c_void) -> EvalResult<NonNull<c_void>> {
    NonNull::new(ptr).ok_or_else(|| ctx.stop(SEXP_NIL, "cannot allocate memory"))
}

fn transient(ctx: &mut Ctx, bytes: usize) -> EvalResult<*mut c_void> {
    if bytes == 0 {
        return Ok(null_mut());
    }
    ctx.ext::<Arena>()
        .allocations
        .try_reserve(1)
        .map_err(|_| ctx.stop(SEXP_NIL, "cannot extend transient allocation stack"))?;
    // SAFETY: The byte count was checked. The arena owns the returned allocation.
    let ptr = checked_pointer(ctx, unsafe { calloc(1, bytes) })?;
    ctx.ext::<Arena>()
        .allocations
        .push(Allocation { ptr, bytes });
    Ok(ptr.as_ptr())
}

pub(crate) fn transient_string(ctx: &mut Ctx, text: &[u8]) -> EvalResult<*const c_char> {
    let size = text
        .len()
        .checked_add(1)
        .ok_or_else(|| ctx.stop(SEXP_NIL, "allocation size is too large"))?;
    let target = transient(ctx, size)?.cast::<u8>();
    // SAFETY: The new zero-filled buffer has space for the bytes and their terminator.
    unsafe { std::ptr::copy_nonoverlapping(text.as_ptr(), target, text.len()) };
    Ok(target.cast())
}

#[cfg(unix)]
fn path_from_bytes(path: &[u8]) -> &Path {
    use std::os::unix::ffi::OsStrExt;
    Path::new(std::ffi::OsStr::from_bytes(path))
}

#[cfg(not(unix))]
fn path_from_bytes(path: &[u8]) -> &Path {
    Path::new(std::str::from_utf8(path).unwrap_or_default())
}

fn user_home(ctx: &mut Ctx, user: &[u8]) -> Option<PathBuf> {
    let host = ctx.host();
    if user.is_empty() || host.env_var("USER").is_some_and(|name| name == user) {
        return host.home_dir();
    }
    let mut passwd = host
        .open(Path::new("/etc/passwd"), OpenMode::read_only())
        .ok()?
        .take(1024 * 1024);
    let mut records = Vec::new();
    passwd.read_to_end(&mut records).ok()?;
    for record in records.split(|&byte| byte == b'\n') {
        let mut fields = record.split(|&byte| byte == b':');
        if fields.next() == Some(user)
            && let Some(home) = fields.nth(4).filter(|home| home.starts_with(b"/"))
        {
            return Some(path_from_bytes(home).to_owned());
        }
    }
    None
}

fn home_expansion(ctx: &mut Ctx, path: &[u8]) -> Option<Vec<u8>> {
    let rest = path.strip_prefix(b"~")?;
    let user_len = rest
        .iter()
        .position(|&byte| byte == b'/')
        .unwrap_or(rest.len());
    let home = user_home(ctx, &rest[..user_len])?;
    let mut output = home.into_os_string().into_encoded_bytes();
    output.extend_from_slice(&rest[user_len..]);
    Some(output)
}

fn temporary_name(
    ctx: &mut Ctx,
    prefix: &[u8],
    directory: &[u8],
    extension: &[u8],
) -> EvalResult<*mut c_char> {
    const MAX_ATTEMPTS: usize = 256;
    let size = directory
        .len()
        .saturating_add(prefix.len())
        .saturating_add(extension.len());
    let pid = ctx.host().pid();
    let mut path = Vec::new();
    path.try_reserve_exact(size.saturating_add(18))
        .map_err(|_| ctx.stop(SEXP_NIL, "cannot allocate temporary file name"))?;
    path.extend_from_slice(directory);
    path.push(b'/');
    path.extend_from_slice(prefix);
    let prefix_len = path.len();
    for _ in 0..MAX_ATTEMPTS {
        let mut entropy = [0; 4];
        ctx.host().fill_entropy(&mut entropy);
        let sequence = u32::from_ne_bytes(entropy) & (u32::MAX >> 1);
        let _ = write!(path, "{pid:x}{sequence:x}");
        path.extend_from_slice(extension);
        if ctx.host().metadata(path_from_bytes(&path), false).is_err() {
            // SAFETY: The C heap buffer has space for the path and a terminator.
            let ptr = checked_pointer(ctx, unsafe { calloc(1, path.len() + 1) })?
                .as_ptr()
                .cast();
            // SAFETY: The new C heap buffer and path are disjoint.
            unsafe { std::ptr::copy_nonoverlapping(path.as_ptr(), ptr, path.len()) };
            return Ok(ptr.cast());
        }
        path.truncate(prefix_len);
    }
    Err(ctx.stop(SEXP_NIL, "cannot find an unused temporary file name"))
}

fn mset(ctx: &mut Ctx, value: Sexp, set: Sexp, name: &str) -> EvalResult<(SexpId, SexpId, SexpId)> {
    let value = id(ctx, value, name)?;
    let set = id(ctx, set, name)?;
    require_type(ctx, set, SexpType::Vec, name)?;
    let contents = ctx.session().list_slice(set)?;
    let [members] = *contents else {
        return Err(ctx.stop(SEXP_NIL, "invalid precious multiset"));
    };
    require_type(ctx, members, SexpType::Vec, name)?;
    Ok((value, set, members))
}

api! {
    /// Add an object to the session's precious multiset.
    fn R_PreserveObject(x: Sexp) -> () = (); |ctx| {
        let x = id(ctx, x, "R_PreserveObject")?;
        ctx.session_mut().heap.preserve(x);
        Ok(())
    }

    /// Remove one occurrence from the session's precious multiset.
    fn R_ReleaseObject(x: Sexp) -> () = (); |ctx| {
        let x = id(ctx, x, "R_ReleaseObject")?;
        ctx.session_mut().heap.release(x);
        Ok(())
    }

    /// Allocate a precious multiset with the specified initial capacity.
    fn R_NewPreciousMSet(size: c_int) -> Sexp = Sexp::NULL; |ctx| {
        let size = usize::try_from(size).map_err(|_| ctx.stop(SEXP_NIL, "invalid multiset size"))?;
        allocating(ctx, &[], |ctx| {
            let members = alloc_vector(ctx, SexpType::Vec, size)?;
            let session = ctx.session_mut();
            let set = session.alloc_list(1)?;
            session.list_set(set, 0, members)?;
            Ok(Sexp::from_id(set))
        })
    }

    /// Keep an object alive while the precious multiset is reachable.
    fn R_PreserveInMSet(x: Sexp, set: Sexp) -> () = (); |ctx| {
        let (x, set, members) = mset(ctx, x, set, "R_PreserveInMSet")?;
        if x == SEXP_NIL {
            return Ok(());
        }
        let items = ctx.session().list_slice(members)?;
        if let Some(i) = items.iter().position(|&member| member == SEXP_NIL) {
            ctx.session_mut().list_set(members, i, x)?;
            return Ok(());
        }
        let len = items.len();
        allocating(ctx, &[x, set], |ctx| {
            let next = alloc_vector(ctx, SexpType::Vec, len.saturating_mul(2).max(1))?;
            let session = ctx.session_mut();
            for i in 0..len {
                let member = session.list_get(members, i)?;
                session.list_set(next, i, member)?;
            }
            session.list_set(next, len, x)?;
            session.list_set(set, 0, next)?;
            Ok(())
        })
    }

    /// Remove one occurrence from a precious multiset.
    fn R_ReleaseFromMSet(x: Sexp, set: Sexp) -> () = (); |ctx| {
        let (x, _, members) = mset(ctx, x, set, "R_ReleaseFromMSet")?;
        if let Some(i) = ctx.session().list_slice(members)?.iter().rposition(|&item| item == x) {
            ctx.session_mut().list_set(members, i, SEXP_NIL)?;
        }
        Ok(())
    }

    /// Allocate transient storage.
    fn R_alloc(count: usize, size: c_int) -> *mut c_char = null_mut(); |ctx| {
        let bytes = array_bytes(ctx, count, size)?;
        Ok(transient(ctx, bytes)?.cast())
    }

    /// Copy a C string into the transient allocation stack.
    fn acopy_string(text: *const c_char) -> *const c_char = std::ptr::null(); |ctx| {
        let text = c_text(ctx, text, "acopy_string")?;
        transient_string(ctx, text)
    }

    /// Expand a leading tilde in a path. The next call can replace the result.
    fn R_ExpandFileName(path: *const c_char) -> *const c_char = std::ptr::null(); |ctx| {
        let input = c_text(ctx, path, "R_ExpandFileName")?;
        let expanded = home_expansion(ctx, input);
        let expanded = expanded.as_deref().unwrap_or(input);
        if expanded.len() >= PATH_BUFFER_SIZE {
            ctx.warning_at(SEXP_NIL, &format!(
                "expanded path length {} would be too long for\n{}",
                expanded.len(),
                String::from_utf8_lossy(input),
            ))?;
            return Ok(path);
        }
        let expanded = expanded.to_vec();
        let result = &mut ctx.ext::<ExpandedPath>().0;
        result[..expanded.len()].copy_from_slice(&expanded);
        result[expanded.len()] = 0;
        Ok(result.as_ptr().cast())
    }

    /// Return an unused temporary file name in the specified directory.
    fn R_tmpnam(prefix: *const c_char, directory: *const c_char) -> *mut c_char = null_mut(); |ctx| {
        let prefix = if prefix.is_null() { b"" } else { c_text(ctx, prefix, "R_tmpnam")? };
        let directory = c_text(ctx, directory, "R_tmpnam")?;
        temporary_name(ctx, prefix, directory, b"")
    }

    /// Return an unused temporary file name with the specified extension.
    fn R_tmpnam2(prefix: *const c_char, directory: *const c_char, extension: *const c_char) -> *mut c_char = null_mut(); |ctx| {
        let prefix = if prefix.is_null() { b"" } else { c_text(ctx, prefix, "R_tmpnam2")? };
        let directory = c_text(ctx, directory, "R_tmpnam2")?;
        let extension = if extension.is_null() { b"" } else { c_text(ctx, extension, "R_tmpnam2")? };
        temporary_name(ctx, prefix, directory, extension)
    }

    /// Allocate transient storage aligned for C long double.
    fn R_allocLD(count: usize) -> *mut c_void = null_mut(); |ctx| {
        // The supported 64-bit targets have a long double of at most 16 bytes.
        let bytes = bytes(ctx, count, 16)?;
        transient(ctx, bytes)
    }

    /// Allocate zero-filled transient storage.
    fn S_alloc(count: c_long, size: c_int) -> *mut c_char = null_mut(); |ctx| {
        let count = nonnegative_count(ctx, count)?;
        let bytes = array_bytes(ctx, count, size)?;
        Ok(transient(ctx, bytes)?.cast())
    }

    /// Grow a transient array and zero-fill its new elements.
    fn S_realloc(ptr: *mut c_char, count: c_long, old_count: c_long, size: c_int) -> *mut c_char = null_mut(); |ctx| {
        let count = nonnegative_count(ctx, count)?;
        let old_count = nonnegative_count(ctx, old_count)?;
        let old_bytes = array_bytes(ctx, old_count, size)?;
        let new_bytes = array_bytes(ctx, count, size)?;
        if old_bytes != 0 && !ctx.ext::<Arena>().allocations.iter().any(|allocation| {
            allocation.ptr.as_ptr() == ptr.cast() && old_bytes <= allocation.bytes
        }) {
            return Err(ctx.stop(SEXP_NIL, "invalid transient allocation"));
        }
        if new_bytes <= old_bytes {
            return Ok(ptr);
        }
        let target = transient(ctx, new_bytes)?.cast();
        if old_bytes != 0 {
            // SAFETY: The arena owns both ranges, and the new buffer is disjoint.
            std::ptr::copy_nonoverlapping(ptr, target, old_bytes);
        }
        Ok(target)
    }

    /// Return a mark for the current transient allocation stack.
    fn vmaxget() -> *mut c_void = null_mut(); |ctx| {
        Ok(ctx.ext::<Arena>().allocations.last().map_or(null_mut(), |allocation| allocation.ptr.as_ptr()))
    }

    /// Release transient allocations made after the mark.
    fn vmaxset(mark: *const c_void) -> () = (); |ctx| {
        let arena = ctx.ext::<Arena>();
        let keep = if mark.is_null() {
            0
        } else if let Some(i) = arena.allocations.iter().position(|allocation| allocation.ptr.as_ptr() == mark.cast_mut()) {
            i + 1
        } else {
            return Err(ctx.stop(SEXP_NIL, "invalid transient allocation mark"));
        };
        if keep < arena.entry_marks.last().copied().unwrap_or(0) {
            return Err(ctx.stop(SEXP_NIL, "transient allocation mark belongs to an outer frame"));
        }
        arena.allocations.truncate(keep);
        Ok(())
    }

    /// Run a full garbage collection.
    fn R_gc() -> () = (); |ctx| {
        ctx.collect_gc();
        Ok(())
    }

    /// Check the C stack for overflow.
    fn R_CheckStack() -> () = (); |ctx| {
        check_stack(ctx, 0)
    }

    /// Check whether the C stack has room for another allocation.
    fn R_CheckStack2(extra: usize) -> () = (); |ctx| {
        check_stack(ctx, extra)
    }
}

fn check_stack(ctx: &mut Ctx, extra: usize) -> EvalResult<()> {
    if cx::depth() >= cx::MAX_DEPTH || extra >= stack_available(ctx) {
        return Err(ctx.stop(SEXP_NIL, "C stack usage is too close to the limit"));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn stack_available(_: &Ctx) -> usize {
    unsafe extern "C" {
        fn pthread_self() -> *mut c_void;
        fn pthread_get_stackaddr_np(thread: *mut c_void) -> *mut c_void;
        fn pthread_get_stacksize_np(thread: *mut c_void) -> usize;
    }

    let local = 0u8;
    let current = std::ptr::from_ref(&local).addr();
    // SAFETY: The current thread has a valid pthread descriptor.
    let (top, size) = unsafe {
        let thread = pthread_self();
        (
            pthread_get_stackaddr_np(thread).addr(),
            pthread_get_stacksize_np(thread),
        )
    };
    let reserved = size / 20;
    let limit = top.saturating_sub(size).saturating_add(reserved);
    current.saturating_sub(limit)
}

#[cfg(not(target_os = "macos"))]
fn stack_available(ctx: &Ctx) -> usize {
    ctx.stack_budget()
}

unsafe extern "C" {
    fn abort() -> !;
}

fn checked_c_heap(ptr: *mut c_void) -> *mut c_void {
    if ptr.is_null() {
        // SAFETY: An R error can jump only when a foreign trampoline is active.
        unsafe {
            if cx::can_jump() {
                crate::jump::boundary((), |ctx| Err(ctx.stop(SEXP_NIL, "cannot allocate memory")));
            }
            abort();
        }
    }
    ptr
}

/// Allocate a zero-filled buffer in the C heap.
#[allow(non_snake_case)]
#[unsafe(no_mangle)]
pub extern "C" fn R_chk_calloc(count: usize, size: usize) -> *mut c_void {
    let (count, size) = if count == 0 || size == 0 {
        (1, 1)
    } else {
        (count, size)
    };
    // SAFETY: calloc accepts any counts and verifies their product.
    checked_c_heap(unsafe { calloc(count, size) })
}

/// Resize a C heap buffer.
///
/// # Safety
/// Pass NULL or a live pointer allocated by the C allocator.
#[allow(non_snake_case)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_chk_realloc(ptr: *mut c_void, size: usize) -> *mut c_void {
    // SAFETY: The caller supplies a pointer accepted by realloc.
    checked_c_heap(unsafe { realloc(ptr, size.max(1)) })
}

/// Free a C heap buffer.
///
/// # Safety
/// Pass NULL or a live pointer allocated by the C allocator.
#[allow(non_snake_case)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_chk_free(ptr: *mut c_void) {
    // SAFETY: The caller supplies a pointer accepted by free.
    unsafe { free(ptr) };
}

/// Free a name allocated by `R_tmpnam` or `R_tmpnam2`.
///
/// # Safety
/// Pass NULL or a live pointer returned by one of those functions.
#[allow(non_snake_case)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_free_tmpnam(name: *mut c_char) {
    // SAFETY: The caller supplies NULL or a live allocation from the C heap.
    unsafe { free(name.cast()) };
}

/// Copy bytes between disjoint C buffers.
///
/// # Safety
/// For a nonzero size, both ranges must be valid for that size and must not overlap.
#[allow(non_snake_case)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_chk_memcpy(
    target: *mut c_void,
    source: *const c_void,
    size: usize,
) -> *mut c_void {
    if size != 0 {
        // SAFETY: The caller supplies two disjoint ranges of the specified size.
        unsafe { std::ptr::copy_nonoverlapping(source.cast::<u8>(), target.cast(), size) };
    }
    target
}

/// Fill a C buffer with a byte.
///
/// # Safety
/// For a nonzero size, the destination must be valid for that size.
#[allow(non_snake_case)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_chk_memset(
    target: *mut c_void,
    value: c_int,
    size: usize,
) -> *mut c_void {
    if size != 0 {
        // SAFETY: The caller supplies a writable range of the specified size.
        unsafe { std::ptr::write_bytes(target.cast::<u8>(), value as u8, size) };
    }
    target
}
