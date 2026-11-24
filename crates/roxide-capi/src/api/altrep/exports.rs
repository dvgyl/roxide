use std::ffi::{c_char, c_int, c_void};
use std::ptr::{null, null_mut};

use roxide_eval::{Ctx, EvalResult};
use roxide_value::{AltSlot, SEXP_NIL, SexpType, ValueApi};

use super::payload::{Access, materialize};
use super::registry::{class, descriptor, foreign, register};
use super::vector::string_elt;
use crate::abi::{R_xlen_t, Rboolean, Sexp};
use crate::api::attrib::copy_attrs;
use crate::api::string::c_text;
use crate::api::sym::install;
use crate::api::vector::{allocating, api, id, is_atomic, length, read_ptr, write_ptr};

/// Opaque ALTREP class descriptor used by the C API.
#[repr(C)]
#[derive(Clone, Copy)]
#[allow(non_camel_case_types)]
pub struct R_altrep_class_t {
    /// Class descriptor as an R object.
    pub ptr: Sexp,
}

const NULL_CLASS: R_altrep_class_t = R_altrep_class_t { ptr: Sexp::NULL };

macro_rules! constructors {
    ($($name:ident => $ty:ident),+ $(,)?) => {
        api! {$(
            /// Register a foreign ALTREP class of the specified vector type.
            fn $name(name: *const c_char, package: *const c_char, dll: *mut c_void)
                -> R_altrep_class_t = NULL_CLASS; |ctx| {
                let name = c_text(ctx, name, stringify!($name))?;
                let package = c_text(ctx, package, stringify!($name))?;
                let name = install(ctx, name)?;
                let package = install(ctx, package)?;
                let s = ctx.session();
                let name = s.sexp_as_sym(name)?;
                let package = s.sexp_as_sym(package)?;
                let ptr = register(ctx, name, package, SexpType::$ty, dll)?;
                Ok(R_altrep_class_t { ptr: Sexp::from_id(ptr) })
            }
        )+}
    };
}

constructors!(
    R_make_altinteger_class => Int,
    R_make_altreal_class => Real,
    R_make_altlogical_class => Lgl,
    R_make_altraw_class => Raw,
    R_make_altcomplex_class => Cplx,
    R_make_altstring_class => Str,
    R_make_altlist_class => Vec,
);

fn class_symbol(ctx: &mut Ctx, x: Sexp, package: bool) -> EvalResult<Sexp> {
    let x = id(ctx, x, "ALTREP class metadata")?;
    let descriptor = descriptor(ctx, x)?;
    let query = if descriptor == SEXP_NIL {
        x
    } else {
        descriptor
    };
    let names = ctx.session().alt_class_names(query)?;
    Ok(names.map_or(Sexp::from_id(SEXP_NIL), |(name, pkg)| {
        Sexp::from_id(if package { pkg } else { name }.as_sexp())
    }))
}

macro_rules! slots {
    ($($get:ident $set:ident $slot:ident $field:ident),+ $(,)?) => {
        api! {$(
            /// Return the value in this foreign ALTREP data slot.
            fn $get(x: Sexp) -> Sexp = Sexp::NULL; |ctx| {
                let x = id(ctx, x, stringify!($get))?;
                Ok(Sexp::from_id(foreign(ctx, x)?.$field))
            }
            /// Replace the value in this foreign ALTREP data slot.
            fn $set(x: Sexp, value: Sexp) -> () = (); |ctx| {
                let x = id(ctx, x, stringify!($set))?;
                let value = id(ctx, value, stringify!($set))?;
                Ok(ctx.session_mut().set_foreign_alt_data(x, AltSlot::$slot, value)?)
            }
        )+}
    };
}

slots!(
    R_altrep_data1 R_set_altrep_data1 Data1 data1,
    R_altrep_data2 R_set_altrep_data2 Data2 data2,
);

