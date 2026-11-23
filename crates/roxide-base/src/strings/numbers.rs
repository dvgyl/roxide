use roxide_value::{FormatOpts, is_na_real};

/// Select a common notation and decimal precision for a numeric vector.
pub(crate) fn real_fields(values: &[f64], opts: FormatOpts, nsmall: usize) -> Vec<String> {
    let sig = opts.digits.clamp(1, 22) - 1;
    let mut fixed_places = 0;
    let mut fixed_integer = 0;
    let mut exponent_places = 0;
    let mut exponent_digits = 2;
    let mut signed = false;
    for &value in values.iter().filter(|x| x.is_finite()) {
        let rounded = format!("{:.sig$e}", value.abs());
        let Some((mantissa, exp)) = rounded.split_once('e') else {
            continue;
        };
        let exp = exp.parse::<i32>().unwrap_or(0);
        let fraction = mantissa
            .split_once('.')
            .map_or(0, |(_, rest)| rest.trim_end_matches('0').len());
        let sign = usize::from(value < 0.0);
        fixed_places = fixed_places.max((fraction as i32 - exp).max(0) as usize);
        fixed_integer = fixed_integer.max((exp + 1).max(1) as usize + sign);
        exponent_places = exponent_places.max(fraction);
        exponent_digits = exponent_digits.max(exp.unsigned_abs().to_string().len());
        signed |= sign != 0;
    }
    let scientific_width = 3
        + exponent_digits
        + exponent_places
        + usize::from(exponent_places > 0)
        + usize::from(signed);
    let fixed_width = fixed_integer + fixed_places + usize::from(fixed_places > 0);
    let scientific = fixed_width as i64 > scientific_width as i64 + i64::from(opts.scipen);
    let fixed_places = fixed_places.max(nsmall);
    values
        .iter()
        .map(|&value| {
            if is_na_real(value) {
                "NA".to_owned()
            } else if value.is_nan() {
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
                    let fields = format!("{value:.exponent_places$e}");
                    let (mantissa, exp) = fields.split_once('e').unwrap_or((&fields, "0"));
                    let power = exp.parse::<i32>().unwrap_or(0);
                    let sign = if power < 0 { '-' } else { '+' };
                    let power = power.unsigned_abs();
                    format!("{mantissa}e{sign}{power:0exponent_digits$}")
                } else {
                    format!("{value:.fixed_places$}")
                }
            }
        })
        .collect()
}

/// Format real and imaginary components with separate widths.
pub(crate) fn complex_fields(
    values: &[roxide_value::RComplex],
    opts: FormatOpts,
    nsmall: usize,
    trim: bool,
) -> Vec<String> {
    let mut real = Vec::new();
    let mut imag = Vec::new();
    for value in values
        .iter()
        .filter(|v| !is_na_real(v.re) && !is_na_real(v.im))
    {
        real.push(value.re);
        imag.push(value.im.abs());
    }
    let real = real_fields(&real, opts, nsmall);
    let imag = real_fields(&imag, opts, nsmall);
    let width = |fields: &[String]| {
        if trim {
            0
        } else {
            fields.iter().map(String::len).max().unwrap_or(0)
        }
    };
    let (rw, iw) = (width(&real), width(&imag));
    let mut fields = real.into_iter().zip(imag);
    values
        .iter()
        .map(|v| {
            if let Some((r, i)) = (!is_na_real(v.re) && !is_na_real(v.im))
                .then(|| fields.next())
                .flatten()
            {
                let sign = if v.im < 0.0 { '-' } else { '+' };
                format!("{r:>rw$}{sign}{i:>iw$}i")
            } else {
                "NA".to_owned()
            }
        })
        .collect()
}
