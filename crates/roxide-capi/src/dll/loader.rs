//! Load and unload native libraries. Track which session owns them.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, Weak};

use libloading::os::unix::{Library, RTLD_GLOBAL, RTLD_LAZY, RTLD_LOCAL, RTLD_NOW};
use roxide_eval::{Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::SEXP_NIL;

use super::make_dll;
use super::registry::{Registry, remove, run_dll_exit_finalizers, with_owner};
use crate::jump::{FrameKind, enter_trampoline_with};
use crate::{binfmt, dotcall};

static NEXT_ID: AtomicU32 = AtomicU32::new(1);
static OWNERS: Mutex<Owners> = Mutex::new(Owners {
    session: Weak::new(),
    images: Vec::new(),
});

struct Owners {
    session: Weak<()>,
    images: Vec<OwnedImage>,
}

struct OwnedImage {
    path: PathBuf,
    id: u32,
    poisoned: bool,
}

const LIBR: &str = if cfg!(target_os = "macos") {
    "libR.dylib"
} else {
    "libR.so"
};

pub(super) struct Image {
    pub(super) library: Library,
    pub(super) owner: ImageOwner,
}

pub(super) struct ImageOwner {
    pub(super) path: PathBuf,
    id: u32,
}

impl ImageOwner {
    fn poison(&self) {
        let mut owners = OWNERS.lock().unwrap_or_else(|err| err.into_inner());
        if let Some(image) = owners.images.iter_mut().find(|image| image.id == self.id) {
            image.poisoned = true;
        }
    }
}

impl Drop for ImageOwner {
    fn drop(&mut self) {
        OWNERS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .images
            .retain(|image| image.id != self.id || image.poisoned);
    }
}

#[repr(C)]
struct DlAddr {
    path: *const c_char,
    base: *mut c_void,
    symbol: *const c_char,
    address: *mut c_void,
}

#[cfg_attr(any(target_os = "linux", target_os = "android"), link(name = "dl"))]
unsafe extern "C" {
    fn dladdr(address: *const c_void, info: *mut DlAddr) -> c_int;
    fn dlopen(path: *const c_char, flags: c_int) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
    fn dlerror() -> *mut c_char;
}

pub(super) fn image_path(address: usize) -> Option<PathBuf> {
    let mut info = std::mem::MaybeUninit::<DlAddr>::zeroed();
    // SAFETY: dladdr looks up the address without dereferencing it and initializes info on success.
    if unsafe {
        dladdr(
            std::ptr::with_exposed_provenance(address),
            info.as_mut_ptr(),
        )
    } == 0
    {
        return None;
    }
    // SAFETY: The fields are zero-initialized and dladdr returned success.
    let path = unsafe { info.assume_init().path };
    if path.is_null() {
        return None;
    }
    // SAFETY: The loader owns the live image and returned a NUL-terminated path.
    let bytes = unsafe { CStr::from_ptr(path) }.to_bytes();
    Some(PathBuf::from(std::ffi::OsStr::from_bytes(bytes)))
}

pub(super) fn image_owner(address: usize) -> Option<u32> {
    let path = image_path(address)?;
    OWNERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .images
        .iter()
        .find(|image| image.path == path)
        .map(|image| image.id)
}

fn allowed_libr(ctx: &mut Ctx) -> Option<PathBuf> {
    let host = ctx.host();
    let configured = host
        .env_var("ROXIDE_LIBR_PATH")
        .map(|bytes| PathBuf::from(std::ffi::OsStr::from_bytes(&bytes)))
        .or_else(|| {
            host.env_var("R_HOME").map(|bytes| {
                Path::new(std::ffi::OsStr::from_bytes(&bytes))
                    .join("lib")
                    .join(LIBR)
            })
        })?;
    // Match the staged runtime image. R_HOME alone can refer to a GNU R installation.
    let canonical = host.canonicalize(&configured).ok()?;
    let runtime = image_path(crate::jump::Rf_protect as *const () as usize)?;
    (host.canonicalize(&runtime).ok()? == canonical).then_some(canonical)
}

fn load_error(
    ctx: &mut Ctx,
    call: SexpId,
    path: &Path,
    reason: impl std::fmt::Display,
) -> roxide_eval::Unwind {
    ctx.stop(
        call,
        format!(
            "unable to load shared object '{}':\n  {reason}",
            path.display()
        ),
    )
}

fn validate_image(ctx: &mut Ctx, path: &Path, call: SexpId) -> EvalResult<()> {
    // Read metadata from the same native file system that dlopen uses.
    let metadata = std::fs::metadata(path).map_err(|err| load_error(ctx, call, path, err))?;
    if !metadata.is_file() {
        return Err(load_error(
            ctx,
            call,
            path,
            "shared library must be a regular file",
        ));
    }
    let mut file = std::fs::File::open(path).map_err(|err| load_error(ctx, call, path, err))?;
    let len = file
        .metadata()
        .map_err(|err| load_error(ctx, call, path, err))?
        .len();
    let libr = allowed_libr(ctx);
    let host = ctx.host();
    binfmt::check(&mut file, len, libr.as_deref(), |path| {
        host.canonicalize(path).ok()
    })
    .map_err(|err| load_error(ctx, call, path, err))
}

struct LoaderError([u8; 512]);

impl LoaderError {
    fn capture(&mut self) {
        // SAFETY: dlerror returns a thread-local C string or NULL. Copy it
        // before the next loader call can replace it.
        unsafe {
            let mut message = dlerror().cast::<u8>();
            if !message.is_null() {
                for byte in &mut self.0 {
                    *byte = message.read();
                    if *byte == 0 {
                        break;
                    }
                    message = message.add(1);
                }
            }
        }
    }
}

impl std::fmt::Display for LoaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let end = self
            .0
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(self.0.len());
        if end == 0 {
            f.write_str("native loader returned no error message")
        } else {
            f.write_str(&String::from_utf8_lossy(&self.0[..end]))
        }
    }
}

