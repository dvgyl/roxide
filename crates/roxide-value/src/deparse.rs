//! Clean-room `deparse()` over R `SEXP` objects (ADR-0004).

use roxide_gc::SexpId;

use crate::api::{ValueApi, ValueError};
use crate::format::{FormatOpts, encode_string, format_real_scalar};
use crate::na::{NA_INTEGER, is_na_real, is_nan_not_na};
use crate::object::{CharId, SexpType};
use crate::session::RSession;

/// Controls for [`deparse`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DeparseOpts {
    /// Soft line-width cutoff in bytes (default `60`).
    pub width_cutoff: usize,
    /// Emit `L` suffix on integer constants (`keepInteger`).
    pub keep_integer: bool,
    /// Emit typed `NA_*` constants when needed (`keepNA`).
    pub keep_na: bool,
    /// Use high-precision double formatting (`digits17`, used by `dput`).
    pub digits17: bool,
}

impl Default for DeparseOpts {
    fn default() -> Self {
        Self {
            width_cutoff: 60,
            keep_integer: true,
            keep_na: true,
            digits17: false,
        }
    }
}

/// Deparse an R object into lines of valid R source text.
pub fn deparse(
    session: &RSession,
    x: SexpId,
    opts: DeparseOpts,
) -> Result<Vec<String>, ValueError> {
    let text = deparse_one(session, x, opts)?;
    Ok(vec![text])
}

fn deparse_one(session: &RSession, x: SexpId, opts: DeparseOpts) -> Result<String, ValueError> {
    let ty = session.type_of(x)?;
    let fmt_opts = FormatOpts {
        digits: if opts.digits17 { 17 } else { 15 },
        scipen: 0,
    };
    match ty {
        SexpType::Nil => Ok(String::from("NULL")),
        SexpType::Lgl => {
            let v = session.lgl(x)?;
            if v.is_empty() {
                return Ok(String::from("logical(0)"));
            }
            let parts: Vec<&str> = v
                .iter()
                .map(|b| match b.get() {
                    Some(true) => "TRUE",
                    Some(false) => "FALSE",
                    None => "NA",
                })
                .collect();
            if parts.len() == 1 {
                Ok(parts[0].to_owned())
            } else {
                Ok(format!("c({})", parts.join(", ")))
            }
        }
        SexpType::Int => {
            let v = session.int(x)?;
            if v.is_empty() {
                return Ok(String::from("integer(0)"));
            }
            let parts: Vec<String> = v
                .iter()
                .map(|n| {
                    if n == NA_INTEGER {
                        if opts.keep_na && v.len() == 1 {
                            String::from("NA_integer_")
                        } else {
                            String::from("NA")
                        }
                    } else if opts.keep_integer {
                        format!("{n}L")
                    } else {
                        n.to_string()
                    }
                })
                .collect();
            if parts.len() == 1 {
                Ok(parts[0].clone())
            } else {
                Ok(format!("c({})", parts.join(", ")))
            }
        }
        SexpType::Real => {
            let v = session.real(x)?;
            if v.is_empty() {
                return Ok(String::from("numeric(0)"));
            }
            let parts: Vec<String> = v
                .iter()
                .map(|f| {
                    if is_na_real(f) {
                        if opts.keep_na && v.len() == 1 {
                            String::from("NA_real_")
                        } else {
                            String::from("NA")
                        }
                    } else if is_nan_not_na(f) {
                        String::from("NaN")
                    } else {
                        format_real_scalar(f, fmt_opts)
                    }
                })
                .collect();
            if parts.len() == 1 {
                Ok(parts[0].clone())
            } else {
                Ok(format!("c({})", parts.join(", ")))
            }
        }
        SexpType::Str => {
            let slice = session.str_slice(x)?;
            if slice.is_empty() {
                return Ok(String::from("character(0)"));
            }
            let mut parts = Vec::with_capacity(slice.len());
            for &cid in slice {
                if cid == CharId::NA {
                    if opts.keep_na && slice.len() == 1 {
                        parts.push(String::from("NA_character_"));
                    } else {
                        parts.push(String::from("NA"));
                    }
                } else {
                    parts.push(encode_string(session.char_bytes(cid)?));
                }
            }
            if parts.len() == 1 {
                Ok(parts[0].clone())
            } else {
                Ok(format!("c({})", parts.join(", ")))
            }
        }
        SexpType::Sym => {
            let sym = crate::object::Sym(x);
            Ok(session.sym_name(sym)?.to_owned())
        }
        _ => Ok(format!("<{}>", ty.type_name(session.is_s4(x)?))),
    }
}
