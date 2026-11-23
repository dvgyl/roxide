//! Vector recycling and C-style format conversions.

use std::collections::HashMap;

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{CharId, Encoding, RSession, SEXP_MISSING, SexpType, ValueApi, is_na_real};

use super::{character, utf8};
use crate::ops::common::{error, tag, vector_limit};

const MAX_FORMAT_FIELD: usize = 8192;

pub(super) fn sprintf(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let s = ctx.session();
    let fmt_index = args
        .iter()
        .position(|(n, _)| tag(s, n) == Some("fmt"))
        .or_else(|| args.iter().position(|(n, _)| n.is_none()));
    let fmt = fmt_index.map_or(SEXP_MISSING, |i| args.get(i));
    let Ok(formats) = s.str_slice(fmt) else {
        return Err(error(ctx, call, "'fmt' is not a character vector"));
    };
    let formats = formats.to_vec();
    let arguments = args
        .iter()
        .enumerate()
        .filter(|(i, _)| Some(*i) != fmt_index)
        .map(|(_, (_, x))| {
            s.xlength(x).map(|len| Argument {
                x,
                len,
                strings: None,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut arguments = arguments;
    let lengths = || std::iter::once(formats.len()).chain(arguments.iter().map(|arg| arg.len));
    if lengths().any(|n| n == 0) {
        return Ok(ctx.session_mut().alloc_str_from(&[])?);
    }
    let len = lengths().max().unwrap_or(0);
    if lengths().any(|n| len % n != 0) {
        return Err(error(
            ctx,
            call,
            "arguments cannot be recycled to the same length",
        ));
    }
    vector_limit(ctx, call, len, size_of::<CharId>())?;
    let limit = ctx.host().limits().max_vector_bytes;
    let mut parsed = HashMap::new();
    let mut ids = Vec::with_capacity(len);
    let mut used = vec![false; arguments.len()];
    let mut total = 0usize;
    for i in 0..len {
        let id = formats[i % formats.len()];
        let mut bytes = Vec::new();
        let mut encoding = Encoding::Native;
        let entry = match parsed.entry(id) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => {
                let s = ctx.session();
                let mark = s.char_encoding(id)?;
                let fmt = utf8(s, id)?;
                if fmt.len() > MAX_FORMAT_FIELD {
                    return Err(error(
                        ctx,
                        call,
                        "'fmt' length exceeds maximal format length 8192",
                    ));
                }
                let pieces = parse(&fmt).map_err(|msg| error(ctx, call, msg))?;
                entry.insert((pieces, mark))
            }
        };
        if matches!(entry.1, Encoding::Latin1 | Encoding::Utf8) {
            encoding = Encoding::Utf8;
        }
        for piece in &entry.0 {
            let part = match piece {
                Piece::Literal(bytes) => std::borrow::Cow::Borrowed(bytes.as_slice()),
                Piece::Field(spec) => {
                    let field = spec.resolve(ctx, call, &arguments, &mut used, i)?;
                    let arg = arguments
                        .get_mut(spec.index)
                        .ok_or_else(|| error(ctx, call, "too few arguments"))?;
                    used[spec.index] = true;
                    let index = i % arg.len;
                    let value = if spec.kind == b's' {
                        if arg.strings.is_none() {
                            let ids = character(ctx.session_mut(), arg.x)?;
                            for &id in &ids {
                                ctx.protect(id.as_sexp());
                            }
                            arg.strings = Some(ids);
                        }
                        let id = arg
                            .strings
                            .as_ref()
                            .and_then(|v| super::recycle(v, i))
                            .unwrap_or(CharId::NA);
                        let s = ctx.session();
                        let mark = s.char_encoding(id)?;
                        let value = utf8(s, id)?;
                        if matches!(mark, Encoding::Utf8 | Encoding::Latin1) {
                            encoding = Encoding::Utf8;
                        }
                        field.string(&value)
                    } else {
                        field.number(ctx, call, arg.x, index)?
                    };
                    std::borrow::Cow::Owned(value)
                }
            };
            total = total.saturating_add(part.len());
            if total > limit {
                return Err(error(ctx, call, "vector size limit exceeded"));
            }
            bytes.extend_from_slice(&part);
        }
        let id = ctx.session_mut().mk_char(&bytes, encoding)?;
        ctx.protect(id.as_sexp());
        ids.push(id);
    }
    let unused = used.iter().filter(|&&used| !used).count();
    if unused != 0 && formats.len() == 1 {
        let s = ctx.session();
        let bytes = utf8(s, formats[0])?;
        let fmt = String::from_utf8_lossy(&bytes);
        let count = if unused == 1 {
            "one argument".to_owned()
        } else {
            format!("{unused} arguments")
        };
        let msg = format!("{count} not used by format '{fmt}'");
        let out = ctx.session_mut().alloc_str_from(&ids)?;
        ctx.protect(out);
        ctx.warn_now(call.err_call(), msg)?;
        return Ok(out);
    }
    Ok(ctx.session_mut().alloc_str_from(&ids)?)
}

struct Argument {
    x: SexpId,
    len: usize,
    strings: Option<Vec<CharId>>,
}

enum Piece {
    Literal(Vec<u8>),
    Field(Spec),
}

#[derive(Clone, Copy, Default)]
struct Flags {
    left: bool,
    plus: bool,
    space: bool,
    zero: bool,
    alternate: bool,
}

#[derive(Clone, Copy)]
enum Measure {
    Literal(usize),
    Argument(usize),
}

struct Spec {
    source: String,
    kind: u8,
    index: usize,
    flags: Flags,
    width: Measure,
    precision: Option<Measure>,
}

fn parse(mut input: &[u8]) -> Result<Vec<Piece>, String> {
    let mut out = Vec::new();
    let mut next = 0usize;
    while let Some(i) = input.iter().position(|&b| b == b'%') {
        if i != 0 {
            out.push(Piece::Literal(input[..i].to_vec()));
        }
        input = &input[i + 1..];
        if let Some(rest) = input.strip_prefix(b"%") {
            out.push(Piece::Literal(vec![b'%']));
            input = rest;
            continue;
        }
        let start = input;
        let index = position(&mut input)?;
        let mut flags = Flags::default();
        while let Some((&b, rest)) = input.split_first() {
            match b {
                b'-' => flags.left = true,
                b'+' => flags.plus = true,
                b' ' => flags.space = true,
                b'0' => flags.zero = true,
                b'#' => flags.alternate = true,
                _ => break,
            }
            input = rest;
        }
        let width = measure(&mut input, &mut next)?;
        let precision = if let Some(rest) = input.strip_prefix(b".") {
            input = rest;
            Some(measure(&mut input, &mut next)?)
        } else {
            None
        };
        let Some((&kind, rest)) = input.split_first() else {
            return Err(format!(
                "unrecognised format specification '%{}'",
                String::from_utf8_lossy(start)
            ));
        };
        input = rest;
        let source = format!(
            "%{}",
            String::from_utf8_lossy(&start[..start.len() - rest.len()])
        );
        if !b"dixXofeEgGaAs".contains(&kind) {
            return Err(format!("unrecognised format specification '{source}'"));
        }
        let index = index.unwrap_or_else(|| take_next(&mut next));
        out.push(Piece::Field(Spec {
            source,
            kind,
            index,
            flags,
            width,
            precision,
        }));
    }
    if !input.is_empty() {
        out.push(Piece::Literal(input.to_vec()));
    }
    Ok(out)
}

fn take_next(next: &mut usize) -> usize {
    let value = *next;
    *next += 1;
    value
}

fn position(input: &mut &[u8]) -> Result<Option<usize>, String> {
    let mut rest = *input;
    let n = digits(&mut rest);
    if let Some(tail) = rest.strip_prefix(b"$") {
        if rest.len() == input.len() || n == 0 {
            return Err("reference to non-existent argument".into());
        }
        *input = tail;
        Ok(Some(n - 1))
    } else {
        Ok(None)
    }
}

fn measure(input: &mut &[u8], next: &mut usize) -> Result<Measure, String> {
    if let Some(rest) = input.strip_prefix(b"*") {
        *input = rest;
        let index = position(input)?.unwrap_or_else(|| take_next(next));
        Ok(Measure::Argument(index))
    } else {
        Ok(Measure::Literal(digits(input)))
    }
}

fn digits(input: &mut &[u8]) -> usize {
    let mut n = 0usize;
    while let Some((&b, rest)) = input.split_first().filter(|(b, _)| b.is_ascii_digit()) {
        n = n.saturating_mul(10).saturating_add(usize::from(b - b'0'));
        *input = rest;
    }
    n
}

struct Field<'a> {
    spec: &'a Spec,
    flags: Flags,
    width: usize,
    precision: Option<usize>,
}

impl Spec {
    fn resolve<'a>(
        &'a self,
        ctx: &mut Ctx,
        call: &Call,
        args: &[Argument],
        used: &mut [bool],
        i: usize,
    ) -> EvalResult<Field<'a>> {
        let mut read = |measure| -> EvalResult<(usize, bool)> {
            let (n, negative) = match measure {
                Measure::Literal(n) => (n, false),
                Measure::Argument(index) => {
                    let arg = args
                        .get(index)
                        .ok_or_else(|| error(ctx, call, "too few arguments"))?;
                    used[index] = true;
                    let s = ctx.session();
                    let value = match s.type_of(arg.x)? {
                        SexpType::Int | SexpType::Real => numeric_at(s, arg.x, i % arg.len)?,
                        _ => f64::NAN,
                    };
                    if !value.is_finite() || value.trunc() != value {
                        return Err(error(
                            ctx,
                            call,
                            "argument for '*' conversion specification must be a number",
                        ));
                    }
                    (value.abs() as usize, value < 0.0)
                }
            };
            if n > MAX_FORMAT_FIELD {
                return Err(error(ctx, call, "format width or precision exceeds 8192"));
            }
            Ok((n, negative))
        };
        let (width, left) = read(self.width)?;
        let precision = self
            .precision
            .map(read)
            .transpose()?
            .and_then(|(n, negative)| (!negative).then_some(n));
        let mut flags = self.flags;
        flags.left |= left;
        Ok(Field {
            spec: self,
            flags,
            width,
            precision,
        })
    }
}