struct OpenImage {
    path: *const c_char,
    flags: c_int,
    handle: *mut c_void,
    error: LoaderError,
}

struct CloseImage {
    handle: *mut c_void,
    status: c_int,
    error: LoaderError,
}

unsafe extern "C" fn open_image(data: *mut c_void) {
    // SAFETY: load supplies a live descriptor and validates the library.
    // A constructor can jump past this frame, so it owns no values with Drop.
    unsafe {
        let data = &mut *data.cast::<OpenImage>();
        data.handle = dlopen(data.path, data.flags);
        if data.handle.is_null() {
            data.error.capture();
        }
    }
}

unsafe extern "C" fn close_image(data: *mut c_void) {
    // SAFETY: close supplies a live descriptor and transfers one loader reference.
    // A destructor can jump past this frame, so it owns no values with Drop.
    unsafe {
        let data = &mut *data.cast::<CloseImage>();
        data.status = dlclose(data.handle);
        if data.status != 0 {
            data.error.capture();
        }
    }
}

pub(super) fn close(ctx: &mut Ctx, image: Image) -> EvalResult<()> {
    let Image { library, owner } = image;
    let mut data = CloseImage {
        handle: library.into_raw(),
        status: 0,
        error: LoaderError([0; 512]),
    };
    let closed = with_owner(ctx, owner.id, |ctx| {
        // SAFETY: close_image only accesses this descriptor during the trampoline call.
        unsafe {
            enter_trampoline_with(
                ctx,
                FrameKind::Entry { label: "dlclose" },
                close_image,
                std::ptr::from_mut(&mut data).cast(),
            )
        }
    });
    if closed.is_err() || data.status != 0 {
        owner.poison();
    }
    closed?;
    if data.status != 0 {
        return Err(ctx.stop(
            SEXP_NIL,
            format!(
                "unable to close shared object '{}':\n  {}",
                owner.path.display(),
                data.error
            ),
        ));
    }
    Ok(())
}

