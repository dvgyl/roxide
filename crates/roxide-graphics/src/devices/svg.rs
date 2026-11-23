//! SVG 1.1 writer with SVG Print page elements for multi-page files.

use std::fmt::Write as _;

use roxide_host::{Host, HostResult};

use super::{
    metrics,
    output::{self, PageName, Point, number as n, valid_point},
};
use crate::engine::{Device, DeviceSpec, GContext, LineEnd, LineJoin, Rect, Rgba};

pub(super) struct SvgDevice {
    file: PageName,
    size: (f64, f64),
    height: f64,
    onefile: bool,
    bg: Rgba,
    pointsize: f64,
    family: String,
    pages: Vec<Page>,
    page: Option<Page>,
    index: usize,
    clips: usize,
    closed: bool,
}

#[derive(Default)]
struct Page {
    body: String,
    clipped: bool,
}

impl Page {
    fn end_clip(&mut self) {
        if self.clipped {
            self.body.push_str("</g>\n");
            self.clipped = false;
        }
    }
}

impl SvgDevice {
    pub(super) fn open(spec: &DeviceSpec, family: String) -> Result<Self, String> {
        let DeviceSpec::Svg {
            filename,
            width,
            height,
            onefile,
            bg,
            pointsize,
        } = spec
        else {
            return Err("invalid SVG device specification".into());
        };
        output::dimensions(*width, *height, *pointsize)?;
        Ok(Self {
            file: PageName::new(filename, true)?,
            size: (*width, *height),
            height: height * 72.0,
            onefile: *onefile,
            bg: *bg,
            pointsize: *pointsize,
            family,
            pages: Vec::new(),
            page: None,
            index: 1,
            clips: 0,
            closed: false,
        })
    }

    fn document(&self, pages: &[Page]) -> String {
        let width = n(self.size.0 * 72.0);
        let height = n(self.height);
        let mut xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" version=\"1.1\" width=\"{width}pt\" height=\"{height}pt\" viewBox=\"0 0 {width} {height}\">\n"
        );
        if self.onefile {
            xml.push_str("<pageSet>\n");
        }
        for page in pages {
            if self.onefile {
                xml.push_str("<page>\n");
            }
            xml.push_str(&page.body);
            if page.clipped {
                xml.push_str("</g>\n");
            }
            if self.onefile {
                xml.push_str("</page>\n");
            }
        }
        if self.onefile {
            xml.push_str("</pageSet>\n");
        }
        xml.push_str("</svg>\n");
        xml
    }

    fn start(&mut self, bg: Rgba) {
        let mut page = Page::default();
        if !bg.is_transparent() {
            page.body.push_str("<rect width=\"100%\" height=\"100%\"");
            paint(&mut page.body, "fill", bg);
            page.body.push_str("/>\n");
        }
        self.page = Some(page);
    }

    fn body(&mut self) -> Option<&mut String> {
        if self.closed {
            return None;
        }
        if self.page.is_none() {
            self.start(self.bg);
        }
        self.page.as_mut().map(|page| &mut page.body)
    }

    fn flush_numbered(&mut self, host: &mut dyn Host) -> HostResult<()> {
        if self.page.is_some() {
            let xml = self.document(self.page.as_slice());
            output::write(host, &self.file.at(self.index), xml.as_bytes())?;
            self.page = None;
        }
        Ok(())
    }

    fn points(&mut self, tag: &str, pts: &[Point], gc: &GContext, fill: bool) {
        if pts.len() < 2 || !pts.iter().copied().all(valid_point) {
            return;
        }
        let height = self.height;
        if let Some(out) = self.body() {
            let _ = write!(out, "<{tag} points=\"");
            for (i, &(x, y)) in pts.iter().enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                let _ = write!(out, "{},{}", n(x), n(height - y));
            }
            out.push('"');
            style(out, gc, fill);
            out.push_str("/>\n");
        }
    }
}

