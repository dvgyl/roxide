//! Class registration and dispatch for the foreign ALTREP C API.

mod callbacks;
mod dispatch;
mod exports;
mod payload;
mod registry;
mod vector;

pub use callbacks::*;
pub use dispatch::inspect;
pub use exports::*;

pub(crate) use dispatch::{dispatch, resolve_for_boundary};
pub(crate) use vector::{
    c_dataptr, c_duplicate, c_flags, c_or_null, c_region, c_set_elt, cplx_elt, int_elt, lgl_elt,
    object_elt, raw_elt, real_elt, string_elt,
};

pub(super) use payload::{Access, materialize};
