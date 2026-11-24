use std::ffi::{c_int, c_void};
use std::mem::MaybeUninit;
use std::ptr::from_mut;

use roxide_eval::{Ctx, EvalResult};
use roxide_value::SEXP_NIL;

use super::exports::R_altrep_class_t;
use super::registry::{active, class_index, methods_mut};
use crate::abi::*;
use crate::api::vector::{api, id};
use crate::dll::{code_owner, with_owner};
use crate::jump::{FrameKind, VoidFn, enter_trampoline_with};

#[derive(Clone, Copy)]
struct Method<F> {
    fun: F,
    owner: u32,
}

fn invoke<T>(ctx: &mut Ctx, owner: u32, adapter: VoidFn, data: &mut T) -> EvalResult<()> {
    if !active(ctx, owner) {
        return Err(ctx.stop(SEXP_NIL, "ALTREP method library was unloaded"));
    }
    with_owner(ctx, owner, |ctx| {
        // SAFETY: Each caller pairs a live C-layout record with its C adapter.
        unsafe { enter_trampoline_with(ctx, FrameKind::Method, adapter, from_mut(data).cast()) }
    })
}

macro_rules! output {
    ($call:ident, ()) => {
        ()
    };
    ($call:ident, $ret:ty) => {{
        // SAFETY: A normal return from the matching adapter initializes the result.
        unsafe { $call.result.assume_init() }
    }};
}

macro_rules! callbacks {
    ($($setter:ident $method:ident $adapter:ident $call:ident [$ty:pat]
       ($($arg:ident: $argty:ty),*) -> $ret:tt;)*) => {
        #[derive(Clone, Copy, Default)]
        pub(super) struct Methods {
            $($method: Option<Method<unsafe extern "C" fn($($argty),*) -> $ret>>,)*
        }

        unsafe extern "C" {
            $(fn $adapter(data: *mut c_void);)*
        }

        $(
            #[repr(C)]
            struct $call {
                fun: unsafe extern "C" fn($($argty),*) -> $ret,
                $($arg: $argty,)*
                result: MaybeUninit<$ret>,
            }

            api! {
                /// Set the callback for this ALTREP method. A C NULL pointer clears the callback.
                fn $setter(klass: R_altrep_class_t, fun: Option<unsafe extern "C" fn($($argty),*) -> $ret>) -> () = (); |ctx| {
                    let descriptor = id(ctx, klass.ptr, stringify!($setter))?;
                    let index = class_index(ctx, descriptor)?;
                    let class = super::registry::class(ctx, descriptor)?;
                    if !matches!(class.ty, $ty) {
                        return Err(ctx.stop(SEXP_NIL, "ALTREP method does not match the class type"));
                    }
                    let method = fun.map(|fun| Method {
                        fun,
                        owner: code_owner(ctx, (fun as *const ()).addr(), class.owner),
                    });
                    methods_mut(ctx, index).$method = method;
                    Ok(())
                }
            }
        )*

        impl Methods {
            pub(super) fn has_dataptr(&self) -> bool {
                self.dataptr.is_some()
            }

            $(
                pub(super) fn $method(self, ctx: &mut Ctx, $($arg: $argty),*) -> EvalResult<Option<$ret>> {
                    let Some(method) = self.$method else { return Ok(None) };
                    let mut call = $call { fun: method.fun, $($arg,)* result: MaybeUninit::uninit() };
                    invoke(ctx, method.owner, $adapter, &mut call)?;
                    Ok(Some(output!(call, $ret)))
                }
            )*
        }
    };
}

use roxide_value::SexpType::{Cplx, Int, Lgl, Raw, Real, Str, Vec};

type Pointer = *mut c_void;
type ConstPointer = *const c_void;
type InspectSubtree = Option<unsafe extern "C" fn(Sexp, c_int, c_int, c_int)>;

