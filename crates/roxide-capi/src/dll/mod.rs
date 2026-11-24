//! Own loaded libraries and the R objects that refer to them.

mod loader;
mod registry;

use std::ffi::{c_int, c_void};
use std::path::Path;

use roxide_eval::{Ctx, EvalResult, NativeCallKind};
use roxide_gc::SexpId;
use roxide_value::{
    CharId, Encoding, RLogical, RSession, SEXP_NIL, SYM_CLASS, SYM_NAMES, ValueApi,
};

pub(crate) use loader::{load, unload};
use registry::RegisteredSymbol;
pub(crate) use registry::{
    CallableEntry, Dll, Registry, code_owner, remember_finalizer, run_finalizer,
    track_exit_finalizer, with_owner,
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
