//! Own loaded libraries and the R objects that refer to them.

use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_int, c_void};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, Weak};

use libloading::os::unix::{Library, RTLD_GLOBAL, RTLD_LAZY, RTLD_LOCAL, RTLD_NOW};
use roxide_eval::{Ctx, CtxExt, EvalResult, NativeCallKind};
use roxide_gc::{FinalizerKind, SexpId, Trace, Tracer};
use roxide_value::{
    CharId, Encoding, RLogical, RSession, SEXP_NIL, SYM_CLASS, SYM_NAMES, ValueApi,
};

use crate::jump::{FrameKind, enter_trampoline_with};
use crate::{binfmt, dotcall};

static NEXT_ID: AtomicU32 = AtomicU32::new(1);
static OWNERS: Mutex<Owners> = Mutex::new(Owners {
    session: Weak::new(),
    images: Vec::new(),
});

struct Owners {
    session: Weak<()>,
    images: Vec<(PathBuf, u32)>,
}
const LIBR: &str = if cfg!(target_os = "macos") {
    "libR.dylib"
} else {
    "libR.so"
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Interface {
    C,
    Call,
    Fortran,
    External,
}

impl Interface {
    const ALL: [Self; 4] = [Self::C, Self::Call, Self::Fortran, Self::External];

    fn name(self) -> &'static str {
        match self {
            Self::C => ".C",
            Self::Call => ".Call",
            Self::Fortran => ".Fortran",
            Self::External => ".External",
        }
    }

    pub(crate) fn label(kind: NativeCallKind) -> &'static str {
        match kind {
            NativeCallKind::External2 => ".External2",
            _ => Self::from(kind).name(),
        }
    }

    fn class(self) -> &'static str {
        match self {
            Self::C => "CRoutine",
            Self::Call => "CallRoutine",
            Self::Fortran => "FortranRoutine",
            Self::External => "ExternalRoutine",
        }
    }
}

