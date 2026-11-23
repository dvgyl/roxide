use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_value::{Encoding, FormatOpts, ValueApi, format_real_vec};

use crate::distn::support::{limit, number, numbers};

pub(super) fn format(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let input = numbers(ctx, call, args.get(0))?;
    let digits = number(ctx, call, args.get(1))?.clamp(1.0, 22.0) as usize;
    let scipen = number(ctx, call, args.get(2))?.clamp(-999.0, 999.0) as i32;
    let options = FormatOpts { digits, scipen };
    let mut integer_width = 4;
    let mut decimal_width = 0;
    let mut width = digits + 7;
    for &value in &input {
        if value == 0.0 || !value.is_finite() {
            continue;
        }
        let exponent = value.abs().log10().floor() as i32;
        if exponent < -(4 + scipen) || exponent >= digits as i32 + 5 + scipen {
            continue;
        }
        let places = exponent.max(0) as usize + 1;
        // Leave space for the sign, a leading digit from rounding, and the decimal point.
        integer_width = integer_width.max(places + 2);
        decimal_width = decimal_width.max(if exponent < 0 {
            digits + exponent.unsigned_abs() as usize
        } else {
            digits.saturating_sub(places) + 1
        });
    }
    width = width.max(integer_width + decimal_width);
    limit::<u8>(
        ctx,
        call,
        input.len().saturating_mul(width + size_of::<String>()),
    )?;
    let formatted = format_real_vec(&input, options);
    strings(ctx, formatted.iter().map(String::as_str))
}

pub(super) fn strings<'a>(ctx: &mut Ctx, values: impl IntoIterator<Item = &'a str>) -> EvalResult {
    ctx.scoped(|ctx| {
        let chars = values
            .into_iter()
            .map(|s| {
                let id = ctx.session_mut().mk_char(s.as_bytes(), Encoding::Utf8)?;
                ctx.protect(id.as_sexp());
                Ok(id)
            })
            .collect::<EvalResult<Vec<_>>>()?;
        Ok(ctx.session_mut().alloc_str_from(&chars)?)
    })
}

pub(super) fn wrap(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let width = number(ctx, call, args.get(2))?.clamp(10.0, 10_000.0) as usize;
    let string = |id| -> EvalResult<String> {
        let chars = ctx.session().str_slice(id)?;
        match chars.first() {
            Some(&c) => Ok(String::from_utf8_lossy(ctx.session().char_bytes(c)?).into_owned()),
            None => Ok(String::new()),
        }
    };
    let method = string(args.get(0))?;
    let prefix = string(args.get(1))?;
    let words = method.split_whitespace().count().max(1);
    limit::<u8>(
        ctx,
        call,
        method
            .len()
            .saturating_add(words.saturating_mul(prefix.len())),
    )?;
    let indent: usize = prefix.chars().map(|c| if c == '\t' { 8 } else { 1 }).sum();
    let mut text = prefix.clone();
    let mut column = indent;
    for word in method.split_whitespace() {
        let len = word.chars().count();
        if column != indent {
            if column + len + 1 >= width {
                text.push('\n');
                text.push_str(&prefix);
                column = indent;
            } else {
                text.push(' ');
                column += 1;
            }
        }
        text.push_str(word);
        column += len;
    }
    strings(ctx, [text.as_str()])
}
