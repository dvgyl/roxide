//! Character string interning, encoding, UTF-8 translation, and `STRING_ELT`.

use std::borrow::Cow;
use std::collections::HashSet;
use std::ffi::{CStr, CString, c_char, c_int};
use std::ptr::null;

use roxide_eval::{Ctx, CtxExt, EvalResult};
use roxide_gc::{SexpId, Trace, Tracer};
use roxide_value::{CharId, Encoding, SEXP_NIL, SexpType, ValueApi};

use super::vector::{allocating, api, check_capacity, checked_len, id, index, require_type};
use crate::abi::{R_xlen_t, Rboolean, Sexp};

const CE_NATIVE: c_int = 0;
const CE_UTF8: c_int = 1;
const CE_LATIN1: c_int = 2;
const CE_BYTES: c_int = 3;

pub(crate) unsafe fn c_text<'a>(
    ctx: &mut Ctx,
    text: *const c_char,
    name: &str,
) -> EvalResult<&'a [u8]> {
    if text.is_null() {
        return Err(ctx.stop(SEXP_NIL, format!("invalid NULL string passed to {name}")));
    }
    // SAFETY: The caller supplies a readable NUL-terminated string.
    Ok(unsafe { CStr::from_ptr(text) }.to_bytes())
}

fn encoding(ctx: &mut Ctx, ce: c_int) -> EvalResult<Encoding> {
    match ce {
        CE_NATIVE => Ok(Encoding::Native),
        CE_UTF8 => Ok(Encoding::Utf8),
        CE_LATIN1 => Ok(Encoding::Latin1),
        CE_BYTES => Ok(Encoding::Bytes),
        _ => Err(ctx.stop(SEXP_NIL, "unknown encoding")),
    }
}

fn encoding_code(encoding: Encoding) -> c_int {
    match encoding {
        Encoding::Native => CE_NATIVE,
        Encoding::Utf8 => CE_UTF8,
        Encoding::Latin1 => CE_LATIN1,
        Encoding::Bytes => CE_BYTES,
    }
}

unsafe fn make_char(
    ctx: &mut Ctx,
    text: *const c_char,
    len: Option<c_int>,
    ce: c_int,
    name: &str,
) -> EvalResult<Sexp> {
    let enc = encoding(ctx, ce)?;
    let bytes = if let Some(len) = len {
        let len = checked_len(ctx, len as isize)?;
        if text.is_null() {
            return Err(ctx.stop(SEXP_NIL, format!("invalid NULL string passed to {name}")));
        }
        // SAFETY: The caller supplies a readable buffer of this length.
        unsafe { std::slice::from_raw_parts(text.cast(), len) }
    } else {
        // SAFETY: The caller supplies a readable NUL-terminated string.
        unsafe { c_text(ctx, text, name)? }
    };
    check_capacity(ctx, bytes.len(), 1)?;
    // The input can point into an unprotected R character object.
    let bytes = bytes.to_vec();
    allocating(ctx, &[], |ctx| {
        Ok(Sexp::from_id(
            ctx.session_mut().mk_char(&bytes, enc)?.as_sexp(),
        ))
    })
}

pub(crate) fn utf8(bytes: &[u8], enc: Encoding) -> Cow<'_, [u8]> {
    if enc == Encoding::Latin1 && !bytes.is_ascii() {
        Cow::Owned(
            bytes
                .iter()
                .map(|&byte| char::from(byte))
                .collect::<String>()
                .into_bytes(),
        )
    } else {
        Cow::Borrowed(bytes)
    }
}

// Keep converted buffers stable for the session, including across collection.
#[derive(Default)]
struct Translations {
    strings: HashSet<CString>,
    bytes: usize,
}

