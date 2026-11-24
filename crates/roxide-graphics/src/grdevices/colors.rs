use std::collections::HashSet;

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{CharId, SEXP_NIL, SexpType, ValueApi, is_na_real};

use crate::engine::{GraphicsExt, Rgba};

use super::color_spaces::{lab_to_xyz, linear_to_srgb, srgb_to_linear, xyz_to_lab};
use super::{
    attr, flag, limit, matrix_attributes, number, numbers, optional_numbers, recycled, set_attr,
    strings,
};

// Keep the order returned by colors() in GNU R 4.6.2. col2rgb() gives each RGB value.
pub(super) fn catalog() -> impl Iterator<Item = (&'static str, &'static str)> {
    include_str!("named_colors.tsv")
        .lines()
        .filter_map(|line| line.split_once('\t'))
}

pub(super) fn hex(color: Rgba, alpha: bool) -> String {
    let Rgba { r, g, b, a } = color;
    if alpha {
        format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
    } else {
        format!("#{r:02X}{g:02X}{b:02X}")
    }
}

pub(super) fn decode_hex(hex: &str) -> Result<Rgba, &'static str> {
    let size = hex.len();
    if !matches!(size, 3 | 4 | 6 | 8) {
        return Err("invalid RGB specification");
    }
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("invalid hex digit in 'color' or 'lty'");
    }
    let value = u32::from_str_radix(hex, 16).map_err(|_| "invalid RGB specification")?;
    let (bits, mask, scale) = if size <= 4 { (4, 15, 17) } else { (8, 255, 1) };
    let rgba = if size.is_multiple_of(3) {
        (value << bits) | mask
    } else {
        value
    };
    let channel = |shift: u32| (((rgba >> (shift * bits)) & mask) * scale) as u8;
    Ok(Rgba::new(channel(3), channel(2), channel(1), channel(0)))
}

pub(super) fn literal(text: &str) -> Result<Rgba, String> {
    if matches!(text, "transparent" | "NA") {
        return Ok(Rgba::TRANSPARENT);
    }
    if let Some(code) = text.strip_prefix('#') {
        return decode_hex(code).map_err(str::to_owned);
    }
    let normalized: String = text
        .chars()
        .filter(|&ch| ch != ' ')
        .map(|ch| ch.to_ascii_lowercase())
        .collect();
    catalog()
        .find(|&(name, _)| name == normalized)
        .ok_or_else(|| format!("invalid color name '{text}'"))
        .and_then(|(_, code)| decode_hex(code).map_err(str::to_owned))
}

fn indexed(palette: &[String], index: f64, background: Option<Rgba>) -> Result<Rgba, String> {
    if !index.is_finite() || index >= 2_147_483_648.0 || index <= -2_147_483_648.0 {
        return Ok(Rgba::TRANSPARENT);
    }
    let index = index as i32;
    match index {
        0 => background.ok_or_else(|| "numerical color values must be positive".to_owned()),
        i if i < 0 => Err(format!("numerical color values must be >= 0, found {i}")),
        i => palette
            .get((i as usize - 1) % palette.len().max(1))
            .map_or(Ok(Rgba::BLACK), |color| literal(color)),
    }
}

fn text_color(text: &str, palette: &[String], background: Option<Rgba>) -> Result<Rgba, String> {
    if text.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        let index: f64 = text
            .parse()
            .map_err(|_| format!("invalid color specification \"{text}\""))?;
        indexed(palette, index, background)
    } else {
        literal(text)
    }
}

/// Resolve the first R color value using the active palette and device background convention.
pub fn parse_rgba(ctx: &mut Ctx, val: SexpId) -> EvalResult<Rgba> {
    let palette = ctx.ext::<GraphicsExt>().palette.clone();
    if ctx.session().xlength(val)? == 0 {
        return Ok(Rgba::TRANSPARENT);
    }
    let background = Some(Rgba::TRANSPARENT);
    let result = match ctx.session().type_of(val)? {
        SexpType::Int | SexpType::Real => indexed(&palette, number(ctx, val)?, background),
        _ => {
            let (val, _) = ctx.session_mut().coerce(val, SexpType::Str)?;
            let id = ctx
                .session()
                .str_slice(val)?
                .first()
                .copied()
                .unwrap_or(CharId::NA);
            if id == CharId::NA {
                Ok(Rgba::TRANSPARENT)
            } else {
                let text = String::from_utf8_lossy(ctx.session().char_bytes(id)?);
                text_color(&text, &palette, background)
            }
        }
    };
    result.map_err(|message| ctx.stop(SEXP_NIL, message))
}

