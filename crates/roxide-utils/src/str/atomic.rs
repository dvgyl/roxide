use roxide_eval::EvalResult;
use roxide_gc::SexpId;
use roxide_value::{
    CharId, Encoding, NA_INTEGER, SEXP_NIL, SexpType, ValueApi, encode_string, is_na_real,
};

use super::Display;

impl Display<'_> {
    pub(super) fn atomic(
        &self,
        x: SexpId,
        ty: SexpType,
        class: Option<&str>,
        dim: SexpId,
        names: SexpId,
        column: bool,
    ) -> EvalResult<String> {
        let s = self.ctx.session();
        let count = s.xlength(x)?;
        let v = self.options.vector;
        let many = (v as f64 * 2.5).round_ties_even() as usize;
        let few = (v as f64 * 1.25).round_ties_even() as usize;
        let header = self.options.head;
        let length = self.options.length && !column;
        let mut line = String::new();
        if header {
            line.push(' ');
            if matches!(class, Some("factor" | "ordered")) {
                let level_id = s
                    .attrs(x)?
                    .into_iter()
                    .find_map(|(key, value)| {
                        (s.sym_name(key).ok() == Some("levels")).then_some(value)
                    })
                    .unwrap_or(SEXP_NIL);
                let levels = if level_id == SEXP_NIL {
                    &[]
                } else {
                    s.str_slice(level_id)?
                };
                let name = if class == Some("ordered") {
                    "Ord.factor"
                } else {
                    "Factor"
                };
                let sep = if class == Some("ordered") { "<" } else { "," };
                let suffix = if levels.len() == 1 { "" } else { "s" };
                line.push_str(&format!("{name} w/ {} level{suffix}", levels.len()));
                let mut level_width = 0;
                let mut shown = 0;
                for &id in levels.iter().take(4) {
                    let quoted = self.quoted(id)?;
                    let width = quoted.chars().count() + 1;
                    if shown > 0 && level_width + width > 26 {
                        break;
                    }
                    line.push_str(if shown == 0 { " " } else { sep });
                    line.push_str(&quoted);
                    level_width += width;
                    shown += 1;
                }
                if levels.len() > shown {
                    line.push_str(sep);
                    line.push_str("..");
                }
                line.push(':');
                if count == 0 {
                    line.push(' ');
                    return Ok(line);
                }
            } else {
                if let Some(class) = class {
                    line.push_str(&format!("'{class}' "));
                }
                if names != SEXP_NIL && dim == SEXP_NIL {
                    line.push_str("Named ");
                }
                line.push_str(match ty {
                    SexpType::Int => "int",
                    SexpType::Lgl => "logi",
                    SexpType::Real => "num",
                    SexpType::Str => "chr",
                    SexpType::Cplx => "cplx",
                    _ => "raw",
                });
                if dim != SEXP_NIL {
                    let dims = s
                        .int(dim)?
                        .iter()
                        .map(|d| match d {
                            0 => "0 ".into(),
                            1 => "1".into(),
                            _ => format!("1:{d}"),
                        })
                        .collect::<Vec<String>>()
                        .join(", ");
                    if count != 0 {
                        line.push(' ');
                    }
                    line.push_str(&format!("[{dims}]"));
                } else if length && count == 0 {
                    line.push_str("(0)");
                } else if length && count > 1 {
                    line.push_str(&format!(" [1:{count}]"));
                } else if count > 1 {
                    line.push(' ');
                }
            }
        }
        if count == 0 {
            if header {
                line.push(' ');
            }
            return Ok(line);
        }
        if ty == SexpType::Str {
            self.characters(&mut line, x, v, header)?;
            return Ok(line);
        }
        let max_sample = self.bound / (size_of::<String>() + 32);
        let digits = self.options.digits;
        let mut limit = match ty {
            SexpType::Int => many,
            SexpType::Lgl => v.saturating_mul(3) / 2,
            SexpType::Raw => v,
            SexpType::Cplx => (v as f64 * 0.75).round_ties_even() as usize,
            SexpType::Real => many,
            _ => 0,
        };
        if limit.min(count) > max_sample {
            return Err(roxide_value::ValueError::InvalidAttribute(
                "vector size limit exceeded".into(),
            )
            .into());
        }
        let values: Vec<String> = match ty {
            SexpType::Int => s
                .int(x)?
                .iter()
                .take(limit)
                .map(|value| {
                    if value == NA_INTEGER {
                        "NA".into()
                    } else {
                        value.to_string()
                    }
                })
                .collect(),
            SexpType::Lgl => {
                let values = s.lgl(x)?;
                let pad = column
                    && count > 1
                    && values.get(0).get() == Some(true)
                    && values.iter().take(limit).all(|v| v.get().is_some())
                    && values.iter().take(limit).any(|v| v.get() == Some(false));
                if pad && header {
                    line.push(' ');
                }
                values
                    .iter()
                    .take(limit)
                    .map(|value| match value.get() {
                        Some(true) => "TRUE".into(),
                        Some(false) => "FALSE".into(),
                        None => "NA".into(),
                    })
                    .collect()
            }
            SexpType::Raw => s
                .raw(x)?
                .iter()
                .take(limit)
                .map(|value| format!("{value:02x}"))
                .collect(),
            SexpType::Cplx => s
                .cplx(x)?
                .iter()
                .take(limit)
                .map(|z| {
                    if is_na_real(z.re) || is_na_real(z.im) {
                        "NA".into()
                    } else {
                        let sign = if z.im < 0.0 { "-" } else { "+" };
                        format!(
                            "{}{sign}{}i",
                            fixed(z.re, digits),
                            fixed(z.im.abs(), digits)
                        )
                    }
                })
                .collect(),
            SexpType::Real => {
                let mut values: Vec<_> = s.real(x)?.iter().take(limit).collect();
                if values.iter().any(|&value| {
                    value.is_finite()
                        && value != 0.0
                        && fixed(value, digits)
                            .parse::<f64>()
                            .is_ok_and(|rounded| rounded != value)
                }) {
                    limit = few;
                    values.truncate(limit);
                }
                format_reals(&values, digits, self.ctx.opts().scipen)
            }
            _ => Vec::new(),
        };
        if header {
            line.push(' ');
        }
        if limit == 0 {
            if ty != SexpType::Raw {
                line.push_str("NULL");
            }
        } else {
            line.push_str(&values.join(" "));
        }
        if limit < count {
            line.push_str(" ...");
        }
        Ok(line)
    }

    pub(super) fn characters(
        &self,
        line: &mut String,
        x: SexpId,
        limit: usize,
        space: bool,
    ) -> EvalResult<()> {
        let values = self.ctx.session().str_slice(x)?;
        let mut shown = 0;
        let mut width = 0usize;
        let budget = self.options.width.saturating_sub(20);
        if space {
            line.push(' ');
        }
        for &value in values.iter().take(limit) {
            let quoted = self.quoted(value)?;
            let len = quoted.chars().count();
            if shown > 0 && width.saturating_add(1 + len) > budget {
                break;
            }
            if quoted.len().saturating_add(1) > self.bound.saturating_sub(line.len()) {
                return Err(roxide_value::ValueError::InvalidAttribute(
                    "vector size limit exceeded".into(),
                )
                .into());
            }
            if shown != 0 {
                line.push(' ');
                width += 1;
            }
            line.push_str(&quoted);
            width += len;
            shown += 1;
        }
        if shown < values.len() {
            line.push_str(" ...");
        }
        Ok(())
    }

    fn quoted(&self, id: CharId) -> EvalResult<String> {
        if id == CharId::NA {
            return Ok("NA".into());
        }
        let s = self.ctx.session();
        let data = s.char_bytes(id)?;
        let n = self.options.nchar;
        let text = if s.char_encoding(id)? == Encoding::Latin1 {
            data.iter()
                .take(n.saturating_add(1))
                .map(|&b| char::from(b))
                .collect()
        } else {
            String::from_utf8_lossy(data)
        };
        let end = text
            .char_indices()
            .nth(n)
            .map(|(i, _)| i)
            .unwrap_or(text.len());
        if end.saturating_mul(4) > self.bound {
            return Err(roxide_value::ValueError::InvalidAttribute(
                "vector size limit exceeded".into(),
            )
            .into());
        }
        let quoted = encode_string(text[..end].as_bytes());
        if end == text.len() && quoted.chars().count() <= n {
            return Ok(quoted);
        }
        if n < 17 {
            return Err(roxide_value::ValueError::InvalidAttribute(format!(
                "'nchar.max = {n}' is too small"
            ))
            .into());
        }
        let end = quoted
            .char_indices()
            .nth(n - 16)
            .map_or(quoted.len(), |(i, _)| i);
        Ok(format!("{}\"| __truncated__", &quoted[..end]))
    }
}

