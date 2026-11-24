//! Conversion between atomic vectors and lists.

use roxide_gc::{HeapError, SexpId};

use crate::api::{CoerceWarn, ValueApi, ValueError};
use crate::deparse::{DeparseOpts, deparse, deparse_number};
use crate::na::{NA_INTEGER, NA_REAL, RComplex, RLogical, is_na_real};
use crate::object::{CharId, Encoding, SexpType};
use crate::session::{RSession, SEXP_NIL};

#[derive(Clone, Copy)]
enum Scalar {
    Missing,
    Raw(u8),
    Lgl(RLogical),
    Int(i32),
    Real(f64),
    Cplx(RComplex),
    Str(CharId),
}

pub(crate) fn coerce(
    session: &mut RSession,
    x: SexpId,
    to: SexpType,
) -> Result<(SexpId, CoerceWarn), ValueError> {
    use SexpType::{Cplx, Int, Lang, Lgl, List, Nil, Raw, Real, Str, Sym, Vec};

    let from = session.type_of(x)?;
    let mut warn = CoerceWarn::default();
    if from == to || to == Nil {
        return Ok((if to == Nil { SEXP_NIL } else { x }, warn));
    }
    let mismatch = ValueError::TypeMismatch {
        expected: to.type_name(false),
        actual: from.type_name(false),
    };
    if from == Sym && to == Str {
        let name = session.sym_name_of(x)?.to_owned();
        let id = store_string(session, &name)?;
        return Ok((session.alloc_str_from(&[id])?, warn));
    }
    if matches!(from, List | Lang) && to == Str {
        let items: std::vec::Vec<SexpId> = session.pairlist_iter(x).map(|(_, v)| v).collect();
        let mut out = std::vec::Vec::with_capacity(items.len());
        for item in items {
            let ty = session.type_of(item)?;
            let value = if ty == Str && session.xlength(item)? == 1 {
                session.str_slice(item)?[0]
            } else {
                let opts = DeparseOpts {
                    keep_integer: false,
                    keep_na: false,
                    width_cutoff: usize::MAX,
                    ..DeparseOpts::default()
                };
                let text = deparse(session, item, opts)?.join(" ");
                store_string(session, &text)?
            };
            out.push(value);
        }
        return Ok((session.alloc_str_from(&out)?, warn));
    }
    if !matches!(from, Nil | Raw | Lgl | Int | Real | Cplx | Str | Vec)
        || !matches!(to, Raw | Lgl | Int | Real | Cplx | Str | Vec)
    {
        return Err(mismatch);
    }

    let len = session.xlength(x)?;
    if to == Vec {
        let names = session.intern("names")?;
        let result = session.alloc_list(len)?;
        for i in 0..len {
            let item = scalar_at(session, x, from, i)?.allocate(session)?;
            session.list_set(result, i, item)?;
        }
        session.set_attrib(result, names, session.attr(x, names))?;
        return Ok((result, warn));
    }

    let mut scalars = std::vec::Vec::new();
    scalars
        .try_reserve_exact(len)
        .map_err(|_| HeapError::OutOfMemory {
            requested: len.saturating_mul(size_of::<Scalar>()),
        })?;
    if from == Vec {
        for i in 0..len {
            let item = session.list_get(x, i)?;
            let ty = session.type_of(item)?;
            if to == Str {
                let value = if ty == Str && session.xlength(item)? == 1 {
                    session.str_slice(item)?[0]
                } else {
                    let opts = DeparseOpts {
                        keep_integer: false,
                        keep_na: false,
                        width_cutoff: usize::MAX,
                        ..DeparseOpts::default()
                    };
                    let text = deparse(session, item, opts)?.join(" ");
                    store_string(session, &text)?
                };
                scalars.push(Scalar::Str(value));
                continue;
            }
            let n = session.xlength(item)?;
            if n > 1 || ty == Nil {
                return Err(mismatch);
            }
            if n == 1 && matches!(ty, Raw | Lgl | Int | Real | Cplx | Str) {
                scalars.push(scalar_at(session, item, ty, 0)?);
            } else {
                scalars.push(Scalar::Missing);
            }
        }
    } else {
        for i in 0..len {
            scalars.push(scalar_at(session, x, from, i)?);
        }
    }

    macro_rules! convert {
        ($alloc:ident, $convert:expr) => {{
            let values = scalars
                .into_iter()
                .map($convert)
                .collect::<Result<std::vec::Vec<_>, ValueError>>()?;
            session.$alloc(&values)?
        }};
    }
    let result = match to {
        Lgl => convert!(alloc_lgl_from, |s: Scalar| s.logical(session, &mut warn)),
        Int => convert!(alloc_int_from, |s: Scalar| s.integer(session, &mut warn)),
        Real => convert!(alloc_real_from, |s: Scalar| s.real(session, &mut warn)),
        Cplx => convert!(alloc_cplx_from, |s: Scalar| s.complex(session, &mut warn)),
        Str => convert!(alloc_str_from, |s: Scalar| s.character(session)),
        Raw => convert!(alloc_raw_from, |s: Scalar| {
            Ok(u8::try_from(s.integer(session, &mut warn)?).unwrap_or(0))
        }),
        _ => return Err(mismatch),
    };
    Ok((result, warn))
}

