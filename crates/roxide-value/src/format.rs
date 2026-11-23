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

/// Format a single double in minimal R scalar form (used by `cat()` and `dput()`).
#[must_use]
pub fn format_real_scalar(x: f64, opts: FormatOpts) -> String {
    if is_na_real(x) {
        return String::from("NA");
    }
    if is_nan_not_na(x) {
        return String::from("NaN");
    }
    if x.is_infinite() {
        return if x.is_sign_negative() {
            String::from("-Inf")
        } else {
            String::from("Inf")
        };
    }
    if x == 0.0 {
        return String::from("0");
    }
    let digits = opts.digits.clamp(1, 22);
    let abs = x.abs();
    let exp = abs.log10().floor() as i32;
    // Choose fixed vs scientific matching R's scipen heuristic.
    let use_sci = exp < -(4 + opts.scipen) || exp >= (digits as i32 + 5 + opts.scipen);
    if use_sci {
        let prec = digits.saturating_sub(1);
        let raw = format!("{x:.prec$e}");
        trim_sci_zeros(&raw)
    } else {
        let dec = if exp < 0 {
            ((-exp) as usize) + digits - 1
        } else {
            digits.saturating_sub((exp as usize) + 1)
        };
        let raw = format!("{x:.dec$}");
        trim_fixed_zeros(&raw)
    }
}

/// Format a double vector with R's aligned fixed-decimal width across elements (used by `print()`).
#[must_use]
pub fn format_real_vec(vals: &[f64], opts: FormatOpts) -> Vec<String> {
    if vals.len() <= 1 {
        return vals.iter().map(|&x| format_real_scalar(x, opts)).collect();
    }
    let digits = opts.digits.clamp(1, 22);
    // Determine shared fixed decimal places across finite entries when using fixed mode.
    let mut max_dec = 0usize;
    let mut any_sci = false;
    for &x in vals {
        if !x.is_finite() || x == 0.0 {
            continue;
        }
        let exp = x.abs().log10().floor() as i32;
        if exp < -(4 + opts.scipen) || exp >= (digits as i32 + 5 + opts.scipen) {
            any_sci = true;
            break;
        }
        let s = format_real_scalar(x, opts);
        if let Some(dot) = s.find('.') {
            max_dec = max_dec.max(s.len() - dot - 1);
        }
    }
    if any_sci {
        return vals.iter().map(|&x| format_real_scalar(x, opts)).collect();
    }
    vals.iter()
        .map(|&x| {
            if is_na_real(x) {
                String::from("NA")
            } else if is_nan_not_na(x) {
                String::from("NaN")
            } else if x.is_infinite() {
                if x.is_sign_negative() {
                    String::from("-Inf")
                } else {
                    String::from("Inf")
                }
            } else if max_dec == 0 {
                let v = if x == 0.0 { 0.0 } else { x };
                format!("{v:.0}")
            } else {
                let v = if x == 0.0 { 0.0 } else { x };
                format!("{v:.max_dec$}")
            }
        })
        .collect()
}

fn trim_fixed_zeros(s: &str) -> String {
    if !s.contains('.') {
        return s.to_owned();
    }
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    if trimmed == "-0" {
        String::from("0")
    } else {
        trimmed.to_owned()
    }
}

fn trim_sci_zeros(s: &str) -> String {
    let Some((mant, exp)) = s.split_once('e') else {
        return s.to_owned();
    };
    let clean_mant = trim_fixed_zeros(mant);
    let exp_num: i32 = exp.parse().unwrap_or(0);
    if exp_num < 0 {
        format!("{clean_mant}e-{:02}", exp_num.unsigned_abs())
    } else {
        format!("{clean_mant}e+{exp_num:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