api! {
    /// Create a lazy foreign ALTREP vector with two R data slots.
    fn R_new_altrep(klass: R_altrep_class_t, data1: Sexp, data2: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        let descriptor = id(ctx, klass.ptr, "R_new_altrep")?;
        let class = class(ctx, descriptor)?;
        let data1 = id(ctx, data1, "R_new_altrep")?;
        let data2 = id(ctx, data2, "R_new_altrep")?;
        allocating(ctx, &[descriptor, data1, data2], |ctx| {
            Ok(Sexp::from_id(ctx.session_mut().alloc_foreign_alt(descriptor, class.ty, data1, data2)?))
        })
    }
    /// Test whether the vector has the specified ALTREP class descriptor.
    fn R_altrep_inherits(x: Sexp, klass: R_altrep_class_t) -> Rboolean = 0; |ctx| {
        let x = id(ctx, x, "R_altrep_inherits")?;
        Ok(Rboolean::from(ctx.session().is_altrep(x)? && Sexp::from_id(descriptor(ctx, x)?) == klass.ptr))
    }
    /// Test whether an object has an ALTREP representation.
    fn ALTREP(x: Sexp) -> c_int = 0; |ctx| {
        let x = id(ctx, x, "ALTREP")?;
        Ok(c_int::from(ctx.session().is_altrep(x)?))
    }
    /// Return the object's ALTREP class descriptor.
    fn ALTREP_CLASS(x: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        let x = id(ctx, x, "ALTREP_CLASS")?;
        Ok(Sexp::from_id(descriptor(ctx, x)?))
    }
    /// Return the ALTREP class name symbol.
    fn R_altrep_class_name(x: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        class_symbol(ctx, x, false)
    }
    /// Return the ALTREP package name symbol.
    fn R_altrep_class_package(x: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        class_symbol(ctx, x, true)
    }
    /// Wrap an atomic vector in an ALTREP object when possible.
    fn R_tryWrap(x: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        let value = id(ctx, x, "R_tryWrap")?;
        let ty = ctx.session().type_of(value)?;
        if !is_atomic(ty) {
            return Ok(x);
        }
        allocating(ctx, &[value], |ctx| {
            if ctx.session().foreign_alt(value)?.is_some() {
                materialize(ctx, value, Access::Evaluate)?;
            }
            if ty == SexpType::Str { ctx.session_mut().str_dataptr_ro(value)?; }
            let result = ctx.session_mut().alloc_alt_wrapper(value)?;
            copy_attrs(ctx, value, result, false)?;
            Ok(Sexp::from_id(result))
        })
    }
    /// Return the length of an ALTREP vector.
    fn ALTREP_LENGTH(x: Sexp) -> R_xlen_t = 0; |ctx| {
        let x = id(ctx, x, "ALTREP_LENGTH")?;
        Ok(length(ctx, x)? as R_xlen_t)
    }
    /// Return the writable data pointer for an ALTREP vector.
    fn ALTVEC_DATAPTR(x: Sexp) -> *mut c_void = null_mut(); |ctx| {
        let x = id(ctx, x, "ALTVEC_DATAPTR")?;
        Ok(write_ptr(ctx, x)? as *mut c_void)
    }
    /// Return the read-only data pointer for an ALTREP vector.
    fn ALTVEC_DATAPTR_RO(x: Sexp) -> *const c_void = null(); |ctx| {
        let x = id(ctx, x, "ALTVEC_DATAPTR_RO")?;
        let ty = ctx.session().type_of(x)?;
        read_ptr(ctx, x, ty)
    }
    /// Return an interned string from an ALTREP character vector.
    fn ALTSTRING_ELT(x: Sexp, i: R_xlen_t) -> Sexp = Sexp::NULL; |ctx| {
        let x = id(ctx, x, "ALTSTRING_ELT")?;
        Ok(Sexp::from_id(string_elt(ctx, x, i, "ALTSTRING_ELT")?))
    }
}
