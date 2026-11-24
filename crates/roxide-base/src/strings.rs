#![forbid(unsafe_code)]

//! Base string operations, formatting, encodings, and regular expressions.

mod format;
mod patterns;
mod text;

use std::borrow::Cow;

use roxide_eval::{
    ArgEval, Arity::Any, Dispatch, EvalResult, Module, PrimDef, PrimKind, Visibility,
};
use roxide_gc::SexpId;
use roxide_value::{CharId, Encoding, RSession, SexpType, ValueApi, ValueError};

use crate::ops::{
    attributes::has_class,
    common::{attr, factor_labels},
    primitive,
};

/// Primitives exported by the base string module.
pub const MODULE: Module = Module {
    prims: &[
        primitive!("paste", text::paste),
        primitive!("paste0", text::paste0),
        primitive!("sprintf", format::sprintf),
        primitive!("nchar", text::nchar),
        primitive!("nzchar", text::nzchar),
        primitive!("substr", text::substr),
        primitive!("substring", text::substring),
        primitive!("startsWith", text::starts_with),
        primitive!("endsWith", text::ends_with),
        primitive!("tolower", text::tolower),
        primitive!("toupper", text::toupper),
        primitive!("chartr", text::chartr),
        primitive!("trimws", patterns::trimws),
        primitive!("strsplit", patterns::strsplit),
        primitive!("grep", patterns::grep),
        primitive!("grepv", patterns::grepv),
        primitive!("grepl", patterns::grepl),
        primitive!("sub", patterns::sub),
        primitive!("gsub", patterns::gsub),
        primitive!("regexpr", patterns::regexpr),
        primitive!("gregexpr", patterns::gregexpr),
        primitive!("regmatches", patterns::regmatches),
        primitive!("Encoding", text::encoding),
        primitive!("Encoding<-", text::set_encoding),
        primitive!("enc2utf8", text::transcode),
        primitive!("enc2native", text::transcode),
        primitive!("charToRaw", text::char_to_raw),
        primitive!("rawToChar", text::raw_to_char),
    ],
    prelude: "",
};

fn character(s: &mut RSession, x: SexpId) -> EvalResult<Vec<CharId>> {
    if has_class(s, x, "factor") {
        let levels = s.str_slice(attr(s, x, "levels"))?;
        return Ok(factor_labels(s, x, levels)?);
    }
    let (x, _) = s.coerce(x, SexpType::Str)?;
    Ok(s.str_slice(x)?.to_vec())
}

pub(crate) fn utf8(s: &RSession, id: CharId) -> Result<Cow<'_, [u8]>, ValueError> {
    let bytes = s.char_bytes(id)?;
    if s.char_encoding(id)? == Encoding::Latin1 {
        return Ok(Cow::Owned(
            bytes
                .iter()
                .map(|&b| char::from(b))
                .collect::<String>()
                .into_bytes(),
        ));
    }
    Ok(Cow::Borrowed(bytes))
}

fn recycle<T: Copy>(items: &[T], i: usize) -> Option<T> {
    i.checked_rem(items.len()).map(|at| items[at])
}

fn text_attrs(name: &str) -> bool {
    matches!(name, "names" | "dim" | "dimnames")
}
