//! Decode R literals and quoted names.

use roxide_diag::Span;

use crate::ast::{Const, Node, StrEncoding};
use crate::parser::{ParseError, ParseWarning};

pub(crate) fn parse_r_double(s: &str) -> Option<f64> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return parse_hex_float(hex);
    }
    s.parse::<f64>().ok()
}

fn parse_hex_float(hex: &str) -> Option<f64> {
    let (mantissa, exponent) = match hex.split_once(['p', 'P']) {
        Some((mantissa, power)) => (mantissa, parse_hex_exponent(power)?),
        None => (hex, 0),
    };
    if mantissa.is_empty() {
        return None;
    }
    let integer_digits = mantissa.find('.').unwrap_or(mantissa.len());
    let mut first_nonzero = None;
    let mut significand = 0.0;
    let mut scale = 1.0;
    // Accumulate the digits in order with double precision.
    for (index, byte) in mantissa.bytes().filter(|&byte| byte != b'.').enumerate() {
        let digit = hex_digit(byte)?;
        if first_nonzero.is_none() {
            if digit == 0 {
                continue;
            }
            first_nonzero = Some(index);
        }
        significand += f64::from(digit) * scale;
        scale *= 0.0625;
    }
    let Some(first_nonzero) = first_nonzero else {
        return Some(0.0);
    };

    // GNU R returns zero for explicit powers below -1023.
    if exponent < -1023 {
        return Some(0.0);
    }
    let power = exponent.saturating_add(4 * (integer_digits as i64 - first_nonzero as i64 - 1));
    Some(match power {
        1024.. => f64::INFINITY,
        ..=-1080 => 0.0,
        -1079..=-1023 => (significand * 2.0f64.powi((power + 1022) as i32)) * f64::MIN_POSITIVE,
        _ => significand * f64::from_bits(((power + 1023) as u64) << 52),
    })
}

