//! Character string interning, encoding, UTF-8 translation, and `STRING_ELT`.

use std::borrow::Cow;
use std::ffi::{CStr, c_char, c_int, c_uint};
use std::io::Write;
use std::ptr::null;

use roxide_eval::{Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::na::NA_REAL_QUIET_BITS;
use roxide_value::{CharId, Encoding, SEXP_NIL, SexpType, ValueApi};

use super::memory::transient_string;
use super::vector::{allocating, api, check_capacity, checked_len, id, index, require_type};
use crate::abi::{R_xlen_t, Rboolean, Sexp};

const CE_NATIVE: c_int = 0;
const CE_UTF8: c_int = 1;
const CE_LATIN1: c_int = 2;
const CE_BYTES: c_int = 3;

/// Encode a code point as GNU R does, with up to six bytes.
///
/// Return the number of encoded bytes. A nonzero code point adds a NUL
/// terminator. A zero code point writes a single NUL byte and returns one.
///
/// # Safety
/// `s` must have space for the encoded bytes and any terminator. Seven
/// writable bytes are sufficient for all inputs.
#[allow(non_snake_case)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_ucstoutf8(s: *mut c_char, c: c_uint) -> usize {
    let mut code = if c > 0x7fff_ffff { 0xfffd } else { c };
    let (len, lead) = match code {
        0..=0x7f => (1, 0x00),
        0x80..=0x7ff => (2, 0xc0),
        0x800..=0xffff => (3, 0xe0),
        0x1_0000..=0x1f_ffff => (4, 0xf0),
        0x20_0000..=0x3ff_ffff => (5, 0xf8),
        _ => (6, 0xfc),
    };
    let mut bytes = [0u8; 7];
    for byte in bytes[1..len].iter_mut().rev() {
        *byte = 0x80 | (code as u8 & 0x3f);
        code >>= 6;
    }
    bytes[0] = lead | code as u8;
    let written = len + usize::from(c != 0);
    // SAFETY: The caller supplies a writable buffer for these bytes.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), s.cast(), written) };
    len
}

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

#[derive(Clone, Copy, PartialEq, Eq)]
enum NativeEncoding {
    Utf8,
    Latin1,
    Ascii,
}

impl NativeEncoding {
    fn current(ctx: &mut Ctx) -> Self {
        let locale = ctx.host().locale();
        if locale.utf8 {
            return Self::Utf8;
        }
        let ctype = locale.ctype.to_ascii_uppercase();
        let codeset = ctype.rsplit('.').next().unwrap_or(&ctype);
        let codeset = codeset.split('@').next().unwrap_or(codeset);
        if matches!(codeset, "ISO8859-1" | "ISO-8859-1" | "LATIN1") {
            Self::Latin1
        } else {
            Self::Ascii
        }
    }
}

fn escape_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(bytes.len());
    for &byte in bytes {
        if byte.is_ascii() {
            result.push(byte);
        } else {
            let _ = write!(result, "<{byte:02x}>");
        }
    }
    result
}

fn utf8_to_native(bytes: &[u8], latin1: bool) -> Vec<u8> {
    let mut result = Vec::with_capacity(bytes.len());
    for chunk in bytes.utf8_chunks() {
        for ch in chunk.valid().chars() {
            let code = u32::from(ch);
            if code <= if latin1 { 255 } else { 127 } {
                result.push(code as u8);
            } else {
                let width = if code <= 0xffff { 4 } else { 8 };
                let _ = write!(result, "<U+{code:0width$X}>");
            }
        }
        result.extend(escape_bytes(chunk.invalid()));
    }
    result
}

fn translation(
    bytes: &[u8],
    from: Encoding,
    target_utf8: bool,
    native: NativeEncoding,
) -> Cow<'_, [u8]> {
    use Encoding::{Latin1, Native, Utf8};

    if bytes.is_ascii() {
        return Cow::Borrowed(bytes);
    }
    match (target_utf8, native, from) {
        (true, _, Latin1)
        | (true, NativeEncoding::Latin1, Native)
        | (false, NativeEncoding::Utf8, Latin1) => utf8(bytes, Latin1),
        (true, NativeEncoding::Ascii, Native) | (false, NativeEncoding::Ascii, Latin1) => {
            Cow::Owned(escape_bytes(bytes))
        }
        (false, NativeEncoding::Ascii, Utf8) => Cow::Owned(utf8_to_native(bytes, false)),
        (false, NativeEncoding::Latin1, Utf8) => Cow::Owned(utf8_to_native(bytes, true)),
        _ => Cow::Borrowed(bytes),
    }
}