impl CtxExt for Translations {}
impl Trace for Translations {
    fn trace(&self, _: &mut Tracer<'_>) {}
}

fn translated_ptr(ctx: &mut Ctx, bytes: Vec<u8>) -> EvalResult<*const c_char> {
    const MAX_TRANSLATION_BYTES: usize = 64 * 1024 * 1024;
    let text = CString::new(bytes).map_err(|err| ctx.stop(SEXP_NIL, err.to_string()))?;
    let cache = ctx.ext::<Translations>();
    if let Some(text) = cache.strings.get(&text) {
        return Ok(text.as_ptr());
    }
    let size = text.as_bytes_with_nul().len() + size_of::<CString>();
    if size > MAX_TRANSLATION_BYTES.saturating_sub(cache.bytes) {
        return Err(ctx.stop(SEXP_NIL, "character translation storage limit exceeded"));
    }
    let ptr = text.as_ptr();
    cache.strings.insert(text);
    cache.bytes += size;
    Ok(ptr)
}

fn translate(ctx: &mut Ctx, x: SexpId) -> EvalResult<*const c_char> {
    let s = ctx.session();
    let ch = s.sexp_as_char(x)?;
    let enc = s.char_encoding(ch)?;
    if enc == Encoding::Bytes {
        return Err(ctx.stop(
            SEXP_NIL,
            "translating strings with \"bytes\" encoding is not allowed",
        ));
    }
    match utf8(s.char_bytes(ch)?, enc) {
        Cow::Borrowed(_) => Ok(s.char_cstr_ptr(ch)?.0 as *const c_char),
        Cow::Owned(bytes) => translated_ptr(ctx, bytes),
    }
}

fn reencode(
    ctx: &mut Ctx,
    text: *const c_char,
    bytes: &[u8],
    from: Encoding,
    to: Encoding,
    subst: c_int,
) -> EvalResult<*const c_char> {
    if from == to || bytes.is_ascii() || from == Encoding::Bytes || to == Encoding::Bytes {
        return Ok(text);
    }
    let bytes = utf8(bytes, from);
    if to != Encoding::Latin1 {
        return match bytes {
            Cow::Borrowed(_) => Ok(text),
            Cow::Owned(bytes) => translated_ptr(ctx, bytes),
        };
    }
    let Ok(value) = std::str::from_utf8(&bytes) else {
        return Ok(text);
    };
    let mut out = Vec::with_capacity(value.len());
    for ch in value.chars() {
        if let Ok(byte) = u8::try_from(ch as u32) {
            out.push(byte);
        } else {
            match subst {
                1 => out.push(b'.'),
                2 => {
                    let mut buf = [0u8; 4];
                    for b in ch.encode_utf8(&mut buf).bytes() {
                        out.extend_from_slice(format!("<{b:02x}>").as_bytes());
                    }
                }
                3 => out.push(b'?'),
                _ => return Ok(text),
            }
        }
    }
    translated_ptr(ctx, out)
}

api! {
    /// Intern a NUL-terminated string in the native encoding.
    fn Rf_mkChar(text: *const c_char) -> Sexp = Sexp::NULL; |ctx| {
        make_char(ctx, text, None, CE_NATIVE, "Rf_mkChar")
    }
    /// Intern a NUL-terminated string with an encoding mark.
    fn Rf_mkCharCE(text: *const c_char, ce: c_int) -> Sexp = Sexp::NULL; |ctx| {
        make_char(ctx, text, None, ce, "Rf_mkCharCE")
    }
    /// Intern a string from a sized buffer.
    fn Rf_mkCharLen(text: *const c_char, len: c_int) -> Sexp = Sexp::NULL; |ctx| {
        make_char(ctx, text, Some(len), CE_NATIVE, "Rf_mkCharLen")
    }
    /// Intern a string from a sized buffer with an encoding mark.
    fn Rf_mkCharLenCE(text: *const c_char, len: c_int, ce: c_int) -> Sexp = Sexp::NULL; |ctx| {
        make_char(ctx, text, Some(len), ce, "Rf_mkCharLenCE")
    }
    /// Return the NUL-terminated bytes of an interned string.
    fn R_CHAR(x: Sexp) -> *const c_char = null(); |ctx| {
        let x = id(ctx, x, "R_CHAR")?;
        let s = ctx.session();
        Ok(s.char_cstr_ptr(s.sexp_as_char(x)?)?.0 as *const c_char)
    }
    /// Read an interned string from a character vector.
    fn STRING_ELT(x: Sexp, i: R_xlen_t) -> Sexp = Sexp::NULL; |ctx| {
        let x = id(ctx, x, "STRING_ELT")?;
        require_type(ctx, x, SexpType::Str, "STRING_ELT")?;
        let i = index(ctx, i, ctx.session().xlength(x)?)?;
        Ok(Sexp::from_id(ctx.session_mut().str_get(x, i)?.as_sexp()))
    }
    /// Set an interned string in a character vector.
    fn SET_STRING_ELT(x: Sexp, i: R_xlen_t, value: Sexp) -> () = (); |ctx| {
        let x = id(ctx, x, "SET_STRING_ELT")?;
        let value = id(ctx, value, "SET_STRING_ELT")?;
        require_type(ctx, x, SexpType::Str, "SET_STRING_ELT")?;
        let ch = ctx.session().sexp_as_char(value)?;
        let i = index(ctx, i, ctx.session().xlength(x)?)?;
        Ok(ctx.session_mut().str_set(x, i, ch)?)
    }
    /// Return a read-only pointer to the elements of a character vector.
    fn STRING_PTR_RO(x: Sexp) -> *const Sexp = null(); |ctx| {
        let x = id(ctx, x, "STRING_PTR_RO")?;
        require_type(ctx, x, SexpType::Str, "STRING_PTR_RO")?;
        Ok(super::vector::read_ptr(ctx, x, SexpType::Str)?.cast())
    }
    /// Return the encoding mark of an interned string.
    fn Rf_getCharCE(x: Sexp) -> c_int = CE_NATIVE; |ctx| {
        let x = id(ctx, x, "Rf_getCharCE")?;
        let s = ctx.session();
        Ok(encoding_code(s.char_encoding(s.sexp_as_char(x)?)?))
    }
    /// Return the string bytes in the native encoding.
    fn Rf_translateChar(x: Sexp) -> *const c_char = null(); |ctx| {
        let x = id(ctx, x, "Rf_translateChar")?;
        allocating(ctx, &[x], |ctx| translate(ctx, x))
    }
    /// Return the string bytes in UTF-8.
    fn Rf_translateCharUTF8(x: Sexp) -> *const c_char = null(); |ctx| {
        let x = id(ctx, x, "Rf_translateCharUTF8")?;
        allocating(ctx, &[x], |ctx| translate(ctx, x))
    }
    /// Convert a C string between encodings.
    fn Rf_reEnc(text: *const c_char, from: c_int, to: c_int, substitute: c_int) -> *const c_char = null(); |ctx| {
        let from = encoding(ctx, from)?;
        let to = encoding(ctx, to)?;
        let bytes = c_text(ctx, text, "Rf_reEnc")?;
        reencode(ctx, text, bytes, from, to, substitute)
    }
    /// Translate an interned character string and intern it as a symbol.
    fn Rf_installTrChar(name: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        super::sym::install_char(ctx, name, "Rf_installTrChar", true)
    }
    /// Test whether the object is the missing-string sentinel.
    fn R_IsNaString(x: Sexp) -> Rboolean = 0; |ctx| {
        let x = id(ctx, x, "R_IsNaString")?;
        Ok(c_int::from(x == CharId::NA.as_sexp()))
    }

}

macro_rules! encoding_predicates {
    ($($name:ident, $test:expr);+ $(;)?) => {
        api! {$(
            /// Test the encoding of an interned string.
            fn $name(x: Sexp) -> Rboolean = 0; |ctx| {
                let x = id(ctx, x, stringify!($name))?;
                let s = ctx.session();
                Ok(c_int::from(($test)(s, s.sexp_as_char(x)?)?))
            }
        )+}
    };
}

encoding_predicates!(
    Rf_charIsASCII, |s: &roxide_value::RSession, ch| s.char_bytes(ch).map(|b| b.is_ascii());
    Rf_charIsLatin1, |s: &roxide_value::RSession, ch| s.char_encoding(ch).map(|e| e == Encoding::Latin1);
    Rf_charIsUTF8, |s: &roxide_value::RSession, ch| s.char_encoding(ch).map(|e| e == Encoding::Utf8);
);