pub(super) fn named_colors(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let distinct = flag(ctx, call, args.get(0), "invalid 'distinct' value")?;
    let mut seen = HashSet::new();
    strings(
        ctx,
        catalog().filter_map(|(name, code)| (!distinct || seen.insert(code)).then_some(Some(name))),
    )
}

pub(super) fn col2rgb(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let input = args.get(0);
    let alpha = flag(ctx, call, args.get(1), "invalid 'alpha' value")?;
    let row_names = ["red", "green", "blue", "alpha"];
    let rows = if alpha { 4 } else { 3 };
    let len = ctx.session().xlength(input)?;
    limit::<i32>(ctx, call, len.saturating_mul(rows))?;
    let palette = ctx.ext::<GraphicsExt>().palette.clone();
    let mut output = Vec::with_capacity(len * rows);
    let mut append = |color: Rgba| {
        output.extend(
            [color.r, color.g, color.b, color.a][..rows]
                .iter()
                .copied()
                .map(i32::from),
        );
    };
    if matches!(
        ctx.session().type_of(input)?,
        SexpType::Int | SexpType::Real
    ) {
        let values = numbers(ctx, call, input)?;
        if values
            .iter()
            .any(|&x| x >= 2_147_483_648.0 || x <= -2_147_483_648.0)
        {
            ctx.warn_now(
                call.err_call(),
                "NAs introduced by coercion to integer range",
            )?;
        }
        for value in values {
            // Sub-unit values entered through double vectors coerce to transparent.
            let color = if value != 0.0 && value.abs() < 1.0 {
                Ok(Rgba::TRANSPARENT)
            } else {
                indexed(&palette, value, None)
            };
            append(color.map_err(|msg| ctx.stop(call.err_call(), msg))?);
        }
    } else {
        let (value, _) = ctx.session_mut().coerce(input, SexpType::Str)?;
        let mut error = None;
        for &id in ctx.session().str_slice(value)? {
            let color = if id == CharId::NA {
                Ok(Rgba::TRANSPARENT)
            } else {
                let text = String::from_utf8_lossy(ctx.session().char_bytes(id)?);
                text_color(&text, &palette, None)
            };
            match color {
                Ok(color) => append(color),
                Err(message) => {
                    error = Some(message);
                    break;
                }
            }
        }
        if let Some(message) = error {
            return Err(ctx.stop(call.err_call(), message));
        }
    }
    ctx.scoped(|ctx| {
        let col_names = attr(ctx, input, "names")?;
        let result = ctx.session_mut().alloc_int_from(&output)?;
        ctx.protect(result);
        let row_names = strings(ctx, row_names[..rows].iter().copied().map(Some))?;
        matrix_attributes(ctx, call, result, rows, len, Some([row_names, col_names]))
    })
}

fn format_number(value: f64) -> String {
    if is_na_real(value) {
        "NA".to_owned()
    } else if value.is_nan() {
        "NaN".to_owned()
    } else if value == f64::INFINITY {
        "Inf".to_owned()
    } else if value == f64::NEG_INFINITY {
        "-Inf".to_owned()
    } else {
        value.to_string()
    }
}

pub(super) fn quantize(value: f64) -> u8 {
    (value * 255.0 + 0.5) as u8
}

pub(super) fn alpha_byte(value: f64) -> Result<u8, String> {
    if (0.0..=1.0).contains(&value) {
        Ok(quantize(value))
    } else {
        Err(format!(
            "alpha level {}, not in [0,1]",
            format_number(value)
        ))
    }
}

fn rgb_channel(value: f64, max: f64, alpha: bool) -> Result<u8, String> {
    let name = if alpha {
        "alpha level"
    } else {
        "color intensity"
    };
    if max == 255.0 {
        let truncated = value.trunc();
        if (0.0..=255.0).contains(&truncated) {
            return Ok(truncated as u8);
        }
        return Err(format!("{name} {}, not in 0:255", format_number(truncated)));
    }
    let fraction = value / max;
    if (0.0..=1.0).contains(&fraction) {
        Ok(quantize(fraction))
    } else {
        Err(format!("{name} {}, not in [0,1]", format_number(fraction)))
    }
}

fn evaluate_colors(
    ctx: &mut Ctx,
    call: &Call,
    count: usize,
    mut get: impl FnMut(usize) -> Result<Option<String>, String>,
) -> EvalResult {
    limit::<String>(ctx, call, count)?;
    let output: Vec<_> = (0..count)
        .map(&mut get)
        .collect::<Result<_, _>>()
        .map_err(|message| ctx.stop(call.err_call(), message))?;
    strings(ctx, output.iter().map(Option::as_deref))
}