fn parse_hex_exponent(power: &str) -> Option<i64> {
    let digits = power.strip_prefix(['-', '+']).unwrap_or(power);
    if digits.is_empty() {
        return None;
    }
    let exponent = digits.bytes().try_fold(0i64, |value, digit| {
        digit.is_ascii_digit().then(|| {
            value
                .saturating_mul(10)
                .saturating_add(i64::from(digit - b'0'))
        })
    })?;
    Some(if power.starts_with('-') {
        -exponent
    } else {
        exponent
    })
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub(crate) fn parse_r_int(
    num_str: &str,
    span: Span,
    warnings: &mut Vec<ParseWarning>,
) -> Result<Node, ParseError> {
    let f = parse_r_double(num_str).ok_or_else(|| ParseError {
        message: format!("invalid integer literal '{num_str}L'"),
        span,
    })?;
    let exponent_marker = if num_str.starts_with("0x") || num_str.starts_with("0X") {
        ['p', 'P']
    } else {
        ['e', 'E']
    };
    let simple_decimal_point = num_str.contains('.') && !num_str.contains(exponent_marker);
    let node = if f.fract() == 0.0 && f >= f64::from(-i32::MAX) && f <= f64::from(i32::MAX) {
        if simple_decimal_point {
            warnings.push(ParseWarning {
                message: format!("integer literal {num_str}L contains unnecessary decimal point"),
                span,
            });
        }
        Const::Int(f as i32)
    } else {
        let message = if simple_decimal_point {
            format!("integer literal {num_str}L contains decimal; using numeric value")
        } else {
            format!("non-integer value {num_str}L qualified with L; using numeric value")
        };
        warnings.push(ParseWarning { message, span });
        Const::Real(f.to_bits())
    };
    Ok(Node::Const(node))
}

pub(crate) fn decode_string(
    raw_token: &str,
    is_raw: bool,
    span: Span,
) -> Result<(Box<[u8]>, StrEncoding), ParseError> {
    if is_raw {
        // Strip r/R, quote, dashes, opening delim ... closing delim, dashes, quote.
        let inner = &raw_token[2..raw_token.len() - 1];
        let dashes = inner.bytes().take_while(|&b| b == b'-').count();
        let body = &inner[dashes + 1..inner.len() - dashes - 1];
        if body.as_bytes().contains(&0) {
            return Err(ParseError {
                message: String::from("nul character not allowed"),
                span,
            });
        }
        let enc = if body.is_ascii() {
            StrEncoding::Native
        } else {
            StrEncoding::Utf8
        };
        return Ok((body.as_bytes().into(), enc));
    }
    let inner = &raw_token[1..raw_token.len() - 1];
    decode_escaped(inner, span, false)
}

pub(crate) fn symbol_name(bytes: &[u8], span: Span) -> Result<Box<str>, ParseError> {
    if bytes.is_empty() {
        return Err(ParseError {
            message: String::from("attempt to use zero-length variable name"),
            span,
        });
    }
    Ok(String::from_utf8_lossy(bytes).into_owned().into_boxed_str())
}

pub(crate) fn decode_escaped(
    inner: &str,
    span: Span,
    backtick: bool,
) -> Result<(Box<[u8]>, StrEncoding), ParseError> {
    let mut out = Vec::with_capacity(inner.len());
    let mut has_unicode_escape = false;
    let mut has_byte_escape = false;
    let mut has_high_hex = false;
    let bytes = inner.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'\\' {
            if bytes[i] == 0 {
                return Err(ParseError {
                    message: String::from("nul character not allowed"),
                    span,
                });
            }
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        i += 1;
        if i >= bytes.len() {
            return Err(ParseError {
                message: String::from("unterminated escape sequence"),
                span,
            });
        }
        match bytes[i] {
            b'n' => {
                out.push(b'\n');
                i += 1;
            }
            b'r' => {
                out.push(b'\r');
                i += 1;
            }
            b't' => {
                out.push(b'\t');
                i += 1;
            }
            b'b' => {
                out.push(0x08);
                i += 1;
            }
            b'a' => {
                out.push(0x07);
                i += 1;
            }
            b'f' => {
                out.push(0x0C);
                i += 1;
            }
            b'v' => {
                out.push(0x0B);
                i += 1;
            }
            b'\\' => {
                out.push(b'\\');
                i += 1;
            }
            b'"' => {
                out.push(b'"');
                i += 1;
            }
            b'\'' => {
                out.push(b'\'');
                i += 1;
            }
            b'`' => {
                out.push(b'`');
                i += 1;
            }
            b' ' => {
                out.push(b' ');
                i += 1;
            }
            b'\n' | b'\r' => {
                out.push(b'\n');
                if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
                    i += 1;
                }
                i += 1;
            }
            b'x' => {
                has_byte_escape = true;
                i += 1;
                let mut val = 0u8;
                let mut count = 0;
                while count < 2 && i < bytes.len() {
                    if let Some(d) = hex_digit(bytes[i]) {
                        val = val * 16 + d;
                        i += 1;
                        count += 1;
                    } else {
                        break;
                    }
                }
                if count == 0 {
                    return Err(ParseError {
                        message: String::from("\\x used without hex digits"),
                        span,
                    });
                }
                if val == 0 {
                    return Err(ParseError {
                        message: String::from("nul character not allowed"),
                        span,
                    });
                }
                if val >= 0x80 {
                    has_high_hex = true;
                }
                out.push(val);
            }
            b'u' | b'U' => {
                if backtick {
                    return Err(ParseError {
                        message: String::from("Unicode escapes are not allowed inside backticks"),
                        span,
                    });
                }
                let max_digits = if bytes[i] == b'u' { 4 } else { 8 };
                i += 1;
                let braced = i < bytes.len() && bytes[i] == b'{';
                if braced {
                    i += 1;
                }
                let mut cp = 0u32;
                let mut count = 0;
                while count < max_digits && i < bytes.len() {
                    if let Some(d) = hex_digit(bytes[i]) {
                        cp = cp * 16 + u32::from(d);
                        i += 1;
                        count += 1;
                    } else {
                        break;
                    }
                }
                if braced {
                    if i >= bytes.len() || bytes[i] != b'}' {
                        return Err(ParseError {
                            message: String::from("unterminated unicode escape"),
                            span,
                        });
                    }
                    i += 1;
                }
                if count == 0 || cp == 0 {
                    return Err(ParseError {
                        message: String::from("invalid unicode escape"),
                        span,
                    });
                }
                let ch = char::from_u32(cp).ok_or_else(|| ParseError {
                    message: String::from("invalid unicode scalar value"),
                    span,
                })?;
                let mut buf = [0u8; 4];
                out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                has_unicode_escape = true;
            }
            b'0'..=b'7' => {
                has_byte_escape = true;
                let mut val = 0u32;
                let mut count = 0;
                while count < 3 && i < bytes.len() && (b'0'..=b'7').contains(&bytes[i]) {
                    val = val * 8 + u32::from(bytes[i] - b'0');
                    i += 1;
                    count += 1;
                }
                if val == 0 {
                    return Err(ParseError {
                        message: String::from("nul character not allowed"),
                        span,
                    });
                }
                if val > 0xFF {
                    return Err(ParseError {
                        message: format!("\\{val:o} exceeds maximum allowed octal value \\377"),
                        span,
                    });
                }
                if val >= 0x80 {
                    has_high_hex = true;
                }
                out.push(val as u8);
            }
            other => {
                return Err(ParseError {
                    message: format!("'\\{}' is an unrecognized escape", other as char),
                    span,
                });
            }
        }
    }
    if has_unicode_escape && has_byte_escape {
        return Err(ParseError {
            message: String::from(
                "mixing Unicode and octal/hex escapes in a string is not allowed",
            ),
            span,
        });
    }
    let enc = if has_high_hex {
        StrEncoding::Bytes
    } else if has_unicode_escape || !out.is_ascii() {
        StrEncoding::Utf8
    } else {
        StrEncoding::Native
    };
    Ok((out.into_boxed_slice(), enc))
}