fn numeric_at(s: &RSession, x: SexpId, i: usize) -> EvalResult<f64> {
    use roxide_value::{NA_INTEGER, NA_REAL};
    Ok(match s.type_of(x)? {
        SexpType::Real => s.real(x)?.get(i),
        SexpType::Int => match s.int(x)?.get(i) {
            NA_INTEGER => NA_REAL,
            value => f64::from(value),
        },
        SexpType::Lgl => s
            .lgl(x)?
            .get(i)
            .get()
            .map_or(NA_REAL, |b| f64::from(u8::from(b))),
        _ => f64::NAN,
    })
}

impl Field<'_> {
    fn string(&self, value: &[u8]) -> Vec<u8> {
        let value = &value[..self.precision.unwrap_or(value.len()).min(value.len())];
        self.pad(b"", value, false)
    }

    fn number(&self, ctx: &mut Ctx, call: &Call, x: SexpId, i: usize) -> EvalResult<Vec<u8>> {
        let s = ctx.session();
        let ty = s.type_of(x)?;
        let integer = b"dixXo".contains(&self.spec.kind);
        let x = numeric_at(s, x, i)?;
        let valid_type = matches!(ty, SexpType::Lgl | SexpType::Int | SexpType::Real);
        let valid_integer = is_na_real(x)
            || (x.trunc() == x && (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&x));
        if !valid_type || (integer && !valid_integer) {
            let advice = match ty {
                SexpType::Real => "use format %f, %e, %g or %a for numeric objects",
                SexpType::Str => "use format %s for character objects",
                SexpType::Lgl => "use format %d or %i for logical objects",
                _ => "use format %s for this type of object",
            };
            return Err(error(
                ctx,
                call,
                format!("invalid format '{}'; {advice}", self.spec.source),
            ));
        }
        if is_na_real(x) || (integer && x == f64::from(i32::MIN)) {
            if !is_na_real(x) {
                ctx.warn_now(
                    call.err_call(),
                    "NAs introduced by coercion to integer range",
                )?;
            }
            return Ok(self.pad(b"", b"NA", self.flags.zero));
        }
        if x.is_nan() {
            return Ok(self.pad(b"", b"NaN", self.flags.zero));
        }
        let unsigned = integer && !b"di".contains(&self.spec.kind);
        let mut prefix = if unsigned {
            String::new()
        } else {
            self.sign(x).to_owned()
        };
        let body = if x.is_infinite() {
            "Inf".into()
        } else if integer {
            self.integer_digits(x as i32, &mut prefix)
        } else if b"aA".contains(&self.spec.kind) {
            let upper = self.spec.kind == b'A';
            prefix.push_str(if upper { "0X" } else { "0x" });
            hexadecimal(x.abs(), self.precision, self.flags.alternate, upper)
        } else {
            self.decimal(x.abs())
        };
        let zero = self.flags.zero && !(integer && self.precision.is_some());
        Ok(self.pad(prefix.as_bytes(), body.as_bytes(), zero))
    }

    fn sign(&self, x: f64) -> &'static str {
        if x.is_sign_negative() {
            "-"
        } else if self.flags.plus {
            "+"
        } else if self.flags.space {
            " "
        } else {
            ""
        }
    }

    fn integer_digits(&self, x: i32, prefix: &mut String) -> String {
        let unsigned = x as u32;
        let mut digits = if x == 0 && self.precision == Some(0) {
            String::new()
        } else {
            match self.spec.kind {
                b'x' => format!("{unsigned:x}"),
                b'X' => format!("{unsigned:X}"),
                b'o' => format!("{unsigned:o}"),
                _ => x.unsigned_abs().to_string(),
            }
        };
        let precision = self.precision.unwrap_or(0);
        if digits.len() < precision {
            digits = format!("{digits:0>precision$}");
        }
        if self.flags.alternate {
            match self.spec.kind {
                b'x' if x != 0 => prefix.push_str("0x"),
                b'X' if x != 0 => prefix.push_str("0X"),
                b'o' if !digits.starts_with('0') => prefix.push('0'),
                _ => (),
            }
        }
        digits
    }

    fn decimal(&self, value: f64) -> String {
        let precision = self.precision.unwrap_or(6);
        let upper = b"EG".contains(&self.spec.kind);
        let alternate = self.flags.alternate;
        match self.spec.kind {
            b'f' => fixed(value, precision, alternate),
            b'e' | b'E' => {
                let (digits, pow) = scientific(value, precision);
                let digits = add_decimal(digits, alternate);
                format!(
                    "{digits}{}",
                    exponent(pow, if upper { 'E' } else { 'e' }, 2)
                )
            }
            _ => {
                let precision = precision.max(1);
                let (digits, pow) = scientific(value, precision - 1);
                if pow < -4 || pow >= precision as i32 {
                    let digits = if alternate {
                        add_decimal(digits, true)
                    } else {
                        trim_zeroes(digits)
                    };
                    format!(
                        "{digits}{}",
                        exponent(pow, if upper { 'E' } else { 'e' }, 2)
                    )
                } else {
                    let digits = fixed(value, (precision as i32 - 1 - pow) as usize, alternate);
                    if alternate {
                        digits
                    } else {
                        trim_zeroes(digits)
                    }
                }
            }
        }
    }

    fn pad(&self, prefix: &[u8], value: &[u8], zero: bool) -> Vec<u8> {
        let len = prefix.len() + value.len();
        let n = self.width.saturating_sub(len);
        let mut bytes = Vec::with_capacity(len + n);
        if !self.flags.left && !zero {
            bytes.resize(n, b' ');
        }
        bytes.extend_from_slice(prefix);
        if !self.flags.left && zero {
            bytes.resize(bytes.len() + n, b'0');
        }
        bytes.extend_from_slice(value);
        if self.flags.left {
            bytes.resize(len + n, b' ');
        }
        bytes
    }
}

