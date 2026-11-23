use std::io::{self, BufRead, Read};

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{
    Encoding, NA_INTEGER, RComplex, RLogical, SEXP_NIL, SexpType, ValueApi, is_na_real,
};

use super::connection::{self, Access};
use super::{ByteEncoding, check_limit, flag, integer, strings, text, translated_bytes};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Integer,
    Logical,
    Double,
    Complex,
    Raw,
    Character,
}

impl Kind {
    fn from_type(ty: SexpType) -> Option<Self> {
        Some(match ty {
            SexpType::Int => Self::Integer,
            SexpType::Lgl => Self::Logical,
            SexpType::Real => Self::Double,
            SexpType::Cplx => Self::Complex,
            SexpType::Raw => Self::Raw,
            SexpType::Str => Self::Character,
            _ => return None,
        })
    }

    fn size(self, ctx: &mut Ctx, call: &Call, value: SexpId) -> EvalResult<usize> {
        let value = integer(ctx, value)?;
        let size = match (self, value) {
            (Self::Raw | Self::Character, _) => 1,
            (Self::Integer | Self::Logical, NA_INTEGER) => 4,
            (Self::Double, NA_INTEGER) => 8,
            (Self::Complex, NA_INTEGER | 16) => 16,
            (Self::Complex, _) => {
                return Err(ctx.stop(
                    call.err_call(),
                    "size changing is not supported for complex vectors",
                ));
            }
            (Self::Integer | Self::Logical, 1 | 2 | 4 | 8) | (Self::Double, 4 | 8) => {
                value as usize
            }
            _ => {
                return Err(ctx.stop(
                    call.err_call(),
                    format!("size {value} is unknown on this machine"),
                ));
            }
        };
        Ok(size)
    }

    fn allocation_size(self) -> usize {
        match self {
            Self::Integer | Self::Logical => 4,
            Self::Double => 8,
            Self::Complex => 16,
            Self::Raw => 1,
            Self::Character => size_of::<Vec<u8>>(),
        }
    }
}

#[derive(Clone, Copy)]
enum Endian {
    Little,
    Big,
}

impl Endian {
    fn parse(ctx: &mut Ctx, call: &Call, value: SexpId) -> EvalResult<Self> {
        match text(ctx, call, value, "endian")?.as_str() {
            "little" | "l" => Ok(Self::Little),
            "big" | "b" => Ok(Self::Big),
            "platform" | "p" => Ok(if cfg!(target_endian = "little") {
                Self::Little
            } else {
                Self::Big
            }),
            _ => Err(ctx.stop(call.err_call(), "invalid 'endian' argument")),
        }
    }

    fn push(self, bytes: &mut Vec<u8>, bits: u64, size: usize) {
        match self {
            Self::Little => bytes.extend_from_slice(&bits.to_le_bytes()[..size]),
            Self::Big => bytes.extend_from_slice(&bits.to_be_bytes()[8 - size..]),
        }
    }

    fn bits(self, bytes: &[u8]) -> u64 {
        let mut bits = [0; 8];
        match self {
            Self::Little => {
                bits[..bytes.len()].copy_from_slice(bytes);
                u64::from_le_bytes(bits)
            }
            Self::Big => {
                bits[8 - bytes.len()..].copy_from_slice(bytes);
                u64::from_be_bytes(bits)
            }
        }
    }

    fn real(self, bytes: &[u8]) -> f64 {
        let bits = self.bits(bytes);
        if bytes.len() == 4 {
            f32::from_bits(bits as u32) as f64
        } else {
            f64::from_bits(bits)
        }
    }

    fn push_real(self, bytes: &mut Vec<u8>, value: f64, size: usize) {
        let bits = if size == 4 {
            let value = if is_na_real(value) {
                f32::NAN
            } else {
                value as f32
            };
            u64::from(value.to_bits())
        } else {
            value.to_bits()
        };
        self.push(bytes, bits, size);
    }
}