fn count(channels: &[&[f64]], alpha: &[f64]) -> usize {
    if channels.iter().any(|component| component.is_empty()) {
        0
    } else {
        channels
            .iter()
            .map(|component| component.len())
            .max()
            .unwrap_or(0)
            .max(alpha.len())
    }
}

pub(super) fn rgb(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let red = numbers(ctx, call, args.get(0))?;
    let green = numbers(ctx, call, args.get(1))?;
    let blue = numbers(ctx, call, args.get(2))?;
    let alpha = optional_numbers(ctx, call, args.get(3))?;
    let names = args.get(4);
    let max = number(ctx, args.get(5))?;
    if !max.is_finite() || max <= 0.0 {
        return Err(ctx.stop(call.err_call(), "invalid value of 'maxColorValue'"));
    }
    let len = count(&[&red, &green, &blue], &alpha);
    if len != 0
        && names != SEXP_NIL
        && (ctx.session().type_of(names)? != SexpType::Str || ctx.session().xlength(names)? != len)
    {
        return Err(ctx.stop(call.err_call(), "invalid 'names' vector"));
    }
    ctx.scoped(|ctx| {
        let result = evaluate_colors(ctx, call, len, |i| {
            let r = rgb_channel(red[i % red.len()], max, false)?;
            let g = rgb_channel(green[i % green.len()], max, false)?;
            let b = rgb_channel(blue[i % blue.len()], max, false)?;
            let a = rgb_channel(recycled(&alpha, i, max), max, true)?;
            Ok(Some(hex(Rgba::new(r, g, b, a), !alpha.is_empty())))
        })?;
        if len != 0 {
            ctx.protect(result);
            set_attr(ctx, result, "names", names)?;
        }
        Ok(result)
    })
}

pub(super) fn hsv_color(h: f64, s: f64, v: f64, alpha: u8) -> Rgba {
    let hue = if h == 1.0 { 0.0 } else { h * 6.0 };
    let fraction = hue - hue.floor();
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * fraction);
    let t = v * (1.0 - s * (1.0 - fraction));
    let [r, g, b] = match hue as u8 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    };
    Rgba::new(quantize(r), quantize(g), quantize(b), alpha)
}

pub(super) fn hsv(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let h = numbers(ctx, call, args.get(0))?;
    let s = numbers(ctx, call, args.get(1))?;
    let v = numbers(ctx, call, args.get(2))?;
    let alpha = optional_numbers(ctx, call, args.get(3))?;
    evaluate_colors(ctx, call, count(&[&h, &s, &v], &alpha), |i| {
        let (h, s, v) = (h[i % h.len()], s[i % s.len()], v[i % v.len()]);
        if [h, s, v].iter().any(|v| !v.is_finite()) {
            return Err("inputs must be finite".to_owned());
        }
        if [h, s, v].iter().any(|v| !(0.0..=1.0).contains(v)) {
            return Err("invalid hsv color".to_owned());
        }
        let a = alpha_byte(recycled(&alpha, i, 1.0))?;
        Ok(Some(hex(hsv_color(h, s, v, a), !alpha.is_empty())))
    })
}

const WHITE: [f64; 3] = [0.95047, 1.0, 1.08883];

fn xyz_to_rgb([x, y, z]: [f64; 3]) -> [f64; 3] {
    [
        3.240479 * x - 1.537150 * y - 0.498535 * z,
        -0.969256 * x + 1.875992 * y + 0.041556 * z,
        0.055648 * x - 0.204043 * y + 1.057311 * z,
    ]
    .map(linear_to_srgb)
}

pub(super) fn hcl_rgb(h: f64, c: f64, l: f64) -> [f64; 3] {
    if l == 0.0 {
        return [0.0; 3];
    }
    let h = h.rem_euclid(360.0).to_radians();
    let [x, y, z] = WHITE;
    let divisor = x + 15.0 * y + 3.0 * z;
    let u = 4.0 * x / divisor + c * h.cos() / (13.0 * l);
    let v = 9.0 * y / divisor + c * h.sin() / (13.0 * l);
    let y = if l <= 8.0 {
        l * (3.0_f64 / 29.0).powi(3)
    } else {
        ((l + 16.0) / 116.0).powi(3)
    };
    let x = y * 9.0 * u / (4.0 * v);
    let z = y * (12.0 - 3.0 * u - 20.0 * v) / (4.0 * v);
    xyz_to_rgb([x, y, z])
}

