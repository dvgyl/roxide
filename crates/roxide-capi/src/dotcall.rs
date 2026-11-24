//! Dispatch native calls and copy arguments across the C boundary.

use std::alloc::{Layout, alloc, dealloc};
use std::ffi::{c_int, c_void};
use std::ptr::NonNull;

use roxide_eval::{Args, Call, Ctx, DotCOpts, EvalResult, NativeCallKind};
use roxide_gc::SexpId;
use roxide_value::{
    CharId, Encoding, NA_INTEGER, RComplex, RLogical, SEXP_NIL, SYM_NAMES, SexpType, ValueApi,
};

use crate::abi::Sexp;
use crate::dll::{self, Interface, Registry, Routine};
use crate::jump::{FrameKind, enter_trampoline_with};

pub(crate) const MAX_ARGS: usize = 65;

#[repr(C)]
struct SexpCall {
    fun: usize,
    nargs: c_int,
    args: [Sexp; MAX_ARGS],
    result: Sexp,
}

#[repr(C)]
struct PointerCall {
    fun: usize,
    nargs: c_int,
    args: [*mut c_void; MAX_ARGS],
}

unsafe extern "C" {
    fn rox_capi_call_sexp(data: *mut c_void);
    fn rox_capi_call_ptrs(data: *mut c_void);
}

fn label(kind: NativeCallKind) -> &'static str {
    match kind {
        NativeCallKind::Call => ".Call",
        NativeCallKind::External => ".External",
        NativeCallKind::External2 => ".External2",
        NativeCallKind::C => ".C",
        NativeCallKind::Fortran => ".Fortran",
    }
}

pub(crate) fn void_entry(
    ctx: &mut Ctx,
    address: usize,
    value: usize,
    label: &'static str,
) -> EvalResult<()> {
    let mut args = [std::ptr::null_mut(); MAX_ARGS];
    args[0] = std::ptr::with_exposed_provenance_mut(value);
    call_ptrs(ctx, address, 1, args, label)
}

fn call_ptrs(
    ctx: &mut Ctx,
    address: usize,
    nargs: usize,
    args: [*mut c_void; MAX_ARGS],
    label: &'static str,
) -> EvalResult<()> {
    let mut data = PointerCall {
        fun: address,
        nargs: nargs as c_int,
        args,
    };
    // SAFETY: The caller supplies the matching native signature and keeps all buffers live.
    unsafe {
        enter_trampoline_with(
            ctx,
            FrameKind::Entry { label },
            rox_capi_call_ptrs,
            std::ptr::from_mut(&mut data).cast(),
        )
    }
}

fn call_sexp(
    ctx: &mut Ctx,
    address: usize,
    values: &[SexpId],
    label: &'static str,
    call: SexpId,
) -> EvalResult {
    let mut data = SexpCall {
        fun: address,
        nargs: values.len() as c_int,
        args: [Sexp::NULL; MAX_ARGS],
        result: Sexp::NULL,
    };
    for (slot, &value) in data.args.iter_mut().zip(values) {
        *slot = Sexp::from_id(value);
    }
    // SAFETY: Dispatch checked the signature and protected the arguments.
    unsafe {
        enter_trampoline_with(
            ctx,
            FrameKind::Entry { label },
            rox_capi_call_sexp,
            std::ptr::from_mut(&mut data).cast(),
        )?
    };
    if data.result == Sexp::NULL {
        ctx.warning_at(call, "converting NULL pointer to R NULL")?;
        return Ok(SEXP_NIL);
    }
    crate::api::vector::id(ctx, data.result, label)
}

