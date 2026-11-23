//! Format atomic vectors and their elements.

use roxide_gc::SexpId;

use crate::api::{ValueApi, ValueError};
use crate::format::encode_string;
use crate::na::{NA_INTEGER, RComplex, is_na_real};
use crate::object::{CharId, Encoding, SexpType};

use super::{Deparser, symbol};

impl Deparser<'_> {
    pub(super) fn atomic(
        &mut self,
        x: SexpId,
        ty: SexpType,
        names: &[CharId],
        indent: usize,
    ) -> Result<(), ValueError> {
        let digits = if self.opts.digits17 { 17 } else { 15 };
        macro_rules! render {
            ($view:ident, $empty:literal, $missing:expr, $na:literal, $show:expr) => {{
                let view = self.session.$view(x)?;
                self.elements(
                    view.iter(),
                    view.len(),
                    names,
                    indent,
                    $empty,
                    $missing,
                    $na,
                    $show,
                )?;
            }};
        }
        match ty {
            SexpType::Lgl => render!(lgl, "logical", |_| false, "NA", |_, v: crate::RLogical| {
                Ok(match v.get() {
                    Some(true) => "TRUE",
                    Some(false) => "FALSE",
                    None => "NA",
                }
                .into())
            }),
            SexpType::Int => {
                let suffix = if self.opts.keep_integer { "L" } else { "" };
                render!(
                    int,
                    "integer",
                    |v| v == NA_INTEGER,
                    "NA_integer_",
                    |_, v| Ok(format!("{v}{suffix}"))
                );
            }
            SexpType::Real => render!(real, "numeric", is_na_real, "NA_real_", |_, v| Ok(
                deparse_number(v, digits)
            )),
            SexpType::Cplx => render!(
                cplx,
                "complex",
                |z: RComplex| is_na_real(z.re) && is_na_real(z.im),
                "NA_complex_",
                |_, z| Ok(complex_text(z, digits))
            ),
            SexpType::Str => {
                let values = self.session.str_slice(x)?;
                self.elements(
                    values.iter().copied(),
                    values.len(),
                    names,
                    indent,
                    "character",
                    |v| v == CharId::NA,
                    "NA_character_",
                    |this, v| this.string(v),
                )?;
            }
            SexpType::Raw => {
                let view = self.session.raw(x)?;
                if view.is_empty() {
                    self.push("raw(0)");
                } else {
                    self.push("as.raw(");
                    self.elements(
                        view.iter(),
                        view.len(),
                        names,
                        indent,
                        "raw",
                        |_| false,
                        "NA",
                        |_, v| Ok(format!("0x{v:02x}")),
                    )?;
                    self.push(")");
                }
            }
            _ => {}
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn elements<T: Copy>(
        &mut self,
        values: impl Iterator<Item = T> + Clone,
        len: usize,
        names: &[CharId],
        indent: usize,
        empty: &str,
        missing: impl Fn(T) -> bool,
        typed_na: &str,
        show: impl Fn(&Self, T) -> Result<String, ValueError>,
    ) -> Result<(), ValueError> {
        if len == 0 {
            self.push(&format!("{empty}(0)"));
            return Ok(());
        }
        let na = if self.opts.keep_na && values.clone().all(&missing) {
            typed_na
        } else {
            "NA"
        };
        let combined = len > 1 || !names.is_empty();
        if combined {
            self.push("c(");
        }
        for (i, v) in values.enumerate() {
            self.separator(i, indent);
            self.vector_name(names, i)?;
            if missing(v) {
                self.push(na);
            } else {
                self.push(&show(self, v)?);
            }
        }
        if combined {
            self.wrap(indent);
            self.push(")");
        }
        Ok(())
    }

    pub(super) fn vector_name(&mut self, names: &[CharId], i: usize) -> Result<(), ValueError> {
        if let Some(&name) = names.get(i) {
            let bytes = self.session.char_bytes(name)?;
            if !bytes.is_empty() {
                let text = String::from_utf8_lossy(bytes);
                let formatted = symbol(&text);
                if self.dput || formatted == text {
                    self.push(&formatted);
                } else {
                    self.push(&self.string(name)?);
                }
                self.push(" = ");
            }
        }
        Ok(())
    }

    pub(super) fn string(&self, id: CharId) -> Result<String, ValueError> {
        let bytes = self.session.char_bytes(id)?;
        Ok(match self.session.char_encoding(id)? {
            Encoding::Latin1 => {
                let text: String = bytes.iter().copied().map(char::from).collect();
                encode_string(text.as_bytes())
            }
            Encoding::Bytes => {
                let mut out = String::from("\"");
                for &b in bytes {
                    if b.is_ascii() {
                        let quoted = encode_string(&[b]);
                        out.push_str(&quoted[1..quoted.len() - 1]);
                    } else {
                        out.push_str(&format!("\\x{b:02x}"));
                    }
                }
                out.push('"');
                out
            }
            _ => encode_string(bytes),
        })
    }
}

pub(crate) fn deparse_number(x: f64, digits: usize) -> String {
    if !x.is_finite() {
        return if is_na_real(x) {
            "NA"
        } else if x.is_nan() {
            "NaN"
        } else if x.is_sign_negative() {
            "-Inf"
        } else {
            "Inf"
        }
        .into();
    }
    if x == 0.0 {
        return "0".into();
    }
    let precision = digits - 1;
    let sci = format!("{:.precision$e}", x.abs());
    let Some((mantissa, exponent)) = sci.split_once('e') else {
        return sci;
    };
    let exponent = exponent.parse::<i32>().unwrap_or(0);
    let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
    let sign = if x.is_sign_negative() { "-" } else { "" };
    let sci = format!(
        "{sign}{mantissa}e{}{:02}",
        if exponent < 0 { "-" } else { "+" },
        exponent.unsigned_abs()
    );
    let significant = mantissa.replace('.', "");
    let point = exponent + 1;
    let fixed_len = if point <= 0 {
        2 + point.unsigned_abs() as usize + significant.len()
    } else if point as usize >= significant.len() {
        point as usize
    } else {
        significant.len() + 1
    };
    if fixed_len + sign.len() > sci.len() {
        return sci;
    }
    let fixed = if point <= 0 {
        format!(
            "0.{}{significant}",
            "0".repeat(point.unsigned_abs() as usize)
        )
    } else if point as usize >= significant.len() {
        format!(
            "{significant}{}",
            "0".repeat(point as usize - significant.len())
        )
    } else {
        let (a, b) = significant.split_at(point as usize);
        format!("{a}.{b}")
    };
    format!("{sign}{fixed}")
}

fn complex_text(z: RComplex, digits: usize) -> String {
    let real = deparse_number(z.re, digits);
    if z.re.is_nan() || !z.im.is_finite() {
        let imaginary = deparse_number(z.im, digits);
        return format!("complex(real={real}, imaginary={imaginary})");
    }
    let sign = if z.im < 0.0 { "-" } else { "+" };
    format!("{real}{sign}{}i", deparse_number(z.im.abs(), digits))
}
