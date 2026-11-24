//! Raster device state and conversion from device points to pixels.

mod bmp;
mod fill;
mod jpeg;
mod png;
mod stroke;
mod tiff;

use std::io::{BufWriter, Write as _};

use roxide_host::{Host, HostError, HostResult, OpenMode};

use super::{
    metrics,
    output::{self, PageName, Point, valid_point},
};
use crate::engine::{Device, DeviceSpec, GContext, LineEnd, LineJoin, Rect, Rgba};
use fill::Frame;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Format {
    Png,
    Bmp,
    Jpeg { quality: u8 },
    Tiff,
}

impl Format {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Bmp => "bmp",
            Self::Jpeg { .. } => "jpeg",
            Self::Tiff => "tiff",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Bmp => "BMP",
            Self::Jpeg { .. } => "JPEG",
            Self::Tiff => "TIFF",
        }
    }
}

pub(super) struct Spec {
    pub(super) filename: String,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) ppi: f64,
    pub(super) has_resolution: bool,
    pub(super) pointsize: f64,
    pub(super) bg: Rgba,
    pub(super) name: String,
    pub(super) format: Format,
}

pub(super) struct RasterDevice {
    file: PageName,
    frame: Frame,
    bg: Rgba,
    ppi: f64,
    has_resolution: bool,
    scale: f64,
    pointsize: f64,
    name: String,
    format: Format,
    index: usize,
    started: bool,
    closed: bool,
}

impl RasterDevice {
    pub(super) fn open_png(max_bytes: usize, spec: &DeviceSpec) -> Result<Self, String> {
        let DeviceSpec::Png {
            filename,
            width_px,
            height_px,
            res,
            bg,
            pointsize,
            device_name,
        } = spec
        else {
            return Err("invalid PNG device specification".into());
        };
        Self::open(
            max_bytes,
            Spec {
                filename: filename.clone(),
                width: *width_px,
                height: *height_px,
                ppi: *res,
                has_resolution: true,
                pointsize: *pointsize,
                bg: *bg,
                name: device_name.clone(),
                format: Format::Png,
            },
        )
    }

    pub(super) fn open(max_bytes: usize, spec: Spec) -> Result<Self, String> {
        let Spec {
            filename,
            width,
            height,
            ppi,
            has_resolution,
            pointsize,
            bg,
            name,
            format,
        } = spec;
        let label = format.label();
        let max_dimension = if matches!(format, Format::Jpeg { .. }) {
            u32::from(u16::MAX)
        } else {
            i32::MAX as u32
        };
        if width == 0 || height == 0 || width > max_dimension || height > max_dimension {
            return Err(format!("invalid {label} dimensions"));
        }
        if !ppi.is_finite()
            || ppi <= 0.0
            || ppi / 0.0254 > f64::from(u32::MAX)
            || !pointsize.is_finite()
            || pointsize <= 0.0
        {
            return Err(format!("invalid {label} resolution or pointsize"));
        }
        let count = (width as usize)
            .checked_mul(height as usize)
            .filter(|count| *count <= max_bytes / 4)
            .ok_or_else(|| format!("{label} framebuffer exceeds the host vector limit"))?;
        match format {
            Format::Bmp => bmp::image_size(width, height),
            Format::Tiff => tiff::image_size(width, height),
            _ => Ok(0),
        }
        .map_err(|error| error.to_string())?;
        let file = PageName::new(&filename, true)?;
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(count)
            .map_err(|_| format!("cannot allocate {label} framebuffer"))?;
        pixels.resize(count, [0; 4]);
        let mut frame = Frame {
            width,
            height,
            pixels,
            clip: Rect::normalized(0.0, 0.0, 0.0, 0.0),
        };
        frame.reset(bg);
        Ok(Self {
            file,
            frame,
            bg,
            ppi,
            has_resolution,
            scale: ppi / 72.0,
            pointsize,
            name,
            format,
            index: 1,
            started: false,
            closed: false,
        })
    }

    fn flush(&self, host: &mut dyn Host) -> HostResult<()> {
        let name = self.file.at(self.index);
        let path = output::path(host, &name);
        let mut file = BufWriter::new(host.open(&path, OpenMode::write_truncate())?);
        let density = self.has_resolution.then_some(self.ppi);
        match self.format {
            Format::Bmp => bmp::encode(&mut file, &self.frame, self.ppi),
            Format::Jpeg { quality } => jpeg::encode(&mut file, &self.frame, density, quality),
            Format::Tiff => tiff::encode(&mut file, &self.frame, density),
            Format::Png => png::encode(&mut file, &self.frame, self.ppi),
        }
        .and_then(|()| file.flush())
        .map_err(HostError::from_io)
    }

    fn point(&self, (x, y): Point) -> Point {
        (x * self.scale, y * self.scale)
    }

    fn paint(&mut self, paths: &[&[Point]], closed: bool, winding: bool, gc: &GContext) {
        if self.closed {
            return;
        }
        self.started = true;
        if closed {
            self.frame.fill(paths, winding, gc.fill);
        }
        stroke::paint(&mut self.frame, paths, closed, self.scale, gc);
    }
}

