//! Symbol interning (`Rf_install`, `Rf_installChar`, `PRINTNAME`).

use std::ffi::c_char;
use std::ptr::null;

use roxide_eval::{Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{CharId, Encoding, SEXP_NIL};

use super::string::{c_text, utf8};
use super::vector::{allocating, api, check_capacity, id};
use crate::abi::Sexp;

pub(crate) fn install(ctx: &mut Ctx, bytes: &[u8]) -> EvalResult<SexpId> {
    if bytes.is_empty() {
        return Err(ctx.stop(SEXP_NIL, "attempt to use zero-length variable name"));
    }
    let name = std::str::from_utf8(bytes)
        .map_err(|_| ctx.stop(SEXP_NIL, "invalid UTF-8 in symbol name"))?;
    Ok(ctx.session_mut().intern(name)?.as_sexp())
}

pub(crate) fn install_char(
    ctx: &mut Ctx,
    input: Sexp,
    function: &str,
    translate: bool,
) -> EvalResult<Sexp> {
    let x = id(ctx, input, function)?;
    allocating(ctx, &[x], |ctx| {
        let s = ctx.session();
        let ch = s.sexp_as_char(x)?;
        let bytes = s.char_bytes(ch)?;
        let enc = s.char_encoding(ch)?;
        let bytes = if translate {
            utf8(bytes, enc).into_owned()
        } else {
            bytes.to_vec()
        };
        install(ctx, &bytes).map(Sexp::from_id)
    })
}

pub(crate) fn print_name(ctx: &mut Ctx, x: SexpId) -> EvalResult<CharId> {
    let text = ctx.session().sym_name_of(x)?.to_owned();
    let encoding = if text.is_ascii() {
        Encoding::Native
    } else {
        Encoding::Utf8
    };
    Ok(ctx.session_mut().mk_char(text.as_bytes(), encoding)?)
}

api! {
    /// Intern a symbol from a NUL-terminated name.
    fn Rf_install(name: *const c_char) -> Sexp = Sexp::NULL; |ctx| {
        let name = c_text(ctx, name, "Rf_install")?;
        check_capacity(ctx, name.len(), 1)?;
        let name = name.to_vec();
        allocating(ctx, &[], |ctx| install(ctx, &name).map(Sexp::from_id))
    }
    /// Intern a symbol from an interned character string.
    fn Rf_installChar(name: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        install_char(ctx, name, "Rf_installChar", true)
    }
    /// Intern a symbol without translating the supplied character string.
    fn Rf_installNoTrChar(name: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        install_char(ctx, name, "Rf_installNoTrChar", false)
    }
    /// Return the interned print name of a symbol.
    fn PRINTNAME(x: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        let x = id(ctx, x, "PRINTNAME")?;
        Ok(Sexp::from_id(print_name(ctx, x)?.as_sexp()))
    }
    /// Return the NUL-terminated print name of a symbol.
    fn R_SymName(x: Sexp) -> *const c_char = null(); |ctx| {
        let x = id(ctx, x, "R_SymName")?;
        let name = print_name(ctx, x)?;
        Ok(ctx.session().char_cstr_ptr(name)?.0 as *const c_char)
    }
}