pub(super) fn read_bin(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let what = args.get(1);
    let ty = ctx.session().type_of(what)?;
    let kind = if ty == SexpType::Str && ctx.session().xlength(what)? > 0 {
        match text(ctx, call, what, "what")?.as_str() {
            "integer" => Some(Kind::Integer),
            "logical" => Some(Kind::Logical),
            "numeric" | "double" => Some(Kind::Double),
            "complex" => Some(Kind::Complex),
            "raw" => Some(Kind::Raw),
            "character" => Some(Kind::Character),
            _ => None,
        }
    } else {
        Kind::from_type(ty)
    }
    .ok_or_else(|| ctx.stop(call.err_call(), "invalid 'what' argument"))?;
    let n = usize::try_from(integer(ctx, args.get(2))?)
        .map_err(|_| ctx.stop(call.err_call(), "invalid 'n' argument"))?;
    let size = kind.size(ctx, call, args.get(3))?;
    let signed = flag(ctx, call, args.get(4), "signed")?;
    let endian = Endian::parse(ctx, call, args.get(5))?;
    if !signed && !(matches!(kind, Kind::Integer | Kind::Logical) && size <= 2) {
        ctx.warn(
            call.err_call(),
            "'signed = FALSE' is only valid for integers of sizes 1 and 2",
        );
    }
    if kind == Kind::Character {
        let source = args.get(0);
        let raw = ctx.session().type_of(source)? == SexpType::Raw;
        let (values, incomplete) =
            connection::read(ctx, call, source, Access::Binary, |input, limit| {
                read_strings(input, n, limit, raw)
            })?;
        if incomplete {
            ctx.warn(
                call.err_call(),
                "incomplete string at end of file has been discarded",
            );
        }
        return strings(ctx, values.into_iter().map(Some), Encoding::Native);
    }

    let data = connection::read(ctx, call, args.get(0), Access::Binary, |input, limit| {
        let limit = limit / kind.allocation_size().max(size);
        let requested = n.min(limit.saturating_add(1)) * size;
        let mut data = Vec::new();
        input.take(requested as u64).read_to_end(&mut data)?;
        if data.len() / size > limit {
            return Err(io::Error::other("I/O result exceeds the vector size limit"));
        }
        Ok(data)
    })?;
    let chunks = data.chunks_exact(size);
    match kind {
        Kind::Raw => Ok(ctx.session_mut().alloc_raw_from(&data)?),
        Kind::Integer | Kind::Logical => {
            let values = chunks.map(|chunk| {
                let bits = endian.bits(chunk);
                match (size, signed) {
                    (1, true) => i32::from(bits as i8),
                    (2, true) => i32::from(bits as i16),
                    _ => bits as i32,
                }
            });
            if kind == Kind::Logical {
                let values: Vec<_> = values
                    .map(|v| match v {
                        0 => RLogical::FALSE,
                        NA_INTEGER => RLogical::NA,
                        _ => RLogical::TRUE,
                    })
                    .collect();
                Ok(ctx.session_mut().alloc_lgl_from(&values)?)
            } else {
                Ok(ctx
                    .session_mut()
                    .alloc_int_from(&values.collect::<Vec<_>>())?)
            }
        }
        Kind::Double => {
            let values: Vec<_> = chunks.map(|chunk| endian.real(chunk)).collect();
            Ok(ctx.session_mut().alloc_real_from(&values)?)
        }
        Kind::Complex => {
            let values: Vec<_> = chunks
                .map(|chunk| RComplex {
                    re: endian.real(&chunk[..8]),
                    im: endian.real(&chunk[8..]),
                })
                .collect();
            Ok(ctx.session_mut().alloc_cplx_from(&values)?)
        }
        Kind::Character => Err(ctx.stop(call.err_call(), "invalid 'what' argument")),
    }
}

fn read_strings(
    input: &mut dyn BufRead,
    n: usize,
    mut limit: usize,
    raw: bool,
) -> io::Result<(Vec<Vec<u8>>, bool)> {
    let mut values = Vec::new();
    for _ in 0..n {
        let mut value = Vec::new();
        let count = input
            .take((limit as u64).saturating_add(1))
            .read_until(0, &mut value)?;
        limit = limit
            .checked_sub(count)
            .ok_or_else(|| io::Error::other("I/O result exceeds the vector size limit"))?;
        if value.last() == Some(&0) {
            value.pop();
        } else if !raw {
            return Ok((values, !value.is_empty()));
        }
        limit = limit
            .checked_sub(size_of::<Vec<u8>>())
            .ok_or_else(|| io::Error::other("I/O result exceeds the vector size limit"))?;
        values.push(value);
    }
    Ok((values, false))
}

pub(super) fn write_bin(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let value = args.get(0);
    let kind = Kind::from_type(ctx.session().type_of(value)?)
        .ok_or_else(|| ctx.stop(call.err_call(), "can only write vector objects"))?;
    let size = kind.size(ctx, call, args.get(2))?;
    let endian = Endian::parse(ctx, call, args.get(3))?;
    let len = ctx.session().xlength(value)?;
    check_limit(ctx, call, len.saturating_mul(size))?;
    let mut data = Vec::with_capacity(len * size);
    match kind {
        Kind::Raw => data.extend(ctx.session().raw(value)?.iter()),
        Kind::Integer | Kind::Logical => {
            let session = ctx.session();
            if kind == Kind::Integer {
                for value in session.int(value)?.iter() {
                    endian.push(&mut data, value as i64 as u64, size);
                }
            } else {
                for value in session.lgl(value)?.iter() {
                    endian.push(&mut data, value.raw() as i64 as u64, size);
                }
            }
        }
        Kind::Double => {
            for value in ctx.session().real(value)?.iter() {
                endian.push_real(&mut data, value, size);
            }
        }
        Kind::Complex => {
            for value in ctx.session().cplx(value)?.iter() {
                endian.push_real(&mut data, value.re, 8);
                endian.push_real(&mut data, value.im, 8);
            }
        }
        Kind::Character => {
            let use_bytes = flag(ctx, call, args.get(4), "useBytes")?;
            let encoding = if use_bytes {
                ByteEncoding::Bytes
            } else {
                ByteEncoding::native(ctx)
            };
            for value in translated_bytes(ctx, call, value, encoding)? {
                let value = value.as_deref().unwrap_or(b"NA");
                check_limit(
                    ctx,
                    call,
                    data.len().saturating_add(value.len()).saturating_add(1),
                )?;
                data.extend_from_slice(value);
                data.push(0);
            }
        }
    }
    let dest = args.get(1);
    if ctx.session().type_of(dest)? == SexpType::Raw {
        ctx.set_visible(true);
        return Ok(ctx.session_mut().alloc_raw_from(&data)?);
    }
    connection::write(ctx, call, dest, Access::Binary, &data)?;
    ctx.set_visible(false);
    Ok(SEXP_NIL)
}