fn resolve(
    ctx: &mut Ctx,
    symbol: SexpId,
    package: Option<&str>,
    kind: Interface,
    call: SexpId,
) -> EvalResult<Routine> {
    let address = match ctx.session().type_of(symbol)? {
        SexpType::Str if ctx.session().xlength(symbol)? == 1 => {
            let ch = ctx.session_mut().str_get(symbol, 0)?;
            if ch == CharId::NA {
                return Err(ctx.stop(call, "invalid native routine name"));
            }
            let name = String::from_utf8_lossy(ctx.session().char_bytes(ch)?).into_owned();
            let registry = ctx.ext::<Registry>();
            let routine = registry
                .lookup(&name, package, Some(kind), true)
                .or_else(|| {
                    registry
                        .lookup(&name, package, None, true)
                        .filter(|routine| routine.registration.is_some())
                });
            return routine.ok_or_else(|| {
                ctx.stop(
                    call,
                    match package {
                        Some(package) => format!(
                            "\"{name}\" not available for .{}() for package \"{package}\"",
                            kind.entry_name()
                        ),
                        None => format!("C symbol name \"{name}\" not in load table"),
                    },
                )
            });
        }
        SexpType::ExtPtr => symbol,
        SexpType::Vec => {
            let names = ctx.session().attr(symbol, SYM_NAMES);
            let index = (0..ctx.session().xlength(names)?).find(|&i| {
                ctx.session_mut()
                    .str_get(names, i)
                    .ok()
                    .and_then(|ch| ctx.session().char_bytes(ch).ok())
                    == Some(b"address")
            });
            let Some(index) = index else {
                return Err(ctx.stop(call, "invalid NativeSymbolInfo object"));
            };
            ctx.session().list_get(symbol, index)?
        }
        _ => {
            return Err(ctx.stop(
                call,
                "first argument must be a string or native symbol reference",
            ));
        }
    };
    let routine = ctx.ext::<Registry>().pointer_routine(address);
    match routine {
        Some(routine) if ctx.session().extptr_addr(address)? == routine.addr => Ok(routine),
        _ => Err(ctx.stop(call, "NULL value passed as symbol address")),
    }
}

impl Interface {
    fn entry_name(self) -> &'static str {
        match self {
            Self::Call => "Call",
            Self::External => "External",
            Self::C => "C",
            Self::Fortran => "Fortran",
        }
    }
}

pub(crate) struct Invocation<'a> {
    pub kind: NativeCallKind,
    pub symbol: SexpId,
    pub args: &'a Args,
    pub package: Option<&'a str>,
    pub dotc: Option<&'a DotCOpts>,
    pub op: SexpId,
    pub env: SexpId,
    pub call: &'a Call,
}

pub(crate) fn dispatch(ctx: &mut Ctx, request: Invocation<'_>) -> EvalResult {
    let Invocation {
        kind,
        symbol,
        args,
        package,
        dotc,
        op,
        env,
        call,
    } = request;
    let interface = Interface::from(kind);
    let label = label(kind);
    let site = call.err_call();
    ctx.scoped(|ctx| {
        for value in [symbol, op, env, call.expr, site]
            .into_iter()
            .chain(args.values().iter().copied())
        {
            ctx.session_mut()
                .heap
                .try_protect(value)
                .map_err(|err| ctx.stop(site, err.to_string()))?;
        }
        if args.len() > MAX_ARGS {
            return Err(ctx.stop(
                site,
                format!("too many arguments in foreign function call (maximum {MAX_ARGS})"),
            ));
        }
        let routine = resolve(
            ctx,
            symbol,
            package.filter(|p| !p.is_empty()),
            interface,
            site,
        )?;
        if let Some((registered, arity)) = routine.registration {
            if registered != interface {
                return Err(ctx.stop(
                    site,
                    format!("'{0}' is not registered for {label}", routine.name),
                ));
            }
            if arity >= 0 && args.len() != arity as usize {
                return Err(ctx.stop(
                    site,
                    format!(
                        "Incorrect number of arguments ({}) passed to '{}' which expects {}",
                        args.len(),
                        routine.name,
                        arity
                    ),
                ));
            }
        }

        match kind {
            NativeCallKind::Call => call_sexp(ctx, routine.addr, args.values(), label, site),
            NativeCallKind::External | NativeCallKind::External2 => {
                let mut pairlist = SEXP_NIL;
                for i in (0..args.len()).rev() {
                    let tag = args.name(i).map_or(SEXP_NIL, |tag| tag.as_sexp());
                    pairlist = ctx.session_mut().cons(args.get(i), pairlist, tag)?;
                }
                pairlist = ctx.session_mut().cons(symbol, pairlist, SEXP_NIL)?;
                ctx.protect(pairlist);
                if kind == NativeCallKind::External2 {
                    call_sexp(
                        ctx,
                        routine.addr,
                        &[call.expr, op, pairlist, env],
                        label,
                        site,
                    )
                } else {
                    call_sexp(ctx, routine.addr, &[pairlist], label, site)
                }
            }
            NativeCallKind::C | NativeCallKind::Fortran => copy_call(
                ctx,
                &routine,
                args,
                kind,
                dotc.is_some_and(|opts| opts.naok),
                site,
            ),
        }
    })
}

