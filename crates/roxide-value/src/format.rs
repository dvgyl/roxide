//! Pure GNU R 4.6.2 scalar and vector number/string formatting helpers.

use crate::na::{NA_INTEGER, RLogical, is_na_real, is_nan_not_na};

/// Formatting options corresponding to R `options(digits=, scipen=)`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FormatOpts {
    /// Significant digits (`options(digits)`, default 7).
    pub digits: usize,
    /// Scientific notation penalty (`options(scipen)`, default 0).
    pub scipen: i32,
}

impl Default for FormatOpts {
    fn default() -> Self {
        Self {
            digits: 7,
            scipen: 0,
        }
    }
}

/// Encode a byte slice as an R quoted string literal (`"..."`), escaping control characters.
#[must_use]
pub fn encode_string(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() + 2);
    out.push('"');
    let text = String::from_utf8_lossy(bytes);
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x07' => out.push_str("\\a"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            '\x0b' => out.push_str("\\v"),
            c if c.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\{:03o}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Format a logical vector into individual element strings.
#[must_use]
pub fn format_lgl_vec(vals: &[RLogical]) -> Vec<String> {
    vals.iter()
        .map(|v| match v.get() {
            Some(true) => String::from("TRUE"),
            Some(false) => String::from("FALSE"),
            None => String::from("NA"),
        })
        .collect()
}

/// Format an integer vector into individual element strings.
#[must_use]
pub fn format_int_vec(vals: &[i32]) -> Vec<String> {
    vals.iter()
        .map(|&n| {
            if n == NA_INTEGER {
                String::from("NA")
            } else {
                n.to_string()
            }
        })
        .collect()
}

/// Format a single double using the same notation rule as a one-element vector.
#[must_use]
pub fn format_real_scalar(x: f64, opts: FormatOpts) -> String {
    format_real_vec(&[x], opts).pop().unwrap_or_default()
}

/// Format a double vector with common notation and decimal precision.
#[must_use]
pub fn format_real_vec(vals: &[f64], opts: FormatOpts) -> Vec<String> {
    let precision = opts.digits.clamp(1, 22) - 1;
    let mut fixed_places = 0;
    let mut integer_width = 1;
    let mut scientific_places = 0;
    let mut exponent_width = 2;
    let mut negative = false;
    for &value in vals.iter().filter(|value| value.is_finite()) {
        let value = if value == 0.0 { 0.0 } else { value };
        negative |= value.is_sign_negative();
        let raw = format!("{value:.precision$e}");
        let (mantissa, exponent) = raw.split_once('e').unwrap_or((&raw, "0"));
        let exponent = exponent.parse::<i32>().unwrap_or(0);
        let places = mantissa
            .split_once('.')
            .map_or(0, |(_, fraction)| fraction.trim_end_matches('0').len());
        scientific_places = scientific_places.max(places);
        fixed_places = fixed_places.max((places as i32 - exponent).max(0) as usize);
        integer_width =
            integer_width.max((exponent + 1).max(1) as usize + usize::from(value < 0.0));
        if exponent.unsigned_abs() >= 100 {
            exponent_width = 3;
        }
    }
    let decimal_width = |places| places + usize::from(places > 0);
    let fixed_width = integer_width + decimal_width(fixed_places);
    let scientific_width =
        3 + exponent_width + usize::from(negative) + decimal_width(scientific_places);
    let scientific = fixed_width as i64 > scientific_width as i64 + i64::from(opts.scipen);
    vals.iter()
        .map(|&value| {
            if is_na_real(value) {
                "NA".to_owned()
            } else if is_nan_not_na(value) {
                "NaN".to_owned()
            } else if value.is_infinite() {
                if value.is_sign_negative() {
                    "-Inf"
                } else {
                    "Inf"
                }
                .to_owned()
            } else {
                let value = if value == 0.0 { 0.0 } else { value };
                if scientific {
                    let raw = format!("{value:.scientific_places$e}");
                    let (mantissa, exp) = raw.split_once('e').unwrap_or((&raw, "0"));
                    let exponent = exp.parse::<i32>().unwrap_or(0);
                    format!(
                        "{mantissa}e{}{:02}",
                        if exponent < 0 { '-' } else { '+' },
                        exponent.unsigned_abs()
                    )
                } else {
                    format!("{value:.fixed_places$}")
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chooses_notation_for_the_entire_vector() {
        let opts = FormatOpts::default();
        assert_eq!(format_real_vec(&[1.0, 1e6], opts), ["1e+00", "1e+06"]);
        assert_eq!(
            format_real_vec(&[1e5, 1.5e5, 3e5], opts),
            ["100000", "150000", "300000"]
        );
        assert_eq!(
            format_real_vec(&[0.001, 0.0015, 0.002], opts),
            ["0.0010", "0.0015", "0.0020"]
        );
        assert_eq!(format_real_scalar(1e5, opts), "1e+05");
        assert_eq!(
            format_real_scalar(1e6, FormatOpts { scipen: 2, ..opts }),
            "1000000"
        );
        assert_eq!(
            format_real_scalar(
                1e6,
                FormatOpts {
                    scipen: i32::MAX,
                    ..opts
                }
            ),
            "1000000"
        );
    }

    #[test]
    fn formats_smoke_numbers_matching_gnu_r() {
        let opts = FormatOpts {
            digits: 5,
            scipen: 0,
        };
        assert_eq!(
            format_real_vec(&[0.0, -0.125, 12.5], opts),
            vec!["0.000", "-0.125", "12.500"]
        );
        assert_eq!(format_real_scalar(2.5e-3, FormatOpts::default()), "0.0025");
        assert_eq!(encode_string(b"red\nblue"), r#""red\nblue""#);
    }
}
