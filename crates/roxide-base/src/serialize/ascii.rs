use super::{WireResult, invalid};

const ESCAPES: &[(u8, u8)] = &[
    (b'\n', b'n'),
    (b'\r', b'r'),
    (b'\t', b't'),
    (7, b'a'),
    (8, b'b'),
    (11, b'v'),
    (12, b'f'),
    (b'\\', b'\\'),
    (b'\'', b'\''),
    (b'"', b'"'),
];

pub(super) fn escape(byte: u8) -> Option<u8> {
    ESCAPES
        .iter()
        .find_map(|&(value, code)| (value == byte).then_some(code))
}

pub(super) fn decode_string(mut line: &[u8], len: usize) -> WireResult<Vec<u8>> {
    let malformed = || invalid("invalid ASCII serialized string");
    if len > line.len() {
        return Err(malformed());
    }
    let mut decoded = Vec::new();
    decoded.try_reserve_exact(len).map_err(invalid)?;
    while let Some((&byte, rest)) = line.split_first() {
        if decoded.len() == len {
            return Err(malformed());
        }
        line = rest;
        if byte != b'\\' {
            decoded.push(byte);
            continue;
        }
        let (&code, rest) = line.split_first().ok_or_else(malformed)?;
        if let Some(value) = ESCAPES
            .iter()
            .find_map(|&(value, escape)| (escape == code).then_some(value))
        {
            decoded.push(value);
            line = rest;
        } else {
            let (octal, rest) = line.split_at_checked(3).ok_or_else(malformed)?;
            let octal = std::str::from_utf8(octal).map_err(invalid)?;
            decoded.push(u8::from_str_radix(octal, 8).map_err(invalid)?);
            line = rest;
        }
    }
    if decoded.len() != len {
        return Err(malformed());
    }
    Ok(decoded)
}

pub(super) fn format_real(value: f64) -> WireResult<String> {
    let scientific = format!("{:.15e}", value.abs());
    let (mantissa, exponent) = scientific
        .split_once('e')
        .ok_or_else(|| invalid("invalid ASCII real"))?;
    let exponent = exponent.parse::<i32>().map_err(invalid)?;
    let mut result = String::new();
    if value.is_sign_negative() {
        result.push('-');
    }
    if !(-4..16).contains(&exponent) {
        let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
        result.push_str(&format!("{mantissa}e{exponent:+03}\n"));
        return Ok(result);
    }

    let digits = mantissa.replace('.', "");
    let digits = digits.trim_end_matches('0');
    let point = exponent + 1;
    if digits.is_empty() {
        result.push('0');
    } else if point <= 0 {
        result.push_str("0.");
        result.extend(std::iter::repeat_n('0', (-point) as usize));
        result.push_str(digits);
    } else {
        let point = point as usize;
        if let Some((whole, fraction)) = digits.split_at_checked(point) {
            result.push_str(whole);
            if !fraction.is_empty() {
                result.push('.');
                result.push_str(fraction);
            }
        } else {
            result.push_str(digits);
            result.extend(std::iter::repeat_n('0', point - digits.len()));
        }
    }
    result.push('\n');
    Ok(result)
}