callbacks! {
    R_set_altrep_Length_method length rox_capi_alt_length Length [_](x: Sexp) -> R_xlen_t;
    R_set_altrep_Serialized_state_method serialized_state rox_capi_alt_serialized_state State [_](x: Sexp) -> Sexp;
    R_set_altrep_Unserialize_method unserialize rox_capi_alt_unserialize Unserialize [_](info: Sexp, state: Sexp) -> Sexp;
    R_set_altrep_UnserializeEX_method unserialize_ex rox_capi_alt_unserialize_ex UnserializeEx [_](info: Sexp, state: Sexp, attrs: Sexp, objf: c_int, levels: c_int) -> Sexp;
    R_set_altrep_Duplicate_method duplicate rox_capi_alt_duplicate Duplicate [_](x: Sexp, deep: Rboolean) -> Sexp;
    R_set_altrep_DuplicateEX_method duplicate_ex rox_capi_alt_duplicate_ex DuplicateEx [_](x: Sexp, deep: Rboolean) -> Sexp;
    R_set_altrep_Coerce_method coerce rox_capi_alt_coerce Coerce [_](x: Sexp, ty: c_int) -> Sexp;
    R_set_altrep_Inspect_method inspect rox_capi_alt_inspect Inspect [_](x: Sexp, pre: c_int, deep: c_int, pvec: c_int, subtree: InspectSubtree) -> Rboolean;
    R_set_altvec_Extract_subset_method extract_subset rox_capi_alt_extract_subset ExtractSubset [_](x: Sexp, index: Sexp, call: Sexp) -> Sexp;
    R_set_altvec_Dataptr_method dataptr rox_capi_alt_dataptr DataPtr [_](x: Sexp, writable: Rboolean) -> Pointer;
    R_set_altvec_Dataptr_or_null_method dataptr_or_null rox_capi_alt_dataptr_or_null DataPtrOrNull [_](x: Sexp) -> ConstPointer;
    R_set_altinteger_Elt_method int_elt rox_capi_alt_int_elt IntElt [Int](x: Sexp, i: R_xlen_t) -> c_int;
    R_set_altreal_Elt_method real_elt rox_capi_alt_real_elt RealElt [Real](x: Sexp, i: R_xlen_t) -> f64;
    R_set_altlogical_Elt_method lgl_elt rox_capi_alt_lgl_elt LglElt [Lgl](x: Sexp, i: R_xlen_t) -> c_int;
    R_set_altraw_Elt_method raw_elt rox_capi_alt_raw_elt RawElt [Raw](x: Sexp, i: R_xlen_t) -> Rbyte;
    R_set_altcomplex_Elt_method cplx_elt rox_capi_alt_cplx_elt CplxElt [Cplx](x: Sexp, i: R_xlen_t) -> Rcomplex;
    R_set_altstring_Elt_method str_elt rox_capi_alt_str_elt StrElt [Str](x: Sexp, i: R_xlen_t) -> Sexp;
    R_set_altlist_Elt_method list_elt rox_capi_alt_list_elt ListElt [Vec](x: Sexp, i: R_xlen_t) -> Sexp;
    R_set_altstring_Set_elt_method str_set_elt rox_capi_alt_str_set_elt StrSet [Str](x: Sexp, i: R_xlen_t, value: Sexp) -> ();
    R_set_altlist_Set_elt_method list_set_elt rox_capi_alt_list_set_elt ListSet [Vec](x: Sexp, i: R_xlen_t, value: Sexp) -> ();
    R_set_altinteger_Get_region_method int_region rox_capi_alt_int_get_region IntRegion [Int](x: Sexp, start: R_xlen_t, len: R_xlen_t, out: *mut c_int) -> R_xlen_t;
    R_set_altreal_Get_region_method real_region rox_capi_alt_real_get_region RealRegion [Real](x: Sexp, start: R_xlen_t, len: R_xlen_t, out: *mut f64) -> R_xlen_t;
    R_set_altlogical_Get_region_method lgl_region rox_capi_alt_lgl_get_region LglRegion [Lgl](x: Sexp, start: R_xlen_t, len: R_xlen_t, out: *mut c_int) -> R_xlen_t;
    R_set_altraw_Get_region_method raw_region rox_capi_alt_raw_get_region RawRegion [Raw](x: Sexp, start: R_xlen_t, len: R_xlen_t, out: *mut Rbyte) -> R_xlen_t;
    R_set_altcomplex_Get_region_method cplx_region rox_capi_alt_cplx_get_region CplxRegion [Cplx](x: Sexp, start: R_xlen_t, len: R_xlen_t, out: *mut Rcomplex) -> R_xlen_t;
    R_set_altinteger_Is_sorted_method int_sorted rox_capi_alt_int_is_sorted IntSorted [Int](x: Sexp) -> c_int;
    R_set_altreal_Is_sorted_method real_sorted rox_capi_alt_real_is_sorted RealSorted [Real](x: Sexp) -> c_int;
    R_set_altlogical_Is_sorted_method lgl_sorted rox_capi_alt_lgl_is_sorted LglSorted [Lgl](x: Sexp) -> c_int;
    R_set_altstring_Is_sorted_method str_sorted rox_capi_alt_str_is_sorted StrSorted [Str](x: Sexp) -> c_int;
    R_set_altinteger_No_NA_method int_no_na rox_capi_alt_int_no_na IntNoNa [Int](x: Sexp) -> c_int;
    R_set_altreal_No_NA_method real_no_na rox_capi_alt_real_no_na RealNoNa [Real](x: Sexp) -> c_int;
    R_set_altlogical_No_NA_method lgl_no_na rox_capi_alt_lgl_no_na LglNoNa [Lgl](x: Sexp) -> c_int;
    R_set_altstring_No_NA_method str_no_na rox_capi_alt_str_no_na StrNoNa [Str](x: Sexp) -> c_int;
    R_set_altinteger_Sum_method int_sum rox_capi_alt_int_sum IntSum [Int](x: Sexp, na_rm: Rboolean) -> Sexp;
    R_set_altreal_Sum_method real_sum rox_capi_alt_real_sum RealSum [Real](x: Sexp, na_rm: Rboolean) -> Sexp;
    R_set_altlogical_Sum_method lgl_sum rox_capi_alt_lgl_sum LglSum [Lgl](x: Sexp, na_rm: Rboolean) -> Sexp;
    R_set_altinteger_Min_method int_min rox_capi_alt_int_min IntMin [Int](x: Sexp, na_rm: Rboolean) -> Sexp;
    R_set_altreal_Min_method real_min rox_capi_alt_real_min RealMin [Real](x: Sexp, na_rm: Rboolean) -> Sexp;
    R_set_altinteger_Max_method int_max rox_capi_alt_int_max IntMax [Int](x: Sexp, na_rm: Rboolean) -> Sexp;
    R_set_altreal_Max_method real_max rox_capi_alt_real_max RealMax [Real](x: Sexp, na_rm: Rboolean) -> Sexp;
}

