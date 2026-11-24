//! Own loaded libraries and the R objects that refer to them.

use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_int, c_void};
use std::io::Read;
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
const MAX_IMAGE_SIZE: u64 = 256 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Interface {
    C,
    Call,
    Fortran,
    External,
}

impl Interface {
    const ALL: [(Self, &'static str); 4] = [
        (Self::C, ".C"),
        (Self::Call, ".Call"),
        (Self::Fortran, ".Fortran"),
        (Self::External, ".External"),
    ];

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
    pub dll: u32,
    pub code_dll: u32,
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
    symbols: Vec<(Routine, bool, SexpId)>,
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
        let address = self.symbol(&format!("{prefix}_{}", self.name))?;
        (image_path(address)? == self.image.as_ref()?.owner.path).then_some(address)
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
                .get::<unsafe extern "C" fn()>(name.as_bytes())
        }
        .ok()
        .map(|symbol| *symbol as usize)
    }
}

#[derive(Default)]
pub(crate) struct Registry {
    pub dlls: Vec<Dll>,
    pub callables: HashMap<(String, String), (usize, u32)>,
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
                    dll: dll.id,
                    code_dll: dll_id_for_addr(addr),
                });
            }
        }
        None
    }

    pub(crate) fn pointer_routine(&self, object: SexpId) -> Option<Routine> {
        self.dlls
            .iter()
            .flat_map(|dll| &dll.symbols)
            .find(|(_, _, pointer)| *pointer == object)
            .map(|(routine, _, _)| routine.clone())
    }
}

