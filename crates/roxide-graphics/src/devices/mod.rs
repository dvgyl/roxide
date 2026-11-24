//! Headless vector and raster devices and their R constructors.

mod metrics;
mod output;
mod pdf;
mod raster;
mod svg;

use roxide_eval::{
    ArgEval, Args, Arity, Call, Ctx, Dispatch, EvalResult, Module, PrimDef, PrimFn, PrimKind,
    Visibility,
};
use roxide_gc::SexpId;
use roxide_host::Host;
use roxide_value::{CharId, Encoding, SEXP_NIL, SexpType, ValueApi};

use crate::engine::{Device, DeviceSpec, GraphicsExt, MAX_DEVICES, RecordingDevice, Rgba};

/// Device constructors for the `grDevices` package.
pub const MODULE: Module = Module {
    prims: &[
        constructor("rox_gr_pdf", 11, pdf),
        constructor("rox_gr_svg", 7, svg),
        constructor("rox_gr_png", 8, png),
        constructor("rox_gr_bmp", 8, bmp),
        constructor("rox_gr_jpeg", 9, jpeg),
        constructor("rox_gr_tiff", 8, tiff),
    ],
    prelude: include_str!("prelude.R"),
};

const fn constructor(name: &'static str, count: usize, fun: PrimFn) -> PrimDef {
    PrimDef {
        name,
        kind: PrimKind::Internal,
        args: ArgEval::Eval,
        arity: Arity::Exact(count),
        dispatch: Dispatch::None,
        visibility: Visibility::Off,
        fun,
    }
}

/// Open the backend selected by the device specification.
pub fn open_device(host: &mut dyn Host, spec: &DeviceSpec) -> Result<Box<dyn Device>, String> {
    match spec {
        DeviceSpec::Recording {
            name,
            width,
            height,
        } => Ok(Box::new(RecordingDevice::new(
            name.clone(),
            *width,
            *height,
        ))),
        DeviceSpec::Pdf { .. } => Ok(Box::new(pdf::PdfDevice::open(
            host,
            spec,
            pdf::Settings::default(),
            false,
        )?)),
        DeviceSpec::Svg { .. } => Ok(Box::new(svg::SvgDevice::open(spec, "sans".into())?)),
        DeviceSpec::Png { .. } => Ok(Box::new(raster::RasterDevice::open_png(
            host.limits().max_vector_bytes,
            spec,
        )?)),
    }
}

fn install(
    ctx: &mut Ctx,
    call: &Call,
    open: impl FnOnce(&mut dyn Host) -> Result<Box<dyn Device>, String>,
) -> EvalResult {
    if ctx.ext::<GraphicsExt>().open_ids().len() >= MAX_DEVICES - 1 {
        return Err(ctx.stop(call.err_call(), "too many open devices"));
    }
    let device = open(ctx.host()).map_err(|message| ctx.stop(call.err_call(), message))?;
    ctx.ext::<GraphicsExt>()
        .open_slot(device)
        .ok_or_else(|| ctx.stop(call.err_call(), "too many open devices"))?;
    Ok(SEXP_NIL)
}

fn pdf(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let in_memory = args.get(0) == SEXP_NIL;
    let spec = DeviceSpec::Pdf {
        file: if in_memory {
            String::new()
        } else {
            text(ctx, call, args.get(0), "file")?
        },
        width: number(ctx, call, args.get(1), "width")?,
        height: number(ctx, call, args.get(2), "height")?,
        onefile: flag(ctx, call, args.get(3), "onefile")?,
        compress: flag(ctx, call, args.get(4), "compress")?,
        bg: background(ctx, call, args.get(5))?,
        pointsize: number(ctx, call, args.get(6), "pointsize")?,
    };
    let settings = pdf::Settings {
        title: text(ctx, call, args.get(7), "title")?,
        kerning: flag(ctx, call, args.get(8), "useKerning")?,
        odd_even: flag(ctx, call, args.get(9), "fillOddEven")?,
    };
    let family = text(ctx, call, args.get(10), "family")?;
    if !matches!(family.as_str(), "Helvetica" | "sans" | "") {
        return Err(ctx.stop(call.err_call(), format!("unknown family '{family}'")));
    }
    install(ctx, call, |host| {
        Ok(Box::new(pdf::PdfDevice::open(
            host, &spec, settings, in_memory,
        )?))
    })
}

fn svg(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let spec = DeviceSpec::Svg {
        filename: text(ctx, call, args.get(0), "filename")?,
        width: number(ctx, call, args.get(1), "width")?,
        height: number(ctx, call, args.get(2), "height")?,
        pointsize: number(ctx, call, args.get(3), "pointsize")?,
        onefile: flag(ctx, call, args.get(4), "onefile")?,
        bg: background(ctx, call, args.get(5))?,
    };
    let family = text(ctx, call, args.get(6), "family")?;
    install(ctx, call, |_| {
        Ok(Box::new(svg::SvgDevice::open(&spec, family)?))
    })
}

fn png(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    open_raster(ctx, call, args, raster::Format::Png)
}

fn bmp(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    open_raster(ctx, call, args, raster::Format::Bmp)
}

fn jpeg(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let quality = number(ctx, call, args.get(8), "quality")?;
    let quality = if quality.is_finite() {
        quality.clamp(1.0, 100.0) as u8
    } else {
        75
    };
    open_raster(ctx, call, args, raster::Format::Jpeg { quality })
}

fn tiff(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    open_raster(ctx, call, args, raster::Format::Tiff)
}