const GUARD: usize = 64;
const SENTINEL: u8 = 0x5a;

struct Guarded<T: Copy> {
    base: NonNull<u8>,
    layout: Layout,
    len: usize,
    element: std::marker::PhantomData<T>,
}

impl<T: Copy> Guarded<T> {
    fn new(ctx: &mut Ctx, values: &[T], call: SexpId) -> EvalResult<Self> {
        let size = std::mem::size_of_val(values);
        let layout = size
            .checked_add(GUARD * 2)
            .and_then(|len| Layout::from_size_align(len, GUARD.max(align_of::<T>())).ok())
            .filter(|_| GUARD.is_multiple_of(align_of::<T>()))
            .ok_or_else(|| ctx.stop(call, "invalid foreign argument buffer size"))?;
        // SAFETY: layout is valid and has space for both guards and the entire slice.
        let base = NonNull::new(unsafe { alloc(layout) })
            .ok_or_else(|| ctx.stop(call, "cannot allocate foreign argument buffer"))?;
        let buffer = Self {
            base,
            layout,
            len: values.len(),
            element: std::marker::PhantomData,
        };
        // SAFETY: The ranges are in the allocation and the payload is aligned.
        unsafe {
            base.as_ptr().write_bytes(SENTINEL, GUARD);
            buffer
                .data()
                .copy_from_nonoverlapping(values.as_ptr(), values.len());
            base.as_ptr().add(GUARD + size).write_bytes(SENTINEL, GUARD);
        }
        Ok(buffer)
    }

    fn data(&self) -> *mut T {
        // SAFETY: GUARD is within the allocation and a multiple of T's alignment.
        unsafe { self.base.as_ptr().add(GUARD).cast() }
    }

    fn values(&self) -> &[T] {
        // SAFETY: The constructor initialized the payload and native code has returned.
        unsafe { std::slice::from_raw_parts(self.data(), self.len) }
    }

    fn intact(&self) -> bool {
        let back = self.layout.size() - GUARD;
        // SAFETY: The constructor initialized both guard ranges.
        unsafe {
            [self.base.as_ptr(), self.base.as_ptr().add(back)]
                .into_iter()
                .all(|ptr| {
                    std::slice::from_raw_parts(ptr, GUARD)
                        .iter()
                        .all(|&b| b == SENTINEL)
                })
        }
    }
}

impl<T: Copy> Drop for Guarded<T> {
    fn drop(&mut self) {
        // SAFETY: This buffer owns the allocation and retains its original layout.
        unsafe { dealloc(self.base.as_ptr(), self.layout) };
    }
}

struct Text {
    data: Guarded<u8>,
    original: CharId,
}

enum Buffer {
    Logical(Guarded<RLogical>),
    Integer(Guarded<i32>),
    Real(Guarded<f64>),
    Complex(Guarded<RComplex>),
    Raw(Guarded<u8>),
    List(Guarded<Sexp>),
    Strings {
        pointers: Guarded<*mut u8>,
        text: Vec<Text>,
    },
    FortranString(Text),
}