impl CtxExt for Registry {}
impl Trace for Registry {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        for dll in &self.dlls {
            tracer.edge(dll.info);
            tracer.edges(&dll.references);
            for (_, _, pointer) in &dll.symbols {
                tracer.edge(*pointer);
            }
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
    let mut info = std::mem::MaybeUninit::<DlAddr>::uninit();
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
    // SAFETY: The loader returned a live image and a NUL-terminated file name.
    let path = unsafe { info.assume_init().path };
    if path.is_null() {
        return None;
    }
    let bytes = unsafe { CStr::from_ptr(path) }.to_bytes();
    Some(PathBuf::from(std::ffi::OsStr::from_bytes(bytes)))
}

pub(crate) fn dll_id_for_addr(address: usize) -> u32 {
    image_path(address).map_or(0, |path| {
        OWNERS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .images
            .iter()
            .find(|(owner, _)| *owner == path)
            .map_or(0, |(_, id)| *id)
    })
}

pub(crate) fn remember_finalizer(
    ctx: &mut Ctx,
    target: SexpId,
    kind: FinalizerKind,
) -> EvalResult<()> {
    let FinalizerKind::Native { fun, dll } = kind else {
        return Ok(());
    };
    if target == SEXP_NIL || dll == 0 {
        return Ok(());
    }
    let registry = ctx.ext::<Registry>();
    if registry.finalizer_owners.len() >= registry.finalizer_prune_at {
        let mut owners = std::mem::take(&mut registry.finalizer_owners);
        owners.retain(|(target, _), _| ctx.session().heap.get(*target).is_ok());
        let registry = ctx.ext::<Registry>();
        registry.finalizer_prune_at = owners.len().saturating_mul(2).max(256);
        registry.finalizer_owners = owners;
    }
    let owners = &mut ctx.ext::<Registry>().finalizer_owners;
    // The evaluator passes only the target and address, even for a batch it has
    // already taken out of the heap. Preserve the original owner across reloads.
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
    if registry
        .finalizer_owners
        .get(&(target, address))
        .is_some_and(|&id| !registry.dlls.iter().any(|dll| dll.id == id))
        || image_path(address).is_none()
    {
        return Ok(());
    }
    ctx.scoped(|ctx| {
        ctx.session_mut()
            .heap
            .try_protect(target)
            .map_err(|err| ctx.stop(SEXP_NIL, err.to_string()))?;
        dotcall::void_entry(
            ctx,
            address,
            crate::abi::Sexp::from_id(target).0,
            "finalizer",
        )
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

fn validate_image(ctx: &mut Ctx, path: &Path, call: SexpId) -> EvalResult<()> {
    let failed = |ctx: &mut Ctx, reason: &dyn std::fmt::Display| {
        ctx.stop(
            call,
            format!(
                "unable to load shared object '{}': {reason}",
                path.display()
            ),
        )
    };
    let metadata = std::fs::metadata(path).map_err(|err| failed(ctx, &err))?;
    if !metadata.is_file() {
        return Err(failed(ctx, &"shared library must be a regular file"));
    }
    if metadata.len() > MAX_IMAGE_SIZE {
        return Err(failed(ctx, &"shared-library file exceeds the size limit"));
    }
    let file = std::fs::File::open(path).map_err(|err| failed(ctx, &err))?;
    let mut bytes = Vec::new();
    file.take(MAX_IMAGE_SIZE + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| failed(ctx, &err))?;
    if bytes.len() as u64 > MAX_IMAGE_SIZE {
        return Err(failed(ctx, &"shared-library file exceeds the size limit"));
    }
    let libr = allowed_libr(ctx);
    binfmt::check(&bytes, libr.as_deref().map(|p| p.as_os_str().as_bytes()))
        .map_err(|err| failed(ctx, &err))
}

pub(crate) fn load(ctx: &mut Ctx, path: &Path, local: bool, now: bool, call: SexpId) -> EvalResult {
    let path = ctx.host().canonicalize(path).map_err(|err| {
        ctx.stop(
            call,
            format!("unable to load shared object '{}': {err}", path.display()),
        )
    })?;
    if let Some(dll) = ctx.ext::<Registry>().by_path(&path) {
        return Ok(dll.info);
    }
    validate_image(ctx, &path, call)?;
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ctx.stop(call, "invalid shared-library name"))?
        .replace('.', "_");
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
        owners.session = session;
        owners.images.push((path.clone(), id));
        ImageOwner { path, id }
    };
    let flags =
        (if local { RTLD_LOCAL } else { RTLD_GLOBAL }) | (if now { RTLD_NOW } else { RTLD_LAZY });
    // SAFETY: The caller requests native code execution. The dependency check precedes dlopen.
    let library = unsafe { Library::open(Some(&owner.path), flags) }.map_err(|err| {
        ctx.stop(
            call,
            format!(
                "unable to load shared object '{}': {err}",
                owner.path.display()
            ),
        )
    })?;
    let image = Image { library, owner };
    let dll = make_dll(ctx, id, name, Some(image), local)?;
    let result = dll.info;
    let init = dll.entry("R_init");
    let handle = dll.handle().addr();
    ctx.ext::<Registry>().dlls.push(dll);
    if let Some(init) = init
        && let Err(err) = dotcall::void_entry(ctx, init, handle, "R_init")
    {
        if let Some(dll) = ctx.ext::<Registry>().get(id) {
            dll.unloading = true;
        }
        ctx.scoped(|ctx| {
            ctx.protect_eval_result(&Err(err.clone()));
            ctx.run_pending_finalizers();
            remove(ctx, id)
        })?;
        return Err(err);
    }
    Ok(result)
}

pub(crate) fn unload(ctx: &mut Ctx, path: &Path, call: SexpId) -> EvalResult<()> {
    let path = ctx
        .host()
        .canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf());
    // A C callable can be active in a different DLL from the current trampoline.
    if crate::cx::depth() != 0 || ctx.ext::<Registry>().dlls.iter().any(|dll| dll.unloading) {
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
        && let Err(err) = dotcall::void_entry(ctx, callback, handle, "R_unload")
    {
        if let Some(dll) = ctx.ext::<Registry>().get(id) {
            dll.unloading = false;
        }
        return Err(err);
    }
    ctx.run_pending_finalizers();
    remove(ctx, id)
}

fn remove(ctx: &mut Ctx, id: u32) -> EvalResult<()> {
    ctx.session_mut()
        .heap
        .retain_finalizers(|kind| !matches!(kind, FinalizerKind::Native { dll, .. } if dll == id));
    let registry = ctx.ext::<Registry>();
    let mut pointers = Vec::new();
    for dll in &mut registry.dlls {
        if dll.id == id {
            pointers.extend(dll.references);
        }
        dll.routines.retain(|routine| routine.code_dll != id);
        dll.symbols.retain(|(routine, _, pointer)| {
            if dll.id == id || routine.code_dll == id {
                pointers.push(*pointer);
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
    registry.callables.retain(|_, (_, owner)| *owner != id);
    let index = registry.dlls.iter().position(|dll| dll.id == id);
    let removed = index.map(|index| registry.dlls.remove(index));
    drop(removed);
    status
}

fn make_dll(
    ctx: &mut Ctx,
    id: u32,
    name: String,
    image: Option<Image>,
    local: bool,
) -> EvalResult<Dll> {
    ctx.scoped(|ctx| {
        let mut dll = Dll {
            id,
            name,
            image,
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
        let path = dll
            .image
            .as_ref()
            .map(|image| image.owner.path.to_string_lossy())
            .unwrap_or_default();
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
    let Some(dll) = ctx.ext::<Registry>().get(routine.dll) else {
        return Err(ctx.stop(SEXP_NIL, "shared library was unloaded"));
    };
    let info = dll.info;
    let cached = dll
        .symbols
        .iter()
        .find(|(entry, registered, _)| entry == routine && *registered == registered_pointer)
        .map(|(_, _, pointer)| *pointer);
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
        if let Some(dll) = ctx.ext::<Registry>().get(routine.dll) {
            dll.symbols
                .push((routine.clone(), registered_pointer, pointer));
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
    let result = named_list(session, &Interface::ALL.map(|(_, name)| name))?;
    for (i, (kind, _)) in Interface::ALL.into_iter().enumerate() {
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
