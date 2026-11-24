//! C entry points for native routine registration and lookup.

use std::ffi::{CStr, c_char, c_int, c_uint, c_void};
use std::ptr::null_mut;

use roxide_eval::{Ctx, EvalResult};
use roxide_value::SEXP_NIL;

use crate::abi::Rboolean;
use crate::api::{extptr::DlFunc, vector::api};
use crate::dll::{self, CallableEntry, Dll, Interface, Registry, Routine};

/// Registration entry for `R_CMethodDef` or `R_FortranMethodDef`.
#[repr(C)]
pub struct PointerMethod {
    name: *const c_char,
    fun: DlFunc,
    num_args: c_int,
    types: *const c_uint,
}

/// Registration entry for `R_CallMethodDef` or `R_ExternalMethodDef`.
#[repr(C)]
pub struct SexpMethod {
    name: *const c_char,
    fun: DlFunc,
    num_args: c_int,
}

const MAX_ROUTINES: usize = 65_536;

fn dll(ctx: &mut Ctx, handle: *mut c_void) -> EvalResult<&mut Dll> {
    let index = ctx
        .ext::<Registry>()
        .dlls
        .iter()
        .position(|dll| dll.handle() == handle)
        .ok_or_else(|| ctx.stop(SEXP_NIL, "invalid DllInfo pointer"))?;
    Ok(&mut ctx.ext::<Registry>().dlls[index])
}

unsafe fn name(ctx: &mut Ctx, ptr: *const c_char) -> EvalResult<String> {
    if ptr.is_null() {
        return Err(ctx.stop(SEXP_NIL, "invalid NULL native routine or package name"));
    }
    // SAFETY: The API caller supplies a NUL-terminated string.
    let name = unsafe { CStr::from_ptr(ptr) };
    Ok(name.to_string_lossy().into_owned())
}

unsafe fn collect<T>(
    ctx: &mut Ctx,
    table: *const T,
    kind: Interface,
    owner: u32,
    fields: impl Fn(&T) -> (*const c_char, DlFunc, c_int),
    routines: &mut Vec<Routine>,
) -> EvalResult<()> {
    if table.is_null() {
        return Ok(());
    }
    for i in 0..MAX_ROUTINES {
        // SAFETY: The caller supplies an array with a readable sentinel entry.
        let (label, fun, arity) = fields(unsafe { &*table.add(i) });
        if label.is_null() {
            return Ok(());
        }
        let label = unsafe { name(ctx, label) }?;
        let Some(fun) = fun else {
            return Err(ctx.stop(
                SEXP_NIL,
                format!("NULL address for native routine '{label}'"),
            ));
        };
        if !(-1..=crate::dotcall::MAX_ARGS as c_int).contains(&arity) {
            return Err(ctx.stop(
                SEXP_NIL,
                format!("invalid argument count for native routine '{label}'"),
            ));
        }
        routines.push(Routine {
            name: label,
            addr: fun as usize,
            registration: Some((kind, arity)),
            owner,
            code_owner: dll::code_owner(ctx, fun as usize, owner),
        });
    }
    Err(ctx.stop(
        SEXP_NIL,
        "native routine registration table has no terminator",
    ))
}

fn function(address: usize) -> DlFunc {
    if address == 0 {
        return None;
    }
    // SAFETY: Registration or symbol lookup supplied this non-null C function address.
    Some(unsafe { std::mem::transmute::<usize, unsafe extern "C" fn() -> *mut c_void>(address) })
}