fn open_raster(ctx: &mut Ctx, call: &Call, args: &mut Args, format: raster::Format) -> EvalResult {
    let units = text(ctx, call, args.get(3), "units")?;
    let kind = if args.get(7) == SEXP_NIL {
        let info = ctx.host().sys_info();
        let default = if matches!(info.sysname.as_str(), "macos" | "Darwin") {
            "quartz"
        } else {
            "cairo"
        };
        default.to_owned()
    } else {
        text(ctx, call, args.get(7), "type")?
    };
    let mut res = number(ctx, call, args.get(6), "res")?;
    let has_resolution = !res.is_nan();
    if res.is_nan() {
        if units != "px" {
            return Err(ctx.stop(
                call.err_call(),
                "'res' must be specified unless 'units = \"px\"'",
            ));
        }
        res = 72.0;
    }
    let factor = match units.as_str() {
        "px" => 1.0,
        "in" => res,
        "cm" => res / 2.54,
        "mm" => res / 25.4,
        _ => return Err(ctx.stop(call.err_call(), "invalid 'units' argument")),
    };
    let dimension = |ctx: &mut Ctx, index, name| -> EvalResult<u32> {
        let pixels = (number(ctx, call, args.get(index), name)? * factor).trunc();
        if !(1.0..=f64::from(i32::MAX)).contains(&pixels) {
            return Err(ctx.stop(call.err_call(), format!("invalid '{name}' argument")));
        }
        Ok(pixels as u32)
    };
    let device_name = match kind.as_str() {
        "quartz" => "quartz_off_screen",
        "cairo-png" if format == raster::Format::Png => "png",
        "cairo" | "Xlib" => format.name(),
        _ => return Err(ctx.stop(call.err_call(), "invalid 'type' argument")),
    };
    let spec = raster::Spec {
        filename: text(ctx, call, args.get(0), "filename")?,
        width: dimension(ctx, 1, "width")?,
        height: dimension(ctx, 2, "height")?,
        pointsize: number(ctx, call, args.get(4), "pointsize")?,
        bg: crate::grdevices::parse_rgba(ctx, args.get(5))?,
        ppi: res,
        has_resolution,
        name: device_name.into(),
        format,
    };
    let used = ctx.ext::<GraphicsExt>().framebuffer_bytes();
    let remaining = ctx.host().limits().max_vector_bytes.saturating_sub(used);
    install(ctx, call, |_| {
        Ok(Box::new(raster::RasterDevice::open(remaining, spec)?))
    })
}

fn number(ctx: &mut Ctx, call: &Call, value: SexpId, name: &str) -> EvalResult<f64> {
    let (value, _) = ctx.session_mut().coerce(value, SexpType::Real)?;
    let values = ctx.session().real(value)?;
    if let Some(number) = values.iter().next() {
        Ok(number)
    } else {
        Err(ctx.stop(call.err_call(), format!("invalid '{name}' argument")))
    }
}

fn flag(ctx: &mut Ctx, call: &Call, value: SexpId, name: &str) -> EvalResult<bool> {
    let (value, _) = ctx.session_mut().coerce(value, SexpType::Lgl)?;
    let first = ctx
        .session()
        .lgl(value)?
        .iter()
        .next()
        .and_then(|v| v.get());
    first.ok_or_else(|| ctx.stop(call.err_call(), format!("invalid '{name}' argument")))
}

fn text(ctx: &mut Ctx, call: &Call, value: SexpId, name: &str) -> EvalResult<String> {
    if ctx.session().type_of(value)? == SexpType::Str
        && let Some(&id) = ctx.session().str_slice(value)?.first()
        && id != CharId::NA
    {
        let data = ctx.session().char_bytes(id)?;
        return Ok(if ctx.session().char_encoding(id)? == Encoding::Latin1 {
            data.iter().copied().map(char::from).collect()
        } else {
            String::from_utf8_lossy(data).into_owned()
        });
    }
    Err(ctx.stop(call.err_call(), format!("invalid '{name}' argument")))
}

fn background(ctx: &mut Ctx, call: &Call, value: SexpId) -> EvalResult<Rgba> {
    let color = text(ctx, call, value, "bg")?.to_ascii_lowercase();
    if let Some(hex) = color.strip_prefix('#')
        && matches!(hex.len(), 6 | 8)
        && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
        && let Ok(mut rgba) = u32::from_str_radix(hex, 16)
    {
        if hex.len() == 6 {
            rgba = (rgba << 8) | 255;
        }
        let [r, g, b, a] = rgba.to_be_bytes();
        return Ok(Rgba::new(r, g, b, a));
    }
    if let Some(level) = color
        .strip_prefix("gray")
        .or_else(|| color.strip_prefix("grey"))
        && let Ok(level) = level.parse::<u16>()
        && level <= 100
    {
        // Match the 8-bit gray levels used by R.
        let gray = (f64::from(level) * 2.55).round() as u8;
        return Ok(Rgba::new(gray, gray, gray, 255));
    }
    let [r, g, b] = match color.as_str() {
        "transparent" => return Ok(Rgba::TRANSPARENT),
        "black" => [0, 0, 0],
        "white" => [255, 255, 255],
        "red" => [255, 0, 0],
        "green" => [0, 255, 0],
        "blue" => [0, 0, 255],
        "yellow" => [255, 255, 0],
        "cyan" => [0, 255, 255],
        "magenta" => [255, 0, 255],
        "gray" | "grey" => [190, 190, 190],
        _ => return Err(ctx.stop(call.err_call(), format!("invalid color name '{color}'"))),
    };
    Ok(Rgba::new(r, g, b, 255))
}