fn fixed(value: f64, digits: usize) -> String {
    if is_na_real(value) {
        return "NA".into();
    }
    if value.is_nan() {
        return "NaN".into();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-Inf"
        } else {
            "Inf"
        }
        .into();
    }
    if value == 0.0 {
        return "0".into();
    }
    let (mantissa, exponent) = scientific_parts(value, digits);
    let Some((head, tail)) = mantissa.split_once('.') else {
        return mantissa;
    };
    let bits = format!("{head}{tail}");
    let (sign, digits) = bits
        .strip_prefix('-')
        .map_or(("", bits.as_str()), |digits| ("-", digits));
    let point = exponent + 1;
    let text = if point <= 0 {
        format!("0.{}{digits}", "0".repeat((-point) as usize))
    } else if point as usize >= digits.len() {
        format!("{digits}{}", "0".repeat(point as usize - digits.len()))
    } else {
        let (head, tail) = digits.split_at(point as usize);
        format!("{head}.{tail}")
    };
    if text.contains('.') {
        format!("{sign}{}", text.trim_end_matches('0').trim_end_matches('.'))
    } else {
        format!("{sign}{text}")
    }
}

fn scientific_parts(value: f64, digits: usize) -> (String, i32) {
    let places = digits - 1;
    let text = format!("{value:.places$e}");
    let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
    let mantissa = if mantissa.contains('.') {
        mantissa.trim_end_matches('0').trim_end_matches('.')
    } else {
        mantissa
    };
    // Keep a decimal point so fixed conversion treats integer mantissas uniformly.
    let mantissa = if mantissa.contains('.') {
        mantissa.to_owned()
    } else {
        format!("{mantissa}.")
    };
    (mantissa, exponent.parse().unwrap_or(0))
}