fn scalar_at(session: &RSession, x: SexpId, ty: SexpType, i: usize) -> Result<Scalar, ValueError> {
    Ok(match ty {
        SexpType::Raw => Scalar::Raw(session.raw(x)?.get(i)),
        SexpType::Lgl => Scalar::Lgl(session.lgl(x)?.get(i)),
        SexpType::Int => Scalar::Int(session.int(x)?.get(i)),
        SexpType::Real => Scalar::Real(session.real(x)?.get(i)),
        SexpType::Cplx => Scalar::Cplx(session.cplx(x)?.get(i)),
        SexpType::Str => Scalar::Str(session.str_slice(x)?[i]),
        _ => Scalar::Missing,
    })
}

impl Scalar {
    fn allocate(self, session: &mut RSession) -> Result<SexpId, ValueError> {
        match self {
            Self::Missing => Ok(SEXP_NIL),
            Self::Raw(x) => session.alloc_raw_from(&[x]),
            Self::Lgl(x) => session.scalar_lgl(x),
            Self::Int(x) => session.scalar_int(x),
            Self::Real(x) => session.scalar_real(x),
            Self::Cplx(x) => session.alloc_cplx_from(&[x]),
            Self::Str(x) => session.alloc_str_from(&[x]),
        }
    }

    fn logical(self, session: &RSession, warn: &mut CoerceWarn) -> Result<RLogical, ValueError> {
        let result = match self {
            Self::Str(CharId::NA) | Self::Missing => None,
            Self::Str(id) => match session.char_bytes(id)? {
                b"T" | b"TRUE" | b"True" | b"true" => Some(true),
                b"F" | b"FALSE" | b"False" | b"false" => Some(false),
                _ => {
                    warn.na_introduced = true;
                    None
                }
            },
            Self::Cplx(z) => (!z.is_na()).then_some(z.re != 0.0 || z.im != 0.0),
            _ => {
                let x = self.real(session, warn)?;
                (!x.is_nan()).then_some(x != 0.0)
            }
        };
        Ok(RLogical::from_option(result))
    }

    fn integer(self, session: &RSession, warn: &mut CoerceWarn) -> Result<i32, ValueError> {
        if let Self::Int(x) = self {
            return Ok(x);
        }
        let x = self.real(session, warn)?;
        if is_na_real(x) {
            return Ok(NA_INTEGER);
        }
        if matches!(self, Self::Str(_)) && x.is_nan() {
            return Ok(0);
        }
        let truncated = x.trunc();
        if !(-f64::from(i32::MAX)..=f64::from(i32::MAX)).contains(&truncated) {
            warn.na_introduced = true;
            return Ok(NA_INTEGER);
        }
        Ok(truncated as i32)
    }

    fn real(self, session: &RSession, warn: &mut CoerceWarn) -> Result<f64, ValueError> {
        Ok(match self {
            Self::Missing | Self::Int(NA_INTEGER) | Self::Str(CharId::NA) => NA_REAL,
            Self::Raw(x) => f64::from(x),
            Self::Lgl(x) => x.get().map_or(NA_REAL, |b| f64::from(u8::from(b))),
            Self::Int(x) => f64::from(x),
            Self::Real(x) => x,
            Self::Cplx(z) if z.is_na() => NA_REAL,
            Self::Cplx(z) => {
                warn.imaginary_discarded |= z.im != 0.0;
                z.re
            }
            Self::Str(id) => {
                let bytes = session.char_bytes(id)?;
                let text = std::str::from_utf8(bytes).unwrap_or("\u{FFFD}");
                parse_real(text.trim_ascii()).unwrap_or_else(|| {
                    warn.na_introduced |= !text.trim_ascii().is_empty();
                    NA_REAL
                })
            }
        })
    }