const BYTES_TRANSLATION_ERROR: &str = "translating strings with \"bytes\" encoding is not allowed";

fn translate(ctx: &mut Ctx, x: SexpId, target_utf8: bool) -> EvalResult<*const c_char> {
    let native = NativeEncoding::current(ctx);
    let s = ctx.session();
    let ch = s.sexp_as_char(x)?;
    let enc = s.char_encoding(ch)?;
    if enc == Encoding::Bytes {
        return Err(ctx.stop(SEXP_NIL, BYTES_TRANSLATION_ERROR));
    }
    match translation(s.char_bytes(ch)?, enc, target_utf8, native) {
        Cow::Borrowed(_) => Ok(s.char_cstr_ptr(ch)?.0 as *const c_char),
        Cow::Owned(bytes) => transient_string(ctx, &bytes),
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
            Cow::Owned(bytes) => transient_string(ctx, &bytes),
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
    transient_string(ctx, &out)
}

fn partial_match(text: &[u8], pattern: &[u8], exact: Rboolean) -> Rboolean {
    c_int::from(if exact == 0 {
        text.starts_with(pattern)
    } else {
        text == pattern
    })
}

fn match_char(ctx: &mut Ctx, x: SexpId) -> EvalResult<CharId> {
    match ctx.session().type_of(x)? {
        SexpType::Char => Ok(ctx.session().sexp_as_char(x)?),
        SexpType::Str if ctx.session().xlength(x)? == 1 => Ok(ctx.session_mut().str_get(x, 0)?),
        SexpType::Sym => super::sym::print_name(ctx, x),
        _ => Err(ctx.stop(SEXP_NIL, "invalid partial string match")),
    }
}

fn c_whitespace(byte: u8) -> bool {
    byte.is_ascii_whitespace() || byte == b'\x0b'
}

fn digit_count(text: &[u8], hex: bool) -> usize {
    text.iter()
        .take_while(|byte| {
            if hex {
                byte.is_ascii_hexdigit()
            } else {
                byte.is_ascii_digit()
            }
        })
        .count()
}

fn exponent(text: &[u8], marker: u8) -> Option<(i64, usize)> {
    if !text
        .first()
        .is_some_and(|byte| byte.eq_ignore_ascii_case(&marker))
    {
        return Some((0, 0));
    }
    let text = &text[1..];
    let start = usize::from(matches!(text.first(), Some(b'+' | b'-')));
    let digits = &text[start..];
    let count = digit_count(digits, false);
    if count == 0 {
        return None;
    }
    let value = digits[..count].iter().fold(0i64, |value, &byte| {
        value
            .saturating_mul(10)
            .saturating_add(i64::from(byte - b'0'))
    });
    let sign = if text[0] == b'-' { -1 } else { 1 };
    Some((sign * value, 1 + start + count))
}

fn hex_real(whole: &[u8], fraction: &[u8], binary_exponent: i64) -> f64 {
    let mut significand = 0u64;
    let mut stored = 0;
    let mut omitted = 0i64;
    let mut sticky = false;
    for &byte in whole
        .iter()
        .chain(fraction)
        .skip_while(|&&byte| byte == b'0')
    {
        let digit = match byte {
            b'0'..=b'9' => byte - b'0',
            b'A'..=b'F' => byte - b'A' + 10,
            _ => byte - b'a' + 10,
        };
        if stored < 16 {
            significand = (significand << 4) | u64::from(digit);
            stored += 1;
        } else {
            omitted += 1;
            sticky |= digit != 0;
        }
    }
    if significand == 0 {
        return 0.0;
    }
    let power = binary_exponent.saturating_add((omitted - fraction.len() as i64).saturating_mul(4));
    let top_bit = i64::from(significand.ilog2());
    let unbiased_exponent = power.saturating_add(top_bit);
    if unbiased_exponent > 1023 {
        return f64::INFINITY;
    }
    let target_power = unbiased_exponent.max(-1022) - 52;
    let shift = target_power.saturating_sub(power);
    let rounded = match shift {
        65.. => 0,
        64 => u64::from(significand > 1 << 63 || (significand == 1 << 63 && sticky)),
        1..=63 => {
            let high = significand >> shift;
            let low = significand & ((1 << shift) - 1);
            let half = 1 << (shift - 1);
            high + u64::from(low > half || (low == half && (sticky || high & 1 != 0)))
        }
        _ => significand << (-shift),
    };
    let unit = if target_power < -1022 {
        f64::from_bits(1 << (target_power + 1074))
    } else {
        f64::from_bits(((target_power + 1023) as u64) << 52)
    };
    rounded as f64 * unit
}

fn parse_c_real(text: &[u8]) -> Option<(f64, usize)> {
    let start = text.iter().position(|&byte| !c_whitespace(byte))?;
    let negative = text[start] == b'-';
    let sign = usize::from(matches!(text[start], b'+' | b'-'));
    let body_start = start + sign;
    let body = &text[body_start..];
    for (name, value) in [
        (b"infinity".as_slice(), f64::INFINITY),
        (b"inf".as_slice(), f64::INFINITY),
        (b"nan".as_slice(), f64::NAN),
    ] {
        if body
            .get(..name.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(name))
        {
            let value = if negative && value.is_infinite() {
                -value
            } else {
                value
            };
            return Some((value, body_start + name.len()));
        }
    }
    let hex = body.len() > 2 && (body.starts_with(b"0x") || body.starts_with(b"0X"));
    let prefix = if hex { 2 } else { 0 };
    let body = &body[prefix..];
    let whole = digit_count(body, hex);
    let dot = usize::from(body.get(whole) == Some(&b'.'));
    let fraction_start = whole + dot;
    let fraction = if dot == 0 {
        0
    } else {
        digit_count(&body[fraction_start..], hex)
    };
    let mantissa_end = fraction_start + fraction;
    if !hex && whole + fraction == 0 {
        return None;
    }
    let (power, exponent_end) = exponent(&body[mantissa_end..], if hex { b'p' } else { b'e' })?;
    let end = body_start + prefix + mantissa_end + exponent_end;
    let value = if hex {
        let magnitude = hex_real(&body[..whole], &body[fraction_start..mantissa_end], power);
        if negative { -magnitude } else { magnitude }
    } else {
        std::str::from_utf8(&text[start..end]).ok()?.parse().ok()?
    };
    Some((value, end))
}

/// Parse a C string as an R real number and report where conversion stopped.
///
/// # Safety
/// `text` must point to a readable C string. If `end` is nonnull, it must be writable.
#[allow(non_snake_case)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_strtod(text: *const c_char, end: *mut *mut c_char) -> f64 {
    let parsed = if text.is_null() {
        None
    } else {
        // SAFETY: The caller supplies a readable NUL-terminated string.
        parse_c_real(unsafe { CStr::from_ptr(text) }.to_bytes())
    };
    let (value, consumed) = parsed.unwrap_or((f64::from_bits(NA_REAL_QUIET_BITS), 0));
    if !end.is_null() {
        // SAFETY: The reported offset is within the string, and end is writable.
        unsafe { end.write(text.wrapping_add(consumed).cast_mut()) };
    }
    value
}