fn format_reals(values: &[f64], digits: usize, scipen: i32) -> Vec<String> {
    let fixed: Vec<_> = values.iter().map(|&value| fixed(value, digits)).collect();
    let sci: Vec<_> = values
        .iter()
        .map(|&v| scientific_parts(v, digits))
        .collect();
    let places = sci
        .iter()
        .zip(values)
        .filter(|&(_, v)| v.is_finite())
        .map(|((m, _), _)| m.split_once('.').map_or(0, |(_, frac)| frac.len()))
        .max()
        .unwrap_or(0);
    let sci: Vec<_> = sci
        .into_iter()
        .zip(values)
        .zip(&fixed)
        .map(|(((m, e), &v), plain)| {
            if !v.is_finite() {
                return plain.clone();
            }
            let (head, fraction) = m.split_once('.').unwrap_or((&m, ""));
            let mantissa = if places == 0 {
                head.to_owned()
            } else {
                format!("{head}.{fraction}{}", "0".repeat(places - fraction.len()))
            };
            if v == 0.0 && places > 0 {
                mantissa.trim_start_matches('-').to_owned()
            } else {
                format!("{mantissa}e{e:+03}")
            }
        })
        .collect();
    let plain = fixed.iter().map(String::len).max().unwrap_or(0) as i64;
    let exp = sci.iter().map(String::len).max().unwrap_or(0) as i64;
    if exp.saturating_add(i64::from(scipen)) < plain {
        sci
    } else {
        fixed
    }
}