impl Device for SvgDevice {
    fn name(&self) -> &str {
        "svg"
    }
    fn size_in(&self) -> (f64, f64) {
        self.size
    }
    fn new_page(&mut self, host: &mut dyn Host, bg: Rgba) -> HostResult<()> {
        if self.closed {
            return Err(output::invalid("graphics device is closed"));
        }
        if self.onefile {
            self.pages.extend(self.page.take());
        } else if self.page.is_some() {
            self.flush_numbered(host)?;
            self.index += 1;
        }
        self.start(bg);
        Ok(())
    }

    fn clip(&mut self, rect: Rect) {
        if self.closed || !valid_point((rect.x0, rect.y0)) || !valid_point((rect.x1, rect.y1)) {
            return;
        }
        let _ = self.body();
        self.clips += 1;
        let id = self.clips;
        if let Some(page) = &mut self.page {
            page.end_clip();
            let _ = write!(page.body, "<defs><clipPath id=\"clip{id}\"><rect");
            rectangle(&mut page.body, rect, self.height);
            let _ = writeln!(
                page.body,
                "/></clipPath></defs>\n<g clip-path=\"url(#clip{id})\">"
            );
            page.clipped = true;
        }
    }

    fn polyline(&mut self, pts: &[Point], gc: &GContext) {
        self.points("polyline", pts, gc, false);
    }
    fn polygon(&mut self, pts: &[Point], gc: &GContext) {
        self.points("polygon", pts, gc, true);
    }

    fn path(&mut self, subpaths: &[&[Point]], winding: bool, gc: &GContext) {
        let height = self.height;
        let rule = if winding { "nonzero" } else { "evenodd" };
        if let Some(out) = self.body() {
            let _ = write!(out, "<path fill-rule=\"{rule}\" d=\"");
            for pts in subpaths
                .iter()
                .filter(|pts| pts.len() >= 2 && pts.iter().copied().all(valid_point))
            {
                for (i, &(x, y)) in pts.iter().enumerate() {
                    let _ = write!(
                        out,
                        "{} {} {} ",
                        if i == 0 { 'M' } else { 'L' },
                        n(x),
                        n(height - y)
                    );
                }
                out.push_str("Z ");
            }
            out.push('"');
            style(out, gc, true);
            out.push_str("/>\n");
        }
    }

    fn rect(&mut self, r: Rect, gc: &GContext) {
        if !valid_point((r.x0, r.y0)) || !valid_point((r.x1, r.y1)) {
            return;
        }
        let height = self.height;
        if let Some(out) = self.body() {
            out.push_str("<rect");
            rectangle(out, r, height);
            style(out, gc, true);
            out.push_str("/>\n");
        }
    }

    fn circle(&mut self, (x, y): Point, r: f64, gc: &GContext) {
        if !valid_point((x, y)) || !r.is_finite() || r <= 0.0 {
            return;
        }
        let y = self.height - y;
        if let Some(out) = self.body() {
            let _ = write!(
                out,
                "<circle cx=\"{}\" cy=\"{}\" r=\"{}\"",
                n(x),
                n(y),
                n(r)
            );
            style(out, gc, true);
            out.push_str("/>\n");
        }
    }