/// Parse a C string as an R real number.
///
/// # Safety
/// `text` must point to a readable C string.
#[allow(non_snake_case)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn R_atof(text: *const c_char) -> f64 {
    // SAFETY: The caller supplies the C string. R_strtod accepts a NULL end pointer.
    unsafe { R_strtod(text, std::ptr::null_mut()) }
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
        Ok(Sexp::from_id(super::altrep::string_elt(ctx, x, i, "STRING_ELT")?))
    }
    /// Set an interned string in a character vector.
    fn SET_STRING_ELT(x: Sexp, i: R_xlen_t, value: Sexp) -> () = (); |ctx| {
        let x = id(ctx, x, "SET_STRING_ELT")?;
        let value = id(ctx, value, "SET_STRING_ELT")?;
        require_type(ctx, x, SexpType::Str, "SET_STRING_ELT")?;
        let ch = ctx.session().sexp_as_char(value)?;
        let i = index(ctx, i, ctx.session().xlength(x)?)?;
        if super::altrep::c_set_elt(ctx, x, SexpType::Str, i, value)? { return Ok(()); }
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
        allocating(ctx, &[x], |ctx| translate(ctx, x, false))
    }
    /// Return the string bytes in UTF-8.
    fn Rf_translateCharUTF8(x: Sexp) -> *const c_char = null(); |ctx| {
        let x = id(ctx, x, "Rf_translateCharUTF8")?;
        allocating(ctx, &[x], |ctx| translate(ctx, x, true))
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

    /// Test whether a C string is empty or consists of whitespace.
    fn Rf_isBlankString(x: *const c_char) -> Rboolean = 0; |ctx| {
        let text = c_text(ctx, x, "Rf_isBlankString")?;
        Ok(c_int::from(text.iter().copied().all(c_whitespace)))
    }

    /// Test whether the object is R NULL, an empty string, or a scalar missing string.
    fn Rf_StringBlank(x: Sexp) -> Rboolean = 0; |ctx| {
        let x = id(ctx, x, "Rf_StringBlank")?;
        let s = ctx.session();
        let blank = match s.type_of(x)? {
            SexpType::Nil => true,
            SexpType::Char => s.char_bytes(s.sexp_as_char(x)?)?.is_empty(),
            SexpType::Str if s.xlength(x)? == 1 => {
                let ch = ctx.session_mut().str_get(x, 0)?;
                ch == CharId::NA || ctx.session().char_bytes(ch)?.is_empty()
            }
            _ => false,
        };
        Ok(c_int::from(blank))
    }

    /// Test whether a C string is one of R's spellings of logical false.
    fn Rf_StringFalse(x: *const c_char) -> Rboolean = 0; |ctx| {
        let text = c_text(ctx, x, "Rf_StringFalse")?;
        Ok(c_int::from(matches!(text, b"F" | b"FALSE" | b"False" | b"false")))
    }

    /// Test whether a C string is one of R's spellings of logical true.
    fn Rf_StringTrue(x: *const c_char) -> Rboolean = 0; |ctx| {
        let text = c_text(ctx, x, "Rf_StringTrue")?;
        Ok(c_int::from(matches!(text, b"T" | b"TRUE" | b"True" | b"true")))
    }

    /// Match the second object against the first as a name or a prefix.
    fn Rf_pmatch(x: Sexp, table: Sexp, exact: Rboolean) -> Rboolean = 0; |ctx| {
        let x = id(ctx, x, "Rf_pmatch")?;
        let table = id(ctx, table, "Rf_pmatch")?;
        allocating(ctx, &[x, table], |ctx| {
            let text = match_char(ctx, x)?;
            ctx.protect(text.as_sexp());
            let pattern = match_char(ctx, table)?;
            if text == pattern {
                return Ok(1);
            }
            let native = NativeEncoding::current(ctx);
            let s = ctx.session();
            let text_encoding = s.char_encoding(text)?;
            let pattern_encoding = s.char_encoding(pattern)?;
            if text_encoding == Encoding::Bytes || pattern_encoding == Encoding::Bytes {
                return Err(ctx.stop(SEXP_NIL, BYTES_TRANSLATION_ERROR));
            }
            let text = translation(s.char_bytes(text)?, text_encoding, true, native);
            let pattern = translation(s.char_bytes(pattern)?, pattern_encoding, true, native);
            Ok(partial_match(&text, &pattern, exact))
        })
    }

    /// Match the second C string against the first as a string or a prefix.
    fn Rf_psmatch(x: *const c_char, table: *const c_char, exact: Rboolean) -> Rboolean = 0; |ctx| {
        let text = c_text(ctx, x, "Rf_psmatch")?;
        let pattern = c_text(ctx, table, "Rf_psmatch")?;
        Ok(partial_match(text, pattern, exact))
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

#[cfg(test)]
mod tests {
    use super::{Encoding, NativeEncoding, translation};

    #[test]
    fn native_and_utf8_translation_use_the_requested_encoding() {
        let ascii = NativeEncoding::Ascii;
        let latin1 = NativeEncoding::Latin1;
        let bytes = b"\xc3\xa9\xf0\x9f\x98\x83";
        assert_eq!(
            translation(bytes, Encoding::Utf8, true, ascii).as_ref(),
            bytes
        );
        assert_eq!(
            translation(bytes, Encoding::Utf8, false, ascii).as_ref(),
            b"<U+00E9><U+0001F603>"
        );
        assert_eq!(
            translation(bytes, Encoding::Utf8, false, latin1).as_ref(),
            b"\xe9<U+0001F603>"
        );
        assert_eq!(
            translation(b"\xe9", Encoding::Latin1, false, ascii).as_ref(),
            b"<e9>"
        );
        assert_eq!(
            translation(b"\xe9", Encoding::Native, true, latin1).as_ref(),
            b"\xc3\xa9"
        );
    }
}