pub(super) fn hcl(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let h = numbers(ctx, call, args.get(0))?;
    let c = numbers(ctx, call, args.get(1))?;
    let l = numbers(ctx, call, args.get(2))?;
    let alpha = optional_numbers(ctx, call, args.get(3))?;
    let fixup_value = args.get(4);
    let (fixup_value, _) = ctx.session_mut().coerce(fixup_value, SexpType::Lgl)?;
    let fixup = ctx
        .session()
        .lgl(fixup_value)?
        .iter()
        .next()
        .and_then(|x| x.get())
        != Some(false);
    evaluate_colors(ctx, call, count(&[&h, &c, &l], &alpha), |i| {
        let (h, c, l) = (h[i % h.len()], c[i % c.len()], l[i % l.len()]);
        if [h, c, l].iter().any(|v| !v.is_finite()) {
            return Ok(None);
        }
        let a = recycled(&alpha, i, 1.0);
        if c < 0.0 || !(0.0..=100.0).contains(&l) || (a.is_finite() && !(0.0..=1.0).contains(&a)) {
            return Err("invalid hcl color".to_owned());
        }
        let rgb = hcl_rgb(h, c, l);
        let channels = rgb.map(|x| (255.0 * x + 0.5) as i32);
        if !fixup && channels.iter().any(|&v| !(0..=255).contains(&v)) {
            return Ok(None);
        }
        let [r, g, b] = channels.map(|v| v.clamp(0, 255) as u8);
        Ok(Some(hex(
            Rgba::new(r, g, b, if a.is_finite() { quantize(a) } else { 255 }),
            !alpha.is_empty(),
        )))
    })
}

pub(super) fn gray(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let level = numbers(ctx, call, args.get(0))?;
    let alpha = optional_numbers(ctx, call, args.get(1))?;
    evaluate_colors(ctx, call, count(&[&level], &alpha), |i| {
        let gray = level[i % level.len()];
        if !(0.0..=1.0).contains(&gray) {
            return Err("invalid gray level, must be in [0,1].".to_owned());
        }
        let g = quantize(gray);
        let a = if alpha.is_empty() && args.get(1) != SEXP_NIL {
            0
        } else {
            alpha_byte(recycled(&alpha, i, 1.0))?
        };
        Ok(Some(hex(Rgba::new(g, g, g, a), args.get(1) != SEXP_NIL)))
    })
}

const LAB_WHITE: [f64; 3] = [0.953205712549377, 1.0, 1.08538438164692];

fn rgb_to_lab(rgb: [f64; 3]) -> [f64; 3] {
    let [r, g, b] = rgb.map(srgb_to_linear);
    let xyz = [
        0.416821341885317 * r + 0.356576717077975 * g + 0.179807653586085 * b,
        0.214923504409617 * r + 0.713153434155949 * g + 0.0719230614344342 * b,
        0.0195385004008742 * r + 0.118858905692658 * g + 0.946986975553383 * b,
    ];
    xyz_to_lab(xyz, LAB_WHITE)
}

fn lab_to_rgb([l, a, b]: [f64; 3]) -> [f64; 3] {
    if [l, a, b].iter().any(|v| v.is_nan()) {
        return [roxide_value::NA_REAL; 3];
    }
    let [x, y, z] = lab_to_xyz([l, a, b], LAB_WHITE);
    [
        3.206520517144467 * x - 1.52104178377366 * y - 0.493310848791455 * z,
        -0.971982546201235 * x + 1.88126865160849 * y + 0.0416724845995893 * z,
        0.0558383385930981 * x - 0.204740574841359 * y + 1.060928433268859 * z,
    ]
    .map(|channel| (linear_to_srgb(channel) * 100_000.0).round() / 100_000.0)
}

pub(super) fn lab(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let input = numbers(ctx, call, args.get(0))?;
    let to_lab = flag(ctx, call, args.get(1), "invalid color space")?;
    let n = input.len() / 3;
    let mut output = vec![0.0; input.len()];
    for i in 0..n {
        let rgb = [input[i], input[i + n], input[i + 2 * n]];
        let values = if to_lab {
            rgb_to_lab(rgb)
        } else {
            lab_to_rgb(rgb)
        };
        for (channel, value) in values.into_iter().enumerate() {
            output[i + channel * n] = value;
        }
    }
    let result = ctx.session_mut().alloc_real_from(&output)?;
    matrix_attributes(ctx, call, result, n, 3, None)
}