api! {
    /// Copy the four sentinel-terminated native routine tables into the DLL registry.
    fn R_registerRoutines(info: *mut c_void, c: *const PointerMethod, call: *const SexpMethod, fortran: *const PointerMethod, external: *const SexpMethod) -> c_int = 0; |ctx| {
        let owner = dll(ctx, info)?.id;
        let mut routines = Vec::new();
        let ptr = |method: &PointerMethod| (method.name, method.fun, method.num_args);
        let sexp = |method: &SexpMethod| (method.name, method.fun, method.num_args);
        collect(ctx, c, Interface::C, owner, ptr, &mut routines)?;
        collect(ctx, call, Interface::Call, owner, sexp, &mut routines)?;
        collect(ctx, fortran, Interface::Fortran, owner, ptr, &mut routines)?;
        collect(ctx, external, Interface::External, owner, sexp, &mut routines)?;
        dll(ctx, info)?.routines = routines;
        Ok(1)
    }

    /// Set dynamic symbol lookup and return its previous value.
    fn R_useDynamicSymbols(info: *mut c_void, enabled: Rboolean) -> Rboolean = 0; |ctx| {
        let entry = dll(ctx, info)?;
        let previous = entry.dynamic;
        let object = entry.info();
        dll::refresh_dynamic(ctx, object, enabled != 0)?;
        dll(ctx, info)?.dynamic = enabled != 0;
        Ok(i32::from(previous))
    }

    /// Set whether R calls must use a symbol object and return the previous value.
    fn R_forceSymbols(info: *mut c_void, enabled: Rboolean) -> Rboolean = 0; |ctx| {
        let entry = dll(ctx, info)?;
        Ok(i32::from(std::mem::replace(&mut entry.force, enabled != 0)))
    }

    /// Find a DLL by its registered name.
    fn R_getDllInfo(package: *const c_char) -> *mut c_void = null_mut(); |ctx| {
        let package = name(ctx, package)?;
        Ok(ctx.ext::<Registry>().dlls.iter().rev().find(|dll| dll.name == package)
            .map_or_else(null_mut, Dll::handle))
    }

    /// Return the registry entry used by the embedding application.
    fn R_getEmbeddingDllInfo() -> *mut c_void = null_mut(); |ctx| {
        dll::embedding(ctx)
    }

    #[doc = "Find the address of a native routine. A NULL or empty package searches all DLLs.

The public headers declare `R_RegisteredNativeSymbol` as an opaque, incomplete type. This function leaves the optional `symbol` pointer untouched."]
    fn R_FindSymbol(routine: *const c_char, package: *const c_char, _symbol: *mut c_void) -> DlFunc = None; |ctx| {
        let routine = name(ctx, routine)?;
        let package = if package.is_null() { None } else { Some(name(ctx, package)?) };
        Ok(ctx.ext::<Registry>().lookup(&routine, package.as_deref().filter(|pkg| !pkg.is_empty()), None, true)
            .and_then(|routine| function(routine.addr)))
    }

    /// Register a callable function under a package and name.
    fn R_RegisterCCallable(package: *const c_char, routine: *const c_char, fun: DlFunc) -> () = (); |ctx| {
        let key = (name(ctx, package)?, name(ctx, routine)?);
        let Some(fun) = fun else {
            return Err(ctx.stop(SEXP_NIL, "cannot register a NULL callable function"));
        };
        let address = fun as usize;
        let owner = dll::code_owner(ctx, address, 0);
        ctx.ext::<Registry>().callables.insert(key, CallableEntry { address, owner });
        Ok(())
    }

    /// Find a callable function, loading the package namespace on demand.
    fn R_GetCCallable(package: *const c_char, routine: *const c_char) -> DlFunc = None; |ctx| {
        let key = (name(ctx, package)?, name(ctx, routine)?);
        if let Some(entry) = ctx.ext::<Registry>().callables.get(&key) {
            return Ok(function(entry.address));
        }
        let loaded = ctx.with_native_toplevel(|ctx| ctx.with_native_error_handler(
            |ctx| ctx.load_namespace(&key.0, SEXP_NIL),
            |_, _| Ok(SEXP_NIL),
        ));
        loaded?;
        if let Some(entry) = ctx.ext::<Registry>().callables.get(&key) {
            return Ok(function(entry.address));
        }
        Err(ctx.stop(SEXP_NIL, format!("function '{}' not provided by package '{}'", key.1, key.0)))
    }
}
