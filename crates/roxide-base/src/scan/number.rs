//! Read numeric fields with R's decimal mark and precision rules.

use roxide_value::RComplex;

/// Parse a decimal integer, excluding R's missing-integer representation.
pub fn integer(text: &str) -> Option<i32> {
    text.trim_start_matches(|c: char| c.is_ascii_whitespace())
        .parse()
        .ok()
        .filter(|&n| n != i32::MIN)
}

/// Parse a logical field. The scanner accepts more spellings than inference.
pub fn logical(text: &str, explicit: bool) -> Option<bool> {
    match text {
        "T" | "TRUE" => Some(true),
        "F" | "FALSE" => Some(false),
        "True" | "true" if explicit => Some(true),
        "False" | "false" if explicit => Some(false),
        _ => None,
    }
}

/// Parse a real field and report whether its significand reached 2^53.
pub fn real(text: &str, decimal: char) -> Option<(f64, bool)> {
    let (value, loss, rest) = prefix(text, decimal)?;
    rest.trim_ascii().is_empty().then_some((value, loss))
}

/// Parse a complex field. Report the input suffix for each component that loses precision.
pub fn complex(text: &str, decimal: char) -> Option<(RComplex, [Option<&str>; 2])> {
    let (first, loss, rest) = prefix(text, decimal)?;
    let first_loss = loss.then_some(text);
    if rest.trim_ascii().is_empty() {
        return Some((RComplex { re: first, im: 0.0 }, [first_loss, None]));
    }
    if rest.trim_end_matches(|c: char| c.is_ascii_whitespace()) == "i" {
        return Some((RComplex { re: 0.0, im: first }, [first_loss, None]));
    }
    let suffix = rest.trim_start_matches(|c: char| c.is_ascii_whitespace());
    if !suffix.starts_with(['+', '-']) {
        return None;
    }
    let (im, loss, rest) = prefix(suffix, decimal)?;
    (rest.trim_end_matches(|c: char| c.is_ascii_whitespace()) == "i").then_some((
        RComplex { re: first, im },
        [first_loss, loss.then_some(suffix)],
    ))
}

fn prefix(text: &str, decimal: char) -> Option<(f64, bool, &str)> {
    let text = text.trim_start_matches(|c: char| c.is_ascii_whitespace());
    let (sign, body) = match text.as_bytes().first()? {
        b'+' => (1.0, &text[1..]),
        b'-' => (-1.0, &text[1..]),
        _ => (1.0, text),
    };
    for (token, value) in [
        ("infinity", f64::INFINITY),
        ("inf", f64::INFINITY),
        ("nan", f64::NAN),
    ] {
        if body.get(..token.len()).is_some_and(|head| {
            if token == "nan" {
                matches!(head, "NaN" | "nan")
            } else {
                head.eq_ignore_ascii_case(token)
            }
        }) {
            return Some((value * sign, false, &body[token.len()..]));
        }
    }
    let hex = body.starts_with("0x") || body.starts_with("0X");
    let body = if hex { &body[2..] } else { body };
    let radix = if hex { 16 } else { 10 };
    let mut value = 0.0;
    let mut end = 0;
    let mut point = false;
    let mut digits = 0i32;
    let mut fractional = 0i32;
    for (index, c) in body.char_indices() {
        if !point && c == decimal {
            point = true;
        } else if let Some(digit) = c.to_digit(radix) {
            value = value * f64::from(radix) + f64::from(digit);
            digits += 1;
            fractional += i32::from(point);
        } else {
            break;
        }
        end = index + c.len_utf8();
    }
    if digits == 0 && !(hex && point) {
        return None;
    }
    let mantissa_end = end;
    let marker = if hex { ['p', 'P'] } else { ['e', 'E'] };
    let mut exponent = 0i32;
    if body[end..].starts_with(marker) {
        end += 1;
        let negative = body[end..].starts_with('-');
        if body[end..].starts_with(['+', '-']) {
            end += 1;
        }
        let start = end;
        for byte in body[end..].bytes().take_while(u8::is_ascii_digit) {
            exponent = exponent
                .saturating_mul(10)
                .saturating_add(i32::from(byte - b'0'));
            end += 1;
        }
        if start == end {
            return None;
        }
        if negative {
            exponent = -exponent;
        }
    }
    let loss = value >= 9_007_199_254_740_992.0;
    let base: f64 = if hex { 2.0 } else { 10.0 };
    let power = exponent.saturating_sub(fractional.saturating_mul(if hex { 4 } else { 1 }));
    let scaled = if value == 0.0 || (hex && exponent < -1023) {
        0.0
    } else if !hex && (!value.is_finite() || !(-308..=308).contains(&power)) {
        body[..end].replace(decimal, ".").parse().ok()?
    } else if hex && !value.is_finite() {
        scale_large_hex(&body[..mantissa_end], decimal, power)
    } else {
        scale(value, base, power)
    };
    Some((scaled * sign, loss, &body[end..]))
}

fn scale(mut value: f64, base: f64, power: i32) -> f64 {
    if power >= 0 {
        return value * base.powi(power);
    }
    let magnitude = power.unsigned_abs();
    let step = if base == 2.0 { 1023 } else { 308 };
    if magnitude > step {
        value /= base.powi(step as i32);
    }
    value / base.powi(magnitude.saturating_sub(if magnitude > step { step } else { 0 }) as i32)
}

fn scale_large_hex(text: &str, decimal: char, mut power: i32) -> f64 {
    let mut value = 0.0;
    for digit in text
        .chars()
        .filter(|&c| c != decimal)
        .filter_map(|c| c.to_digit(16))
    {
        if value < 9_007_199_254_740_992.0 {
            value = value * 16.0 + f64::from(digit);
        } else {
            power = power.saturating_add(4);
        }
    }
    scale(value, 2.0, power)
}