pub(crate) fn load(ctx: &mut Ctx, path: &Path, local: bool, now: bool, call: SexpId) -> EvalResult {
    let path = ctx
        .host()
        .canonicalize(path)
        .map_err(|err| load_error(ctx, call, path, err))?;
    let c_path = CString::new(path.as_os_str().as_bytes())
        .map_err(|err| load_error(ctx, call, &path, err))?;
    validate_image(ctx, &path, call)?;
    if ctx.ext::<Registry>().by_path(&path).is_some() {
        unload(ctx, &path, call)?;
    }
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| load_error(ctx, call, &path, "invalid shared-library name"))?
        .to_owned();
    let id = NEXT_ID
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .map_err(|_| ctx.stop(call, "shared-library ID limit exceeded"))?;
    let session = Arc::downgrade(&ctx.ext::<Registry>().session);
    let owner = {
        let mut owners = OWNERS.lock().unwrap_or_else(|e| e.into_inner());
        if owners.session.strong_count() != 0 && !owners.session.ptr_eq(&session) {
            drop(owners);
            return Err(ctx.stop(call, "another R session owns native library loading"));
        }
        if let Some(image) = owners.images.iter().find(|image| image.path == path) {
            let reason = if image.poisoned {
                "shared library is poisoned after an interrupted or failed loader call"
            } else {
                "shared library is already loading or closing"
            };
            drop(owners);
            return Err(load_error(ctx, call, &path, reason));
        }
        owners.session = session;
        owners.images.push(OwnedImage {
            path: path.clone(),
            id,
            poisoned: false,
        });
        ImageOwner { path, id }
    };
    let flags =
        (if local { RTLD_LOCAL } else { RTLD_GLOBAL }) | (if now { RTLD_NOW } else { RTLD_LAZY });
    let dll = make_dll(ctx, id, name, Some(&owner.path), local)?;
    let result = dll.info;
    let handle = dll.handle().addr();
    ctx.ext::<Registry>().dlls.push(dll);
    let mut data = OpenImage {
        path: c_path.as_ptr(),
        flags,
        handle: std::ptr::null_mut(),
        error: LoaderError([0; 512]),
    };
    let loaded = with_owner(ctx, id, |ctx| {
        // SAFETY: open_image only accesses this descriptor during the trampoline call.
        unsafe {
            enter_trampoline_with(
                ctx,
                FrameKind::Entry { label: "dlopen" },
                open_image,
                std::ptr::from_mut(&mut data).cast(),
            )
        }
    });
    if loaded.is_err() {
        owner.poison();
    }
    let initialized = loaded.and_then(|()| {
        if data.handle.is_null() {
            return Err(load_error(ctx, call, &owner.path, data.error));
        }
        let Some(dll) = ctx.ext::<Registry>().get(id) else {
            owner.poison();
            return Err(roxide_eval::Unwind::Internal(roxide_eval::InternalError {
                message: "missing DLL registry entry".into(),
            }));
        };
        // SAFETY: dlopen returned a non-null handle and transferred one reference.
        let library = unsafe { Library::from_raw(data.handle) };
        dll.image = Some(Image { library, owner });
        if let Some(init) = dll.entry("R_init") {
            with_owner(ctx, id, |ctx| {
                dotcall::void_entry(ctx, init, handle, "R_init")
            })?;
        }
        Ok(())
    });
    if let Err(err) = initialized {
        if let Some(dll) = ctx.ext::<Registry>().get(id) {
            dll.unloading = true;
        }
        ctx.scoped(|ctx| {
            ctx.protect_eval_result(&Err(err.clone()));
            ctx.run_pending_finalizers();
            let _ = remove(ctx, id);
            Err::<(), _>(err)
        })?;
    }
    Ok(result)
}

pub(crate) fn unload(ctx: &mut Ctx, path: &Path, call: SexpId) -> EvalResult<()> {
    let path = ctx
        .host()
        .canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf());
    // Dispatch can signal a warning before it calls the resolved native address.
    // A warning handler must not unload that address.
    let registry = ctx.ext::<Registry>();
    if crate::cx::depth() != 0
        || registry.active_owner != 0
        || registry.dlls.iter().any(|dll| dll.unloading)
    {
        return Err(ctx.stop(
            call,
            "cannot unload a shared library while native code or DLL unload is active",
        ));
    }
    let Some(dll) = ctx.ext::<Registry>().by_path(&path) else {
        return Err(ctx.stop(
            call,
            format!("shared object '{}' was not loaded", path.display()),
        ));
    };
    dll.unloading = true;
    let id = dll.id;
    let handle = dll.handle().addr();
    let callback = dll.entry("R_unload");
    if let Some(callback) = callback
        && let Err(err) = with_owner(ctx, id, |ctx| {
            dotcall::void_entry(ctx, callback, handle, "R_unload")
        })
    {
        if let Some(dll) = ctx.ext::<Registry>().get(id) {
            dll.unloading = false;
        }
        return Err(err);
    }
    run_dll_exit_finalizers(ctx, id);
    ctx.run_pending_finalizers();
    remove(ctx, id)
}