    fn text(&mut self, at: Point, s: &str, rot: f64, hadj: f64, gc: &GContext) {
        let size = metrics::size(gc, self.pointsize);
        let (x, y) = metrics::text_origin(at, self.str_width(s, gc), rot, hadj);
        if s.is_empty()
            || gc.col.is_transparent()
            || !valid_point((x, y))
            || !rot.is_finite()
            || !size.is_finite()
            || size <= 0.0
        {
            return;
        }
        let y = self.height - y;
        let (x, y) = (n(x), n(y));
        let family = if gc.fontfamily.is_empty() {
            &self.family
        } else {
            &gc.fontfamily
        };
        let family = match family.as_str() {
            "sans" => "Helvetica, sans-serif",
            "mono" => "monospace",
            family => family,
        };
        let family = escaped(family);
        if let Some(out) = self.body() {
            let _ = write!(
                out,
                "<text x=\"{x}\" y=\"{y}\" font-family=\"{family}\" font-size=\"{}\" kerning=\"0\" xml:space=\"preserve\"",
                n(size)
            );
            if rot != 0.0 {
                let _ = write!(out, " transform=\"rotate({} {x} {y})\"", n(-rot));
            }
            if matches!(gc.fontface, 2 | 4) {
                out.push_str(" font-weight=\"bold\"");
            }
            if matches!(gc.fontface, 3 | 4) {
                out.push_str(" font-style=\"oblique\"");
            }
            paint(out, "fill", gc.col);
            let _ = writeln!(out, ">{}</text>", escaped(s));
        }
    }

    fn str_width(&self, s: &str, gc: &GContext) -> f64 {
        metrics::width(s, gc, self.pointsize, false)
    }
    fn metric_info(&self, c: char, gc: &GContext) -> (f64, f64, f64) {
        metrics::info(c, gc, self.pointsize)
    }

    fn close(&mut self, host: &mut dyn Host) -> HostResult<()> {
        if self.closed {
            return Ok(());
        }
        if self.onefile {
            self.pages.extend(self.page.take());
            let xml = self.document(&self.pages);
            output::write(host, &self.file.at(1), xml.as_bytes())?;
            self.pages.clear();
        } else {
            self.flush_numbered(host)?;
        }
        self.closed = true;
        Ok(())
    }
}

fn rectangle(out: &mut String, r: Rect, height: f64) {
    let _ = write!(
        out,
        " x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"",
        n(r.x0.min(r.x1)),
        n(height - r.y0.max(r.y1)),
        n(r.width()),
        n(r.height())
    );
}

fn paint(out: &mut String, kind: &str, c: Rgba) {
    if c.is_transparent() {
        let _ = write!(out, " {kind}=\"none\"");
        return;
    }
    let _ = write!(out, " {kind}=\"#{:02X}{:02X}{:02X}\"", c.r, c.g, c.b);
    if c.a != 255 {
        let opacity = f64::from(c.a) / 255.0;
        let digits = (5 - opacity.log10().floor() as i32) as usize;
        let value = format!("{opacity:.digits$}");
        let _ = write!(out, " {kind}-opacity=\"{}\"", value.trim_end_matches('0'));
    }
}

fn style(out: &mut String, gc: &GContext, fill: bool) {
    paint(out, "fill", if fill { gc.fill } else { Rgba::TRANSPARENT });
    if let Some(width) = output::line_width(gc) {
        paint(out, "stroke", gc.col);
        let cap = match gc.lend {
            LineEnd::Butt => "butt",
            LineEnd::Round => "round",
            LineEnd::Square => "square",
        };
        let join = match gc.ljoin {
            LineJoin::Mitre => "miter",
            LineJoin::Round => "round",
            LineJoin::Bevel => "bevel",
        };
        let mitre = if gc.lmitre.is_finite() {
            gc.lmitre.max(1.0)
        } else {
            10.0
        };
        let _ = write!(
            out,
            " stroke-width=\"{}\" stroke-linecap=\"{cap}\" stroke-linejoin=\"{join}\" stroke-miterlimit=\"{}\"",
            n(width),
            n(mitre)
        );
        let dash = output::dashes(gc, width)
            .map(n)
            .collect::<Vec<_>>()
            .join(" ");
        if !dash.is_empty() {
            let _ = write!(out, " stroke-dasharray=\"{dash}\"");
        }
    }
}

fn escaped(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(c),
            '\u{0}'..='\u{1f}' | '\u{fffe}' | '\u{ffff}' => out.push('\u{fffd}'),
            _ => out.push(c),
        }
    }
    out
}