impl Buffer {
    fn argument(
        ctx: &mut Ctx,
        value: SexpId,
        index: usize,
        fortran: bool,
        naok: bool,
        call: SexpId,
    ) -> EvalResult<Self> {
        macro_rules! numeric {
            ($read:ident, $kind:ident, $bad:expr, $message:literal) => {{
                let values: Vec<_> = ctx.session().$read(value)?.iter().collect();
                if !naok && values.iter().copied().any($bad) {
                    return Err(ctx.stop(
                        call,
                        format!(
                            concat!($message, " in foreign function call (arg {})"),
                            index
                        ),
                    ));
                }
                Self::$kind(Guarded::new(ctx, &values, call)?)
            }};
        }
        Ok(match ctx.session().type_of(value)? {
            SexpType::Lgl => numeric!(lgl, Logical, |v: RLogical| v == RLogical::NA, "NAs"),
            SexpType::Int => numeric!(int, Integer, |v| v == NA_INTEGER, "NAs"),
            SexpType::Real => numeric!(real, Real, |v: f64| !v.is_finite(), "NA/NaN/Inf"),
            SexpType::Cplx => numeric!(
                cplx,
                Complex,
                |v: RComplex| !v.re.is_finite() || !v.im.is_finite(),
                "complex NA/NaN/Inf"
            ),
            SexpType::Raw => {
                let values: Vec<_> = ctx.session().raw(value)?.iter().collect();
                Self::Raw(Guarded::new(ctx, &values, call)?)
            }
            SexpType::Vec => {
                let values = ctx
                    .session()
                    .list_slice(value)?
                    .iter()
                    .copied()
                    .map(Sexp::from_id)
                    .collect::<Vec<_>>();
                Self::List(Guarded::new(ctx, &values, call)?)
            }
            SexpType::Str => {
                let len = ctx.session().xlength(value)?;
                if fortran {
                    if len != 1 {
                        return Err(
                            ctx.stop(call, "character argument to .Fortran must have length one")
                        );
                    }
                    return Ok(Self::FortranString(text(ctx, value, 0, true, call)?));
                }
                let text = (0..len)
                    .map(|i| text(ctx, value, i, false, call))
                    .collect::<EvalResult<Vec<_>>>()?;
                let pointers = text.iter().map(|item| item.data.data()).collect::<Vec<_>>();
                Self::Strings {
                    pointers: Guarded::new(ctx, &pointers, call)?,
                    text,
                }
            }
            _ => return Err(ctx.stop(call, format!("invalid foreign function argument {index}"))),
        })
    }

    fn pointer(&self) -> *mut c_void {
        match self {
            Self::Logical(buf) => buf.data().cast(),
            Self::Integer(buf) => buf.data().cast(),
            Self::Real(buf) => buf.data().cast(),
            Self::Complex(buf) => buf.data().cast(),
            Self::Raw(buf) => buf.data().cast(),
            Self::List(buf) => buf.data().cast(),
            Self::Strings { pointers, .. } => pointers.data().cast(),
            Self::FortranString(text) => text.data.data().cast(),
        }
    }

    fn intact(&self) -> bool {
        match self {
            Self::Logical(buf) => buf.intact(),
            Self::Integer(buf) => buf.intact(),
            Self::Real(buf) => buf.intact(),
            Self::Complex(buf) => buf.intact(),
            Self::Raw(buf) => buf.intact(),
            Self::List(buf) => buf.intact(),
            Self::Strings { pointers, text } => {
                pointers.intact() && text.iter().all(|item| item.data.intact())
            }
            Self::FortranString(text) => text.data.intact(),
        }
    }

