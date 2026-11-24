use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_value::{NA_INTEGER, NA_REAL, RLogical, SEXP_NIL, ValueApi};

use super::{attr, flag, matrix_attributes, numbers, strings};

type Color = [f64; 3];

pub(super) fn linear_to_srgb(channel: f64) -> f64 {
    if channel <= 0.0031308 {
        12.92 * channel
    } else {
        1.055 * channel.powf(1.0 / 2.4) - 0.055
    }
}

pub(super) fn srgb_to_linear(channel: f64) -> f64 {
    if channel <= 0.04045 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

// ISO/CIE 11664-4 and 11664-5 use the same lightness function.
const DELTA: f64 = 6.0 / 29.0;
const EPSILON: f64 = DELTA * DELTA * DELTA;
const KAPPA: f64 = 24389.0 / 27.0;

fn lightness_ratio(value: f64) -> f64 {
    if value > EPSILON {
        value.cbrt()
    } else {
        (KAPPA * value + 16.0) / 116.0
    }
}

fn inverse_lightness_ratio(value: f64) -> f64 {
    if value > DELTA {
        value.powi(3)
    } else {
        (116.0 * value - 16.0) / KAPPA
    }
}

pub(super) fn xyz_to_lab(xyz: Color, white: Color) -> Color {
    let [x, y, z] = std::array::from_fn(|i| lightness_ratio(xyz[i] / white[i]));
    [116.0 * y - 16.0, 500.0 * (x - y), 200.0 * (y - z)]
}

pub(super) fn lab_to_xyz([l, a, b]: Color, white: Color) -> Color {
    let y = (l + 16.0) / 116.0;
    let ratios = [y + a / 500.0, y, y - b / 200.0];
    std::array::from_fn(|i| white[i] * inverse_lightness_ratio(ratios[i]))
}

fn uv_chromaticity([x, y, z]: Color, zero_default: bool) -> [f64; 2] {
    let denominator = x + 15.0 * y + 3.0 * z;
    if zero_default && denominator == 0.0 {
        return [1.0; 2];
    }
    [4.0 * x / denominator, 9.0 * y / denominator]
}

fn xyz_to_luv(xyz: Color, white: Color) -> Color {
    let luminance = xyz[1] / white[1];
    let l = if luminance > EPSILON {
        116.0 * luminance.cbrt() - 16.0
    } else {
        KAPPA * luminance
    };
    let [u, v] = uv_chromaticity(xyz, true);
    let [white_u, white_v] = uv_chromaticity(white, false);
    [l, 13.0 * l * (u - white_u), 13.0 * l * (v - white_v)]
}

fn luv_to_xyz([l, u, v]: Color, white: Color) -> Color {
    if l == 0.0 {
        return [0.0; 3];
    }
    let y = if l > 8.0 {
        ((l + 16.0) / 116.0).powi(3)
    } else {
        l / KAPPA
    };
    let [white_u, white_v] = uv_chromaticity(white, false);
    let x_coefficient = (52.0 * l / (u + 13.0 * l * white_u) - 1.0) / 3.0;
    let y_offset = -5.0 * y;
    let v_offset = y * (39.0 * l / (v + 13.0 * l * white_v) - 5.0);
    let x = (v_offset - y_offset) / (x_coefficient + 1.0 / 3.0);
    [x, y, x * x_coefficient + y_offset]
}

pub(super) fn srgb_transfer(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let mut values = numbers(ctx, call, args.get(0))?;
    let encode = flag(ctx, call, args.get(1), "invalid sRGB transfer direction")?;
    let transfer = if encode {
        linear_to_srgb
    } else {
        srgb_to_linear
    };
    for value in &mut values {
        if !value.is_nan() {
            *value = transfer(*value);
        }
    }
    Ok(ctx.session_mut().alloc_real_from(&values)?)
}

pub(super) fn named_rows(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let names = attr(ctx, args.get(0), "row.names")?;
    let automatic = ctx.session().int(names).is_ok_and(|names| {
        let mut names = names.iter();
        names.next() == Some(NA_INTEGER) && names.next().is_some_and(|count| count < 0)
    });
    Ok(ctx
        .session_mut()
        .scalar_lgl(RLogical::from_option(Some(!automatic)))?)
}

pub(super) fn perceptual_color(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let input = args.get(0);
    let values = numbers(ctx, call, input)?;
    let white = numbers(ctx, call, args.get(1))?;
    let white = std::array::from_fn(|i| white.get(i).copied().unwrap_or(NA_REAL));
    let luv = flag(ctx, call, args.get(2), "invalid perceptual color space")?;
    let to_xyz = flag(ctx, call, args.get(3), "invalid color conversion direction")?;
    let (convert, columns): (fn(Color, Color) -> Color, _) = match (luv, to_xyz) {
        (false, false) => (xyz_to_lab, ["L", "a", "b"]),
        (true, false) => (xyz_to_luv, ["L", "u", "v"]),
        (false, true) => (lab_to_xyz, ["X", "Y", "Z"]),
        (true, true) => (luv_to_xyz, ["X", "Y", "Z"]),
    };
    let rows = values.len() / 3;
    let mut output = vec![0.0; values.len()];
    for row in 0..rows {
        let color = std::array::from_fn(|column| values[column * rows + row]);
        for (column, value) in convert(color, white).into_iter().enumerate() {
            output[column * rows + row] = value;
        }
    }
    ctx.scoped(|ctx| {
        let dimnames = attr(ctx, input, "dimnames")?;
        let row_names = if dimnames == SEXP_NIL {
            SEXP_NIL
        } else {
            ctx.session().list_get(dimnames, 0)?
        };
        let column_names = strings(ctx, columns.map(Some))?;
        ctx.protect(column_names);
        let result = ctx.session_mut().alloc_real_from(&output)?;
        matrix_attributes(ctx, call, result, rows, 3, Some([row_names, column_names]))
    })
}
