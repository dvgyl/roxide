//! Raster device state and conversion from device points to pixels.

mod fill;
mod png;
mod stroke;

use roxide_host::{Host, HostError, HostResult};

use super::{
    metrics,
    output::{self, PageName, Point, valid_point},
};
use crate::engine::{Device, DeviceSpec, GContext, LineEnd, LineJoin, Rect, Rgba};
use fill::Frame;

pub(super) struct PngDevice {
    file: PageName,
    frame: Frame,
    ppi: f64,
    scale: f64,
    pointsize: f64,
    name: String,
    index: usize,
    started: bool,
    closed: bool,
}

impl PngDevice {
    pub(super) fn open(max_bytes: usize, spec: &DeviceSpec) -> Result<Self, String> {
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
        let (width, height) = (*width_px, *height_px);
        if width == 0 || height == 0 || width > i32::MAX as u32 || height > i32::MAX as u32 {
            return Err("invalid PNG dimensions".into());
        }
        if !res.is_finite()
            || *res <= 0.0
            || *res / 0.0254 > f64::from(u32::MAX)
            || !pointsize.is_finite()
            || *pointsize <= 0.0
        {
            return Err("invalid PNG resolution or pointsize".into());
        }
        let count = (width as usize)
            .checked_mul(height as usize)
            .filter(|count| *count <= max_bytes / 4)
            .ok_or("PNG framebuffer exceeds the host vector limit")?;
        let file = PageName::new(filename, true)?;
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(count)
            .map_err(|_| "cannot allocate PNG framebuffer")?;
        pixels.resize(count, [0; 4]);
        let mut frame = Frame {
            width,
            height,
            pixels,
            clip: Rect::normalized(0.0, 0.0, 0.0, 0.0),
        };
        frame.reset(*bg);
        Ok(Self {
            file,
            frame,
            ppi: *res,
            scale: res / 72.0,
            pointsize: *pointsize,
            name: device_name.clone(),
            index: 1,
            started: false,
            closed: false,
        })
    }

    fn flush(&self, host: &mut dyn Host) -> HostResult<()> {
        let file = png::encode(&self.frame, self.ppi).map_err(HostError::from_io)?;
        output::write(host, &self.file.at(self.index), &file)
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

impl Device for PngDevice {
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