impl Device for RasterDevice {
    fn framebuffer_bytes(&self) -> usize {
        self.frame.pixels.capacity() * size_of::<[u8; 4]>()
    }
    fn name(&self) -> &str {
        &self.name
    }

    fn size_in(&self) -> (f64, f64) {
        (
            f64::from(self.frame.width) / self.ppi,
            f64::from(self.frame.height) / self.ppi,
        )
    }

    fn size_px(&self) -> (f64, f64) {
        (f64::from(self.frame.width), f64::from(self.frame.height))
    }

    fn initial_background(&self) -> Option<Rgba> {
        Some(self.bg)
    }

    fn new_page(&mut self, host: &mut dyn Host, bg: Rgba) -> HostResult<()> {
        if self.closed {
            return Err(output::invalid("graphics device is closed"));
        }
        if self.started {
            self.flush(host)?;
            self.index += 1;
        }
        self.frame.reset(bg);
        self.started = true;
        Ok(())
    }

    fn clip(&mut self, r: Rect) {
        let a = self.point((r.x0, r.y0));
        let b = self.point((r.x1, r.y1));
        if !valid_point(a) || !valid_point(b) {
            return;
        }
        let width = f64::from(self.frame.width);
        let height = f64::from(self.frame.height);
        self.frame.clip = Rect::normalized(
            a.0.clamp(0.0, width),
            a.1.clamp(0.0, height),
            b.0.clamp(0.0, width),
            b.1.clamp(0.0, height),
        );
    }

    fn polyline(&mut self, pts: &[Point], gc: &GContext) {
        if pts.len() < 2 {
            return;
        }
        let scaled: Vec<_> = pts.iter().map(|&p| self.point(p)).collect();
        self.paint(&[&scaled], false, true, gc);
    }

    fn polygon(&mut self, pts: &[Point], gc: &GContext) {
        self.path(&[pts], true, gc);
    }

    fn path(&mut self, subpaths: &[&[Point]], winding: bool, gc: &GContext) {
        let scaled: Vec<Vec<_>> = subpaths
            .iter()
            .filter(|pts| pts.len() >= 2)
            .map(|pts| pts.iter().map(|&p| self.point(p)).collect())
            .collect();
        let paths: Vec<_> = scaled.iter().map(Vec::as_slice).collect();
        self.paint(&paths, true, winding, gc);
    }

    fn rect(&mut self, r: Rect, gc: &GContext) {
        self.polygon(
            &[(r.x0, r.y0), (r.x1, r.y0), (r.x1, r.y1), (r.x0, r.y1)],
            gc,
        );
    }

    fn circle(&mut self, center: Point, r: f64, gc: &GContext) {
        let radius = r * self.scale;
        let center = self.point(center);
        if !radius.is_finite() || radius <= 0.0 || !valid_point(center) {
            return;
        }
        let points = stroke::disk(center, radius);
        self.paint(&[&points], true, true, gc);
    }

    fn text(&mut self, at: Point, s: &str, rot: f64, hadj: f64, gc: &GContext) {
        let size = metrics::size(gc, self.pointsize);
        let origin = metrics::text_origin(at, self.str_width(s, gc), rot, hadj);
        if self.closed
            || s.is_empty()
            || gc.col.is_transparent()
            || !valid_point(origin)
            || !rot.is_finite()
            || !size.is_finite()
            || size <= 0.0
        {
            return;
        }
        let (sin, cos) = (rot % 360.0).to_radians().sin_cos();
        let italic = matches!(gc.fontface, 3 | 4);
        let bold = matches!(gc.fontface, 2 | 4);
        let pen = GContext {
            col: gc.col,
            lwd: size * if bold { 0.11 } else { 0.072 } / 0.75,
            lend: LineEnd::Round,
            ljoin: LineJoin::Round,
            ..GContext::default()
        };
        let mut offset = 0.0;
        self.started = true;
        for c in s.chars() {
            let advance = self.metric_info(c, gc).2;
            let mut lines = vec![Vec::new()];
            for vertex in metrics::strokes(c, advance, size, italic) {
                if let Some((x, y)) = vertex {
                    let x = x + offset;
                    let p =
                        self.point((origin.0 + x * cos - y * sin, origin.1 + x * sin + y * cos));
                    if let Some(line) = lines.last_mut() {
                        line.push(p);
                    }
                } else {
                    lines.push(Vec::new());
                }
            }
            let paths: Vec<_> = lines.iter().map(Vec::as_slice).collect();
            stroke::paint(&mut self.frame, &paths, false, self.scale, &pen);
            offset += advance;
        }
    }

    fn str_width(&self, s: &str, gc: &GContext) -> f64 {
        metrics::width(s, gc, self.pointsize, false)
    }
    fn metric_info(&self, c: char, gc: &GContext) -> (f64, f64, f64) {
        metrics::info(c, gc, self.pointsize)
    }

    fn close(&mut self, host: &mut dyn Host) -> HostResult<()> {
        if !self.closed && self.started {
            self.flush(host)?;
        }
        self.closed = true;
        Ok(())
    }
}