    fn into_r(self, ctx: &mut Ctx, call: SexpId) -> EvalResult {
        let session = ctx.session_mut();
        Ok(match self {
            Self::Logical(buf) => session.alloc_lgl_from(buf.values())?,
            Self::Integer(buf) => session.alloc_int_from(buf.values())?,
            Self::Real(buf) => session.alloc_real_from(buf.values())?,
            Self::Complex(buf) => session.alloc_cplx_from(buf.values())?,
            Self::Raw(buf) => session.alloc_raw_from(buf.values())?,
            Self::List(buf) => {
                let list = session.alloc_list(buf.len)?;
                for (i, &value) in buf.values().iter().enumerate() {
                    let value = crate::api::vector::id(ctx, value, "foreign function result")?;
                    ctx.session_mut().list_set(list, i, value)?;
                }
                list
            }
            Self::Strings { pointers, text } => {
                let contents: std::collections::HashMap<_, _> =
                    text.iter().map(|item| (item.data.data(), item)).collect();
                let mut values = Vec::with_capacity(pointers.len);
                for ptr in pointers.values() {
                    let Some(item) = contents.get(ptr) else {
                        return Err(
                            ctx.stop(call, "foreign function replaced character buffer address")
                        );
                    };
                    values.push(item.to_r(ctx, call)?);
                }
                ctx.session_mut().alloc_str_from(&values)?
            }
            Self::FortranString(text) => {
                let value = text.to_r(ctx, call)?;
                ctx.session_mut().alloc_str_from(&[value])?
            }
        })
    }
}

fn text(
    ctx: &mut Ctx,
    value: SexpId,
    index: usize,
    fortran: bool,
    call: SexpId,
) -> EvalResult<Text> {
    let original = ctx.session_mut().str_get(value, index)?;
    let mut bytes = ctx.session().char_bytes(original)?.to_vec();
    if fortran {
        bytes.resize(255, 0);
    }
    bytes.push(0);
    Ok(Text {
        data: Guarded::new(ctx, &bytes, call)?,
        original,
    })
}

impl Text {
    fn to_r(&self, ctx: &mut Ctx, call: SexpId) -> EvalResult<CharId> {
        let bytes = self.data.values();
        let Some(end) = bytes.iter().position(|&byte| byte == 0) else {
            return Err(ctx.stop(
                call,
                "foreign function returned an unterminated character buffer",
            ));
        };
        let bytes = &bytes[..end];
        let session = ctx.session_mut();
        if bytes == session.char_bytes(self.original)? {
            return Ok(self.original);
        }
        let encoding = if self.original == CharId::NA {
            Encoding::Native
        } else {
            session.char_encoding(self.original)?
        };
        Ok(session.mk_char(bytes, encoding)?)
    }
}

fn copy_call(
    ctx: &mut Ctx,
    routine: &Routine,
    args: &Args,
    kind: NativeCallKind,
    naok: bool,
    call: SexpId,
) -> EvalResult {
    let label = label(kind);
    let buffers = args
        .values()
        .iter()
        .enumerate()
        .map(|(i, &value)| {
            Buffer::argument(
                ctx,
                value,
                i + 1,
                kind == NativeCallKind::Fortran,
                naok,
                call,
            )
        })
        .collect::<EvalResult<Vec<_>>>()?;
    let mut pointers = [std::ptr::null_mut(); MAX_ARGS];
    for (slot, buffer) in pointers.iter_mut().zip(&buffers) {
        *slot = buffer.pointer();
    }
    call_ptrs(ctx, routine.addr, buffers.len(), pointers, label)?;
    if let Some(index) = buffers.iter().position(|buffer| !buffer.intact()) {
        return Err(ctx.stop(
            call,
            format!(
                "array over-run in {label}(\"{}\") argument {}",
                routine.name,
                index + 1
            ),
        ));
    }
    let result = ctx.session_mut().alloc_list(buffers.len())?;
    for (i, buffer) in buffers.into_iter().enumerate() {
        let value = buffer.into_r(ctx, call)?;
        ctx.session_mut().list_set(result, i, value)?;
    }
    if args.iter().any(|(name, _)| name.is_some()) {
        let names = args
            .iter()
            .map(|(name, _)| {
                name.map_or(Ok(""), |name| ctx.session().sym_name(name))
                    .map(str::to_owned)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let names = names.iter().map(String::as_str).collect::<Vec<_>>();
        let names = dll::string_vector(ctx.session_mut(), &names)?;
        ctx.session_mut().set_attrib(result, SYM_NAMES, names)?;
    }
    Ok(result)
}
