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