    fn complex(self, session: &RSession, warn: &mut CoerceWarn) -> Result<RComplex, ValueError> {
        Ok(match self {
            Self::Missing | Self::Str(CharId::NA) => RComplex::NA,
            Self::Cplx(z) => z,
            Self::Str(id) => {
                let bytes = session.char_bytes(id)?;
                let text = std::str::from_utf8(bytes)
                    .unwrap_or("\u{FFFD}")
                    .trim_ascii();
                parse_complex(text).unwrap_or_else(|| {
                    warn.na_introduced |= !text.is_empty();
                    RComplex::NA
                })
            }
            _ => RComplex {
                re: self.real(session, warn)?,
                im: 0.0,
            },
        })
    }

    fn character(self, session: &mut RSession) -> Result<CharId, ValueError> {
        let text = match self {
            Self::Missing | Self::Int(NA_INTEGER) => return Ok(CharId::NA),
            Self::Str(id) => return Ok(id),
            Self::Lgl(x) => match x.get() {
                Some(true) => "TRUE".into(),
                Some(false) => "FALSE".into(),
                None => return Ok(CharId::NA),
            },
            Self::Int(x) => x.to_string(),
            Self::Raw(x) => format!("{x:02x}"),
            Self::Real(x) if is_na_real(x) => return Ok(CharId::NA),
            Self::Real(x) => character_real(x),
            Self::Cplx(z) if is_na_real(z.re) || is_na_real(z.im) => return Ok(CharId::NA),
            Self::Cplx(z) => {
                let sign = if z.im < 0.0 { "-" } else { "+" };
                format!(
                    "{}{sign}{}i",
                    character_real(z.re),
                    character_real(z.im.abs())
                )
            }
        };
        store_string(session, &text)
    }
}

fn store_string(session: &mut RSession, text: &str) -> Result<CharId, ValueError> {
    session.mk_char(text.as_bytes(), Encoding::Utf8)
}

fn character_real(x: f64) -> String {
    deparse_number(x, 15)
}

fn parse_real(text: &str) -> Option<f64> {
    let (sign, unsigned) = match text.as_bytes().first() {
        Some(b'-') => (-1.0, &text[1..]),
        Some(b'+') => (1.0, &text[1..]),
        _ => (1.0, text),
    };
    if let Some(hex) = unsigned
        .strip_prefix("0x")
        .or_else(|| unsigned.strip_prefix("0X"))
    {
        return parse_hex(hex).map(|x| sign * x);
    }
    text.parse().ok()
}

fn parse_hex(text: &str) -> Option<f64> {
    let (mantissa, exponent) = match text.split_once(['p', 'P']) {
        Some((m, e)) => (m, e.parse::<i32>().ok()?),
        None => (text, 0),
    };
    let mut saw_digit = false;
    let mut value = 0.0;
    let mut fractional = false;
    let mut place = 1.0 / 16.0;
    for ch in mantissa.chars() {
        if ch == '.' && !fractional {
            fractional = true;
            continue;
        }
        let digit = f64::from(ch.to_digit(16)?);
        saw_digit = true;
        if fractional {
            value += digit * place;
            place /= 16.0;
        } else {
            value = value * 16.0 + digit;
        }
    }
    saw_digit.then(|| {
        if value == 0.0 {
            0.0
        } else {
            value * 2.0_f64.powi(exponent)
        }
    })
}

fn parse_complex(text: &str) -> Option<RComplex> {
    if let Some(re) = parse_real(text) {
        return Some(RComplex { re, im: 0.0 });
    }
    let parts = text.strip_suffix('i')?;
    if parts.is_empty() {
        return Some(RComplex {
            re: 0.0,
            im: NA_REAL,
        });
    }
    if let Some(im) = parse_real(parts) {
        return Some(RComplex { re: 0.0, im });
    }
    let unsigned = parts.trim_start_matches(['+', '-']);
    let exponent = if unsigned.starts_with("0x") || unsigned.starts_with("0X") {
        b"pP"
    } else {
        b"eE"
    };
    let bytes = parts.as_bytes();
    let split = (1..bytes.len())
        .find(|&i| matches!(bytes[i], b'+' | b'-') && !exponent.contains(&bytes[i - 1]))?;
    Some(RComplex {
        re: parse_real(&parts[..split])?,
        im: parse_real(&parts[split..])?,
    })
}