fn scientific(value: f64, precision: usize) -> (String, i32) {
    let mut digits = format!("{value:.precision$e}");
    let power = digits.find('e').map_or(0, |pos| {
        let power = digits[pos + 1..].parse().unwrap_or(0);
        digits.truncate(pos);
        power
    });
    (digits, power)
}

fn fixed(value: f64, precision: usize, alternate: bool) -> String {
    add_decimal(format!("{value:.precision$}"), alternate)
}

fn add_decimal(mut digits: String, alternate: bool) -> String {
    if alternate && !digits.contains('.') {
        digits.push('.');
    }
    digits
}

fn trim_zeroes(mut digits: String) -> String {
    if digits.contains('.') {
        digits.truncate(digits.trim_end_matches('0').trim_end_matches('.').len());
    }
    digits
}

fn exponent(power: i32, marker: char, width: usize) -> String {
    let sign = if power < 0 { '-' } else { '+' };
    format!("{marker}{sign}{:0width$}", power.unsigned_abs())
}

fn hexadecimal(value: f64, precision: Option<usize>, alternate: bool, upper: bool) -> String {
    let bits = value.to_bits();
    let fraction = bits & ((1 << 52) - 1);
    let biased = ((bits >> 52) & 0x7ff) as i32;
    let (mut significand, power) = if biased != 0 {
        (fraction | (1 << 52), biased - 1023)
    } else if fraction != 0 {
        let shift = fraction.leading_zeros() - 11;
        (fraction << shift, -1022 - shift as i32)
    } else {
        (0, 0)
    };
    let meaningful = precision.unwrap_or(13).min(13);
    let shift = (13 - meaningful) * 4;
    if shift != 0 {
        let rest = significand & ((1 << shift) - 1);
        let half = 1 << (shift - 1);
        significand >>= shift;
        if rest > half || (rest == half && significand & 1 != 0) {
            significand += 1;
        }
    }
    let lead = significand >> (4 * meaningful);
    let fraction = significand & ((1 << (4 * meaningful)) - 1);
    let mut digits = if meaningful == 0 {
        String::new()
    } else if upper {
        format!("{fraction:0meaningful$X}")
    } else {
        format!("{fraction:0meaningful$x}")
    };
    if let Some(n) = precision {
        digits.extend(std::iter::repeat_n('0', n.saturating_sub(meaningful)));
    } else {
        digits.truncate(digits.trim_end_matches('0').len());
    }
    let point = if !digits.is_empty() || alternate {
        "."
    } else {
        ""
    };
    format!(
        "{lead}{point}{digits}{}",
        exponent(power, if upper { 'P' } else { 'p' }, 1)
    )
}