impl From<NativeCallKind> for Interface {
    fn from(kind: NativeCallKind) -> Self {
        match kind {
            NativeCallKind::C => Self::C,
            NativeCallKind::Call => Self::Call,
            NativeCallKind::Fortran => Self::Fortran,
            NativeCallKind::External | NativeCallKind::External2 => Self::External,
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub(crate) struct Routine {
    pub name: String,
    pub addr: usize,
    pub registration: Option<(Interface, c_int)>,
    pub owner: u32,
    pub code_owner: u32,
}

struct RegisteredSymbol {
    routine: Routine,
    with_registration: bool,
    pointer: SexpId,
}

pub(crate) struct CallableEntry {
    pub address: usize,
    pub owner: u32,
}

struct ExitFinalizer {
    owner: u32,
    reference: SexpId,
}

struct Image {
    library: Library,
    owner: ImageOwner,
}

struct ImageOwner {
    path: PathBuf,
    id: u32,
}

impl Drop for ImageOwner {
    fn drop(&mut self) {
        OWNERS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .images
            .retain(|(_, id)| *id != self.id);
    }
}

pub(crate) struct Dll {
    pub id: u32,
    pub name: String,
    pub dynamic: bool,
    pub force: bool,
    pub routines: Vec<Routine>,
    image: Option<Image>,
    info: SexpId,
    references: [SexpId; 2],
    symbols: Vec<RegisteredSymbol>,
    unloading: bool,
}

impl Dll {
    pub(crate) fn info(&self) -> SexpId {
        self.info
    }

    pub(crate) fn handle(&self) -> *mut c_void {
        std::ptr::without_provenance_mut(self.id as usize + 1)
    }

    fn entry(&self, prefix: &str) -> Option<usize> {
        let path = &self.image.as_ref()?.owner.path;
        let find = |name: &str| {
            let address = self.symbol(&format!("{prefix}_{name}"))?;
            (image_path(address)? == *path).then_some(address)
        };
        find(self.name.as_str()).or_else(|| {
            self.name
                .contains('.')
                .then(|| self.name.replace('.', "_"))
                .and_then(|name| find(&name))
        })
    }

    fn symbol(&self, name: &str) -> Option<usize> {
        if name.as_bytes().contains(&0) {
            return None;
        }
        // SAFETY: The address is only called after the caller selects its C ABI.
        unsafe {
            self.image
                .as_ref()?
                .library
                .get::<*mut c_void>(name.as_bytes())
        }
        .ok()
        .and_then(|symbol| (!symbol.is_null()).then(|| symbol.expose_provenance()))
    }
}

#[derive(Default)]
pub(crate) struct Registry {
    pub dlls: Vec<Dll>,
    pub callables: HashMap<(String, String), CallableEntry>,
    active_owner: u32,
    exit_finalizers: Vec<ExitFinalizer>,
    finalizer_owners: HashMap<(SexpId, usize), u32>,
    finalizer_prune_at: usize,
    session: Arc<()>,
}

impl Registry {
    pub(crate) fn get(&mut self, id: u32) -> Option<&mut Dll> {
        self.dlls.iter_mut().find(|dll| dll.id == id)
    }

    fn by_path(&mut self, path: &Path) -> Option<&mut Dll> {
        self.dlls.iter_mut().find(|dll| {
            dll.image
                .as_ref()
                .is_some_and(|image| image.owner.path == path)
        })
    }

    pub(crate) fn lookup(
        &self,
        name: &str,
        package: Option<&str>,
        kind: Option<Interface>,
        string_call: bool,
    ) -> Option<Routine> {
        let candidates = || {
            self.dlls.iter().rev().filter(|dll| {
                package.is_none_or(|package| package == dll.name) && (!string_call || !dll.force)
            })
        };
        for dll in candidates() {
            if let Some(routine) = dll.routines.iter().find(|routine| {
                let Some((registered, _)) = routine.registration else {
                    return false;
                };
                kind.is_none_or(|kind| kind == registered)
                    && (routine.name == name
                        || (registered == Interface::Fortran
                            && routine.name == name.to_ascii_lowercase()))
            }) {
                return Some(routine.clone());
            }
        }
        for dll in candidates() {
            if !dll.dynamic || dll.force || dll.routines.iter().any(|entry| entry.name == name) {
                continue;
            }
            let addr = match kind {
                Some(Interface::Fortran) => dll.symbol(&format!("{}_", name.to_ascii_lowercase())),
                None => dll
                    .symbol(name)
                    .or_else(|| dll.symbol(&format!("{}_", name.to_ascii_lowercase()))),
                _ => dll.symbol(name),
            };
            if let Some(addr) = addr
                && !dll.routines.iter().any(|entry| entry.addr == addr)
            {
                return Some(Routine {
                    name: name.into(),
                    addr,
                    registration: None,
                    owner: dll.id,
                    code_owner: image_owner(addr).unwrap_or(dll.id),
                });
            }
        }
        None
    }

    pub(crate) fn pointer_routine(&self, object: SexpId) -> Option<Routine> {
        self.dlls
            .iter()
            .flat_map(|dll| &dll.symbols)
            .find(|entry| entry.pointer == object)
            .map(|entry| entry.routine.clone())
    }
}

impl CtxExt for Registry {}
impl Trace for Registry {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        for dll in &self.dlls {
            tracer.edge(dll.info);
            tracer.edges(&dll.references);
            for entry in &dll.symbols {
                tracer.edge(entry.pointer);
            }
        }
        for entry in &self.exit_finalizers {
            tracer.edge(entry.reference);
        }
    }
}

#[repr(C)]
struct DlAddr {
    path: *const c_char,
    base: *mut c_void,
    symbol: *const c_char,
    address: *mut c_void,
}

unsafe extern "C" {
    fn dladdr(address: *const c_void, info: *mut DlAddr) -> c_int;
}

fn image_path(address: usize) -> Option<PathBuf> {
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

fn image_owner(address: usize) -> Option<u32> {
    let path = image_path(address)?;
    OWNERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .images
        .iter()
        .find(|(owner, _)| *owner == path)
        .map(|(_, id)| *id)
}

pub(crate) fn code_owner(ctx: &mut Ctx, address: usize, owner: u32) -> u32 {
    image_owner(address).unwrap_or_else(|| {
        let active = ctx.ext::<Registry>().active_owner;
        if active == 0 { owner } else { active }
    })
}

pub(crate) fn with_owner<T>(
    ctx: &mut Ctx,
    owner: u32,
    run: impl FnOnce(&mut Ctx) -> EvalResult<T>,
) -> EvalResult<T> {
    if owner == 0 {
        return run(ctx);
    }
    let previous = std::mem::replace(&mut ctx.ext::<Registry>().active_owner, owner);
    let result = run(ctx);
    ctx.ext::<Registry>().active_owner = previous;
    result
}

pub(crate) fn track_exit_finalizer(ctx: &mut Ctx, reference: SexpId, kind: FinalizerKind) {
    if let FinalizerKind::Native { dll, .. } = kind
        && dll != 0
    {
        ctx.ext::<Registry>().exit_finalizers.push(ExitFinalizer {
            owner: dll,
            reference,
        });
    }
}

pub(crate) fn remember_finalizer(
    ctx: &mut Ctx,
    target: SexpId,
    kind: FinalizerKind,
) -> EvalResult<()> {
    let FinalizerKind::Native { fun, dll } = kind else {
        return Ok(());
    };
    if target == SEXP_NIL {
        return Ok(());
    }
    let registry = ctx.ext::<Registry>();
    if registry.finalizer_owners.len() >= registry.finalizer_prune_at {
        let mut owners = std::mem::take(&mut registry.finalizer_owners);
        owners.retain(|(target, _), _| ctx.session().heap.get(*target).is_ok());
        let mut exit = std::mem::take(&mut ctx.ext::<Registry>().exit_finalizers);
        exit.retain(|entry| {
            ctx.session()
                .weakref_key(entry.reference)
                .is_ok_and(|key| key != SEXP_NIL)
        });
        let registry = ctx.ext::<Registry>();
        registry.exit_finalizers = exit;
        registry.finalizer_prune_at = owners.len().saturating_mul(2).max(256);
        registry.finalizer_owners = owners;
    }
    let owners = &mut ctx.ext::<Registry>().finalizer_owners;
    // The evaluator passes only the target and address, even for a batch it has
    // already taken out of the heap. Keep the owner until the DLL is unloaded.
    if owners.get(&(target, fun)).is_some_and(|&old| old != dll) {
        return Err(ctx.stop(
            SEXP_NIL,
            "cannot reuse a native finalizer address for a target from an unloaded library",
        ));
    }
    owners.insert((target, fun), dll);
    Ok(())
}

pub(crate) fn run_finalizer(ctx: &mut Ctx, address: usize, target: SexpId) -> EvalResult<()> {
    let registry = ctx.ext::<Registry>();
    let Some(&owner) = registry.finalizer_owners.get(&(target, address)) else {
        return Ok(());
    };
    if (owner != 0 && !registry.dlls.iter().any(|dll| dll.id == owner))
        || image_path(address).is_none()
    {
        return Ok(());
    }
    ctx.scoped(|ctx| {
        ctx.session_mut()
            .heap
            .try_protect(target)
            .map_err(|err| ctx.stop(SEXP_NIL, err.to_string()))?;
        with_owner(ctx, owner, |ctx| {
            dotcall::void_entry(
                ctx,
                address,
                crate::abi::Sexp::from_id(target).0,
                "finalizer",
            )
        })
    })
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

struct OpenImage<'a> {
    path: &'a Path,
    flags: c_int,
    result: Option<Result<Library, libloading::Error>>,
}

unsafe extern "C" fn open_image(data: *mut c_void) {
    // SAFETY: load keeps the descriptor live while the trampoline runs the loader.
    let data = unsafe { &mut *data.cast::<OpenImage<'_>>() };
    // SAFETY: The caller requested native code execution and validated the dependencies.
    // Loader callbacks must return normally while the OS loader holds its internal locks.
    data.result = Some(unsafe { Library::open(Some(data.path), data.flags) });
}

unsafe extern "C" fn close_image(data: *mut c_void) {
    // SAFETY: close keeps the descriptor live while the trampoline runs the loader.
    let library = unsafe { &mut *data.cast::<Option<Library>>() };
    drop(library.take());
}

fn close(ctx: &mut Ctx, image: Image) -> EvalResult<()> {
    let Image { library, owner } = image;
    let mut library = Some(library);
    with_owner(ctx, owner.id, |ctx| {
        // SAFETY: close_image only accesses this descriptor during the trampoline call.
        unsafe {
            enter_trampoline_with(
                ctx,
                FrameKind::Entry { label: "dlclose" },
                close_image,
                std::ptr::from_mut(&mut library).cast(),
            )
        }
    })
}

pub(crate) fn load(ctx: &mut Ctx, path: &Path, local: bool, now: bool, call: SexpId) -> EvalResult {
    let path = ctx
        .host()
        .canonicalize(path)
        .map_err(|err| load_error(ctx, call, path, err))?;
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
        if owners.images.iter().any(|(image, _)| *image == path) {
            drop(owners);
            return Err(load_error(
                ctx,
                call,
                &path,
                "shared library is already loading or closing",
            ));
        }
        owners.session = session;
        owners.images.push((path.clone(), id));
        ImageOwner { path, id }
    };
    let flags =
        (if local { RTLD_LOCAL } else { RTLD_GLOBAL }) | (if now { RTLD_NOW } else { RTLD_LAZY });
    let dll = make_dll(ctx, id, name, Some(&owner.path), local)?;
    let result = dll.info;
    let handle = dll.handle().addr();
    ctx.ext::<Registry>().dlls.push(dll);
    let mut data = OpenImage {
        path: &owner.path,
        flags,
        result: None,
    };
    let loaded = with_owner(ctx, id, |ctx| {
        // SAFETY: open_image only accesses this descriptor during the trampoline call.
        unsafe {
            enter_trampoline_with(
                ctx,
                FrameKind::Entry { label: "dlopen" },
                open_image,
                std::ptr::from_mut(&mut data).cast(),
            )?;
        }
        data.result
            .take()
            .ok_or_else(|| load_error(ctx, call, data.path, "loader returned no result"))?
            .map_err(|err| load_error(ctx, call, data.path, err))
    });
    let initialized = loaded.and_then(|library| {
        let dll = ctx.ext::<Registry>().get(id).ok_or_else(|| {
            roxide_eval::Unwind::Internal(roxide_eval::InternalError {
                message: "missing DLL registry entry".into(),
            })
        })?;
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

fn run_dll_exit_finalizers(ctx: &mut Ctx, owner: u32) {
    let references: Vec<_> = ctx
        .ext::<Registry>()
        .exit_finalizers
        .extract_if(.., |entry| entry.owner == owner)
        .map(|entry| entry.reference)
        .collect();
    let visible = ctx.is_visible();
    let _ = ctx.scoped(|ctx| {
        let mut callbacks = Vec::new();
        for reference in references {
            let Some(entry) = ctx.session_mut().heap.remove_weakref(reference) else {
                continue;
            };
            let _ = ctx.session_mut().clear_weakref(reference);
            if let Some(FinalizerKind::Native { fun, .. }) = entry.finalizer {
                ctx.protect(entry.key);
                callbacks.push((fun, entry.key));
            }
        }
        for (address, target) in callbacks.into_iter().rev() {
            let _ = run_finalizer(ctx, address, target);
        }
        Ok(())
    });
    ctx.set_visible(visible);
}

fn purge_callbacks(ctx: &mut Ctx, id: u32) {
    ctx.session_mut()
        .heap
        .retain_finalizers(|kind| !matches!(kind, FinalizerKind::Native { dll, .. } if dll == id));
    let registry = ctx.ext::<Registry>();
    registry.callables.retain(|_, entry| entry.owner != id);
    registry.finalizer_owners.retain(|_, owner| *owner != id);
    registry.exit_finalizers.retain(|entry| entry.owner != id);
}

fn remove(ctx: &mut Ctx, id: u32) -> EvalResult<()> {
    purge_callbacks(ctx, id);
    let registry = ctx.ext::<Registry>();
    let mut pointers = Vec::new();
    for dll in &mut registry.dlls {
        if dll.id == id {
            pointers.extend(dll.references);
        }
        dll.routines.retain(|routine| routine.code_owner != id);
        dll.symbols.retain(|entry| {
            if dll.id == id || entry.routine.code_owner == id {
                pointers.push(entry.pointer);
                false
            } else {
                true
            }
        });
    }
    let mut status = Ok(());
    for pointer in pointers {
        if let Err(error) = ctx.session_mut().set_extptr_addr(pointer, 0) {
            status = Err(error.into());
        }
    }
    let registry = ctx.ext::<Registry>();
    if let Some(index) = registry.dlls.iter().position(|dll| dll.id == id)
        && let Some(image) = registry.dlls.remove(index).image
    {
        let closed = close(ctx, image);
        // A destructor can register callbacks while dlclose is active.
        purge_callbacks(ctx, id);
        closed?;
    }
    status
}

fn make_dll(
    ctx: &mut Ctx,
    id: u32,
    name: String,
    path: Option<&Path>,
    local: bool,
) -> EvalResult<Dll> {
    ctx.scoped(|ctx| {
        let mut dll = Dll {
            id,
            name,
            image: None,
            dynamic: id != 0,
            force: false,
            routines: Vec::new(),
            symbols: Vec::new(),
            info: SEXP_NIL,
            references: [SEXP_NIL; 2],
            unloading: false,
        };
        let session = ctx.session_mut();
        let info = named_list(
            session,
            &["name", "path", "dynamicLookup", "handle", "info"],
        )?;
        ctx.protect(info);
        let session = ctx.session_mut();
        let name = session.scalar_str(&dll.name)?;
        session.list_set(info, 0, name)?;
        let path = path.map(Path::to_string_lossy).unwrap_or_default();
        let path = session.scalar_str(&path)?;
        session.list_set(info, 1, path)?;
        let dynamic = session.scalar_lgl(RLogical::from_raw(i32::from(dll.dynamic)))?;
        session.list_set(info, 2, dynamic)?;
        let flag = session.scalar_lgl(RLogical::from_raw(i32::from(local)))?;
        let handle = session.alloc_extptr(dll.handle().addr(), SEXP_NIL, flag)?;
        session.list_set(info, 3, handle)?;
        set_class(session, handle, &["DLLHandle"])?;
        let ptr = session.alloc_extptr(dll.handle().addr(), SEXP_NIL, SEXP_NIL)?;
        session.list_set(info, 4, ptr)?;
        set_class(session, ptr, &["DLLInfoReference"])?;
        set_class(session, info, &["DLLInfo"])?;
        dll.info = info;
        dll.references = [handle, ptr];
        Ok(dll)
    })
}

pub(crate) fn embedding(ctx: &mut Ctx) -> EvalResult<*mut c_void> {
    if let Some(dll) = ctx.ext::<Registry>().get(0) {
        return Ok(dll.handle());
    }
    let dll = make_dll(ctx, 0, "(embedding)".into(), None, true)?;
    let handle = dll.handle();
    ctx.ext::<Registry>().dlls.push(dll);
    Ok(handle)
}

pub(crate) fn refresh_dynamic(ctx: &mut Ctx, info: SexpId, enabled: bool) -> EvalResult<()> {
    let session = ctx.session_mut();
    let value = if enabled {
        roxide_value::TRUE_SCALAR
    } else {
        roxide_value::FALSE_SCALAR
    };
    Ok(session.list_set(info, 2, value)?)
}

pub(crate) fn string_vector(session: &mut RSession, names: &[&str]) -> EvalResult {
    let values = names
        .iter()
        .map(|name| session.mk_char(name.as_bytes(), Encoding::Utf8))
        .collect::<Result<Vec<CharId>, _>>()?;
    Ok(session.alloc_str_from(&values)?)
}

pub(crate) fn named_list(session: &mut RSession, names: &[&str]) -> EvalResult {
    let list = session.alloc_list(names.len())?;
    let names = string_vector(session, names)?;
    session.set_attrib(list, SYM_NAMES, names)?;
    Ok(list)
}

fn set_class(session: &mut RSession, value: SexpId, names: &[&str]) -> EvalResult<()> {
    let class = string_vector(session, names)?;
    Ok(session.set_attrib(value, SYM_CLASS, class)?)
}

fn symbol_info(ctx: &mut Ctx, routine: &Routine, registered_pointer: bool) -> EvalResult {
    let Some(dll) = ctx.ext::<Registry>().get(routine.owner) else {
        return Err(ctx.stop(SEXP_NIL, "shared library was unloaded"));
    };
    let info = dll.info;
    let cached = dll
        .symbols
        .iter()
        .find(|entry| entry.routine == *routine && entry.with_registration == registered_pointer)
        .map(|entry| entry.pointer);
    let pointer = if let Some(pointer) = cached {
        pointer
    } else {
        let class = if registered_pointer && routine.registration.is_some() {
            "RegisteredNativeSymbol"
        } else {
            "NativeSymbol"
        };
        let session = ctx.session_mut();
        let pointer = session.alloc_extptr(routine.addr, SEXP_NIL, info)?;
        set_class(session, pointer, &[class])?;
        if let Some(dll) = ctx.ext::<Registry>().get(routine.owner) {
            dll.symbols.push(RegisteredSymbol {
                routine: routine.clone(),
                with_registration: registered_pointer,
                pointer,
            });
        }
        pointer
    };
    let session = ctx.session_mut();
    let mut fields = vec!["name", "address", "dll"];
    if routine.registration.is_some() {
        fields.push("numParameters");
    }
    let result = named_list(session, &fields)?;
    let name = session.scalar_str(&routine.name)?;
    session.list_set(result, 0, name)?;
    session.list_set(result, 1, pointer)?;
    session.list_set(result, 2, info)?;
    if let Some((kind, arity)) = routine.registration {
        let arity = session.scalar_int(arity)?;
        session.list_set(result, 3, arity)?;
        set_class(session, result, &[kind.class(), "NativeSymbolInfo"])?;
    } else {
        set_class(session, result, &["NativeSymbolInfo"])?;
    }
    Ok(result)
}

pub(crate) fn native_symbol_info(
    ctx: &mut Ctx,
    name: &str,
    package: Option<&str>,
    with_registration: bool,
    call: SexpId,
) -> EvalResult {
    let routine = ctx
        .ext::<Registry>()
        .lookup(name, package, None, false)
        .ok_or_else(|| ctx.stop(call, format!("no such symbol {name}")))?;
    symbol_info(ctx, &routine, with_registration)
}

pub(crate) fn loaded_dlls(ctx: &mut Ctx) -> EvalResult {
    let dlls: Vec<_> = ctx
        .ext::<Registry>()
        .dlls
        .iter()
        .map(|dll| (dll.name.clone(), dll.info))
        .collect();
    let names: Vec<_> = dlls.iter().map(|(name, _)| name.as_str()).collect();
    let session = ctx.session_mut();
    let result = named_list(session, &names)?;
    for (i, (_, info)) in dlls.into_iter().enumerate() {
        session.list_set(result, i, info)?;
    }
    set_class(session, result, &["DLLInfoList"])?;
    Ok(result)
}

pub(crate) fn registered_routines(
    ctx: &mut Ctx,
    info: SexpId,
    add_names: bool,
    call: SexpId,
) -> EvalResult {
    let Some(dll) = ctx
        .ext::<Registry>()
        .dlls
        .iter()
        .find(|dll| dll.info == info || dll.references.contains(&info))
    else {
        return Err(ctx.stop(call, "invalid DLLInfo object"));
    };
    let routines = dll.routines.clone();
    let session = ctx.session_mut();
    let result = named_list(session, &Interface::ALL.map(Interface::name))?;
    for (i, kind) in Interface::ALL.into_iter().enumerate() {
        let selected: Vec<_> = routines
            .iter()
            .filter(|routine| {
                routine
                    .registration
                    .is_some_and(|(registered, _)| kind == registered)
            })
            .collect();
        let list = ctx.session_mut().alloc_list(selected.len())?;
        ctx.session_mut().list_set(result, i, list)?;
        for (j, routine) in selected.iter().enumerate() {
            let info = symbol_info(ctx, routine, true)?;
            ctx.session_mut().list_set(list, j, info)?;
        }
        if add_names {
            let names: Vec<_> = selected
                .iter()
                .map(|routine| routine.name.as_str())
                .collect();
            let names = string_vector(ctx.session_mut(), &names)?;
            ctx.session_mut().set_attrib(list, SYM_NAMES, names)?;
        }
        set_class(ctx.session_mut(), list, &["NativeRoutineList"])?;
    }
    set_class(ctx.session_mut(), result, &["DLLRegisteredRoutines"])?;
    Ok(result)
}
