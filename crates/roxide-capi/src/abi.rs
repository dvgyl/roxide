//! C ABI types shared by the native call boundary and API exports.

use std::ffi::{c_int, c_uint, c_void};

use roxide_gc::SexpId;
use roxide_value::SexpType;

/// Opaque handle. C code must never dereference it.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sexp(pub usize);

impl Sexp {
    /// Null C pointer.
    pub const NULL: Self = Self(0);

    /// Encode an R object handle.
    pub const fn from_id(id: SexpId) -> Self {
        Self(id.to_bits() as usize)
    }

    /// Decode an R object handle. Heap access must still check its liveness.
    pub const fn to_id(self) -> Option<SexpId> {
        SexpId::from_bits(self.0 as u64)
    }
}

const _: () = assert!(size_of::<Sexp>() == 8);
const _: () = assert!(size_of::<*mut c_void>() == 8);

/// C boolean with values 0 and 1.
pub type Rboolean = c_int;
/// Byte in an R raw vector.
pub type Rbyte = u8;
/// Short vector length.
#[allow(non_camel_case_types)]
pub type R_len_t = c_int;
/// Long vector length.
#[allow(non_camel_case_types)]
pub type R_xlen_t = isize;
/// Numeric R object type.
pub type SEXPTYPE = c_uint;

/// Complex number in the public C ABI.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rcomplex {
    /// Real component.
    pub r: f64,
    /// Imaginary component.
    pub i: f64,
}

macro_rules! sexptypes {
    ($($(#[$doc:meta])* $name:ident = $variant:ident),+ $(,)?) => {
        $($(#[$doc])* pub const $name: SEXPTYPE = SexpType::$variant as SEXPTYPE;)+
    };
}

sexptypes! {
    /// R NULL.
    NILSXP = Nil,
    /// Symbol.
    SYMSXP = Sym,
    /// Pairlist.
    LISTSXP = List,
    /// Closure.
    CLOSXP = Clo,
    /// Environment.
    ENVSXP = Env,
    /// Promise.
    PROMSXP = Prom,
    /// Language call.
    LANGSXP = Lang,
    /// Special primitive.
    SPECIALSXP = Special,
    /// Builtin primitive.
    BUILTINSXP = Builtin,
    /// Interned string.
    CHARSXP = Char,
    /// Logical vector.
    LGLSXP = Lgl,
    /// Integer vector.
    INTSXP = Int,
    /// Double vector.
    REALSXP = Real,
    /// Complex vector.
    CPLXSXP = Cplx,
    /// Character vector.
    STRSXP = Str,
    /// Ellipsis arguments.
    DOTSXP = Dot,
    /// Unrestricted object type.
    ANYSXP = Any,
    /// List vector.
    VECSXP = Vec,
    /// Expression vector.
    EXPRSXP = Expr,
    /// Bytecode.
    BCODESXP = Bcode,
    /// External pointer.
    EXTPTRSXP = ExtPtr,
    /// Weak reference.
    WEAKREFSXP = WeakRef,
    /// Raw vector.
    RAWSXP = Raw,
    /// Object.
    OBJSXP = Obj,
    /// S4 object.
    S4SXP = Obj,
}