macro_rules! bulk_fill {
    ($($fill:ident $element:ty, $adapter:ident $record:ident $elt:ident;)*) => {
        unsafe extern "C" {
            $(fn $adapter(data: *mut c_void);)*
        }

        impl Methods {
            $(
                pub(super) fn $fill(
                    self, ctx: &mut Ctx, x: Sexp, len: R_xlen_t, out: *mut $element,
                ) -> EvalResult<bool> {
                    #[repr(C)]
                    struct $record {
                        fun: unsafe extern "C" fn(Sexp, R_xlen_t) -> $element,
                        x: Sexp,
                        len: R_xlen_t,
                        out: *mut $element,
                    }
                    let Some(method) = self.$elt else { return Ok(false) };
                    let mut call = $record { fun: method.fun, x, len, out };
                    invoke(ctx, method.owner, $adapter, &mut call)?;
                    Ok(true)
                }
            )*
        }
    };
}

bulk_fill! {
    int_fill c_int, rox_capi_alt_fill_int_elt FillInt int_elt;
    real_fill f64, rox_capi_alt_fill_real_elt FillReal real_elt;
    lgl_fill c_int, rox_capi_alt_fill_lgl_elt FillLgl lgl_elt;
    raw_fill Rbyte, rox_capi_alt_fill_raw_elt FillRaw raw_elt;
    cplx_fill Rcomplex, rox_capi_alt_fill_cplx_elt FillCplx cplx_elt;
}
