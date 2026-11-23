//! PDF 1.4 writer for the graphics device contract.

use std::{collections::BTreeSet, fmt::Write as _, io::Write as _, path::PathBuf};

use flate2::{Compression, write::ZlibEncoder};
use roxide_host::{Host, HostError, HostResult, WallTime};

use super::{
    metrics,
    output::{self, PageName, Point, number as n, valid_point},
};
use crate::engine::{Device, GContext, LineEnd, LineJoin, Rect, Rgba};

pub(super) struct Settings {
    pub(super) title: String,
    pub(super) kerning: bool,
    pub(super) odd_even: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            title: "R Graphics Output".into(),
            kerning: true,
            odd_even: false,
        }
    }
}

pub(super) struct PdfDevice {
    file: PageName,
    first_path: PathBuf,
    size: (f64, f64),
    points: Rect,
    onefile: bool,
    compress: bool,
    bg: Rgba,
    pointsize: f64,
    settings: Settings,
    created: WallTime,
    pages: Vec<Page>,
    page: Option<Page>,
    index: usize,
    closed: bool,
}

struct Page {
    stream: String,
    alpha: BTreeSet<(u8, u8)>,
}

impl PdfDevice {
    pub(super) fn open(
        host: &mut dyn Host,
        spec: &crate::engine::DeviceSpec,
        settings: Settings,
    ) -> Result<Self, String> {
        let crate::engine::DeviceSpec::Pdf {
            file,
            width,
            height,
            onefile,
            compress,
            bg,
            pointsize,
        } = spec
        else {
            return Err("invalid PDF device specification".into());
        };
        output::dimensions(*width, *height, *pointsize)?;
        let file = PageName::new(file, !onefile)?;
        let first_path = output::path(host, &file.at(1));
        let first_path = if first_path.is_relative() {
            host.cwd().map_err(|err| err.message)?.join(first_path)
        } else {
            first_path
        };
        output::write_path(host, &first_path, &[]).map_err(|err| err.message)?;
        Ok(Self {
            file,
            first_path,
            size: (*width, *height),
            points: Rect::normalized(0.0, 0.0, width * 72.0, height * 72.0),
            onefile: *onefile,
            compress: *compress,
            bg: *bg,
            pointsize: *pointsize,
            settings,
            created: host.now(),
            pages: Vec::new(),
            page: None,
            index: 1,
            closed: false,
        })
    }

    fn start(&mut self, bg: Rgba) {
        let mut page = Page {
            stream: "q\n".into(),
            alpha: BTreeSet::new(),
        };
        if !bg.is_transparent() {
            page.alpha(bg.a, 255);
            color(&mut page.stream, bg, "rg");
            rectangle(&mut page.stream, self.points);
            page.stream.push_str("f\n");
        }
        self.page = Some(page);
    }

    fn page(&mut self) -> Option<&mut Page> {
        if self.closed {
            return None;
        }
        if self.page.is_none() {
            self.start(self.bg);
        }
        self.page.as_mut()
    }

    fn flush_numbered(&mut self, host: &mut dyn Host) -> HostResult<()> {
        let pages = self.page.as_slice();
        let file = self.document(pages)?;
        if self.index == 1 {
            output::write_path(host, &self.first_path, &file)?;
        } else {
            output::write(host, &self.file.at(self.index), &file)?;
        }
        self.page = None;
        Ok(())
    }

    fn document(&self, pages: &[Page]) -> HostResult<Vec<u8>> {
        let mut pdf = Document::new();
        pdf.obj("<< /Type /Catalog /Pages 2 0 R >>".into());
        let mut tree = format!(
            "<< /Type /Pages /Count {} /MediaBox [0 0 {} {}] /Kids [",
            pages.len(),
            n(self.points.x1),
            n(self.points.y1)
        );
        for i in 0..pages.len() {
            let _ = write!(tree, "{} 0 R ", 8 + 2 * i);
        }
        tree.push_str("] >>");
        pdf.obj(tree);
        pdf.obj(format!(
            "<< /Title {} /CreationDate ({}) >>",
            title(&self.settings.title),
            date(self.created)
        ));
        for name in [
            "Helvetica",
            "Helvetica-Bold",
            "Helvetica-Oblique",
            "Helvetica-BoldOblique",
        ] {
            pdf.obj(format!(
                "<< /Type /Font /Subtype /Type1 /BaseFont /{name} /Encoding /WinAnsiEncoding >>"
            ));
        }
        for page in pages {
            let mut resources = String::from("/Font << /F1 4 0 R /F2 5 0 R /F3 6 0 R /F4 7 0 R >>");
            if !page.alpha.is_empty() {
                resources.push_str(" /ExtGState <<");
                for (fill, stroke) in &page.alpha {
                    let _ = write!(
                        resources,
                        " /A{fill}_{stroke} << /Type /ExtGState /ca {} /CA {} >>",
                        alpha(*fill),
                        alpha(*stroke)
                    );
                }
                resources.push_str(" >>");
            }
            pdf.obj(format!(
                "<< /Type /Page /Parent 2 0 R /Resources << {resources} >> /Contents {} 0 R >>",
                pdf.offsets.len() + 2
            ));
            pdf.stream(&page.stream, self.compress)?;
        }
        Ok(pdf.finish())
    }
}

impl Page {
    fn alpha(&mut self, fill: u8, stroke: u8) {
        self.alpha.insert((fill, stroke));
        let _ = writeln!(self.stream, "/A{fill}_{stroke} gs");
    }

    fn shape(&mut self, gc: &GContext, fill: bool, odd_even: bool, path: impl FnOnce(&mut String)) {
        let fill = fill && !gc.fill.is_transparent();
        let stroke = output::line_width(gc);
        if !fill && stroke.is_none() {
            return;
        }
        self.alpha(
            if fill { gc.fill.a } else { 255 },
            stroke.map_or(255, |_| gc.col.a),
        );
        if fill {
            color(&mut self.stream, gc.fill, "rg");
        }
        if let Some(width) = stroke {
            color(&mut self.stream, gc.col, "RG");
            let cap = match gc.lend {
                LineEnd::Butt => 0,
                LineEnd::Round => 1,
                LineEnd::Square => 2,
            };
            let join = match gc.ljoin {
                LineJoin::Mitre => 0,
                LineJoin::Round => 1,
                LineJoin::Bevel => 2,
            };
            let mitre = if gc.lmitre.is_finite() {
                gc.lmitre.max(1.0)
            } else {
                10.0
            };
            let dash = output::dashes(gc, width)
                .map(n)
                .collect::<Vec<_>>()
                .join(" ");
            let _ = writeln!(
                self.stream,
                "{} w {cap} J {join} j {} M [{dash}] 0 d",
                n(width),
                n(mitre)
            );
        }
        path(&mut self.stream);
        let op = match (fill, stroke.is_some(), odd_even) {
            (true, true, true) => "B*\n",
            (true, true, false) => "B\n",
            (true, false, true) => "f*\n",
            (true, false, false) => "f\n",
            _ => "S\n",
        };
        self.stream.push_str(op);
    }
}

impl Device for PdfDevice {
    fn name(&self) -> &str {
        "pdf"
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
        if valid_point((rect.x0, rect.y0))
            && valid_point((rect.x1, rect.y1))
            && let Some(page) = self.page()
        {
            page.stream.push_str("Q\nq\n");
            rectangle(&mut page.stream, rect);
            page.stream.push_str("W n\n");
        }
    }

    fn polyline(&mut self, pts: &[Point], gc: &GContext) {
        if pts.len() >= 2
            && pts.iter().copied().all(valid_point)
            && let Some(page) = self.page()
        {
            page.shape(gc, false, false, |out| lines(out, pts, false));
        }
    }

    fn polygon(&mut self, pts: &[Point], gc: &GContext) {
        self.path(&[pts], !self.settings.odd_even, gc);
    }

    fn path(&mut self, subpaths: &[&[Point]], winding: bool, gc: &GContext) {
        if let Some(page) = self.page() {
            page.shape(gc, true, !winding, |out| {
                for pts in subpaths
                    .iter()
                    .filter(|pts| pts.len() >= 2 && pts.iter().copied().all(valid_point))
                {
                    lines(out, pts, true);
                }
            });
        }
    }

    fn rect(&mut self, rect: Rect, gc: &GContext) {
        if valid_point((rect.x0, rect.y0))
            && valid_point((rect.x1, rect.y1))
            && let Some(page) = self.page()
        {
            page.shape(gc, true, false, |out| rectangle(out, rect));
        }
    }

    fn circle(&mut self, (x, y): Point, r: f64, gc: &GContext) {
        if !valid_point((x, y)) || !r.is_finite() || r <= 0.0 {
            return;
        }
        if let Some(page) = self.page() {
            page.shape(gc, true, false, |out| {
                let k = r * (4.0 / 3.0) * (2.0_f64.sqrt() - 1.0);
                let _ = writeln!(out, "{} {} m", n(x + r), n(y));
                for (a, b, c) in [
                    ((x + r, y + k), (x + k, y + r), (x, y + r)),
                    ((x - k, y + r), (x - r, y + k), (x - r, y)),
                    ((x - r, y - k), (x - k, y - r), (x, y - r)),
                    ((x + k, y - r), (x + r, y - k), (x + r, y)),
                ] {
                    let _ = writeln!(
                        out,
                        "{} {} {} {} {} {} c",
                        n(a.0),
                        n(a.1),
                        n(b.0),
                        n(b.1),
                        n(c.0),
                        n(c.1)
                    );
                }
                out.push_str("h\n");
            });
        }
    }

    fn text(&mut self, at: Point, s: &str, rot: f64, hadj: f64, gc: &GContext) {
        let size = metrics::size(gc, self.pointsize);
        let origin = metrics::text_origin(at, self.str_width(s, gc), rot, hadj);
        if s.is_empty()
            || gc.col.is_transparent()
            || !valid_point(origin)
            || !rot.is_finite()
            || !size.is_finite()
            || size <= 0.0
        {
            return;
        }
        let kern = self.settings.kerning;
        if let Some(page) = self.page() {
            let (sin, cos) = (rot % 360.0).to_radians().sin_cos();
            page.alpha(gc.col.a, 255);
            color(&mut page.stream, gc.col, "rg");
            let _ = writeln!(
                page.stream,
                "BT /F{} {} Tf {} {} {} {} {} {} Tm",
                metrics::face(gc) + 1,
                n(size),
                n(cos),
                n(sin),
                n(-sin),
                n(cos),
                n(origin.0),
                n(origin.1)
            );
            text_operand(&mut page.stream, s, gc, kern);
            page.stream.push_str("ET\n");
        }
    }

    fn str_width(&self, s: &str, gc: &GContext) -> f64 {
        metrics::width(s, gc, self.pointsize, self.settings.kerning)
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
            let bytes = self.document(&self.pages)?;
            output::write_path(host, &self.first_path, &bytes)?;
            self.pages.clear();
        } else {
            self.flush_numbered(host)?;
        }
        self.closed = true;
        Ok(())
    }
}

fn lines(out: &mut String, pts: &[Point], close: bool) {
    for (i, &(x, y)) in pts.iter().enumerate() {
        let _ = writeln!(out, "{} {} {}", n(x), n(y), if i == 0 { "m" } else { "l" });
    }
    if close {
        out.push_str("h\n");
    }
}

fn rectangle(out: &mut String, r: Rect) {
    let _ = writeln!(
        out,
        "{} {} {} {} re",
        n(r.x0.min(r.x1)),
        n(r.y0.min(r.y1)),
        n(r.width()),
        n(r.height())
    );
}

fn alpha(value: u8) -> String {
    n(f64::from(value) / 255.0)
}

fn color(out: &mut String, c: Rgba, op: &str) {
    let _ = writeln!(out, "{} {} {} {op}", alpha(c.r), alpha(c.g), alpha(c.b));
}

fn literal_byte(out: &mut String, b: u8) {
    match b {
        b'(' | b')' | b'\\' => {
            out.push('\\');
            out.push(char::from(b));
        }
        32..=126 => out.push(char::from(b)),
        _ => {
            let _ = write!(out, "\\{b:03o}");
        }
    }
}

fn text_operand(out: &mut String, s: &str, gc: &GContext, kern: bool) {
    let mut chars = s.chars().map(metrics::latin1).peekable();
    let use_array = kern
        && chars
            .clone()
            .zip(chars.clone().skip(1))
            .any(|(a, b)| metrics::kern(a, b, gc) != 0);
    out.push_str(if use_array { "[(" } else { "(" });
    while let Some(c) = chars.next() {
        literal_byte(out, c);
        if use_array && let Some(&next) = chars.peek() {
            let offset = -metrics::kern(c, next, gc);
            if offset != 0 {
                let _ = write!(out, ") {offset} (");
            }
        }
    }
    out.push_str(if use_array { ")] TJ\n" } else { ") Tj\n" });
}

fn title(text: &str) -> String {
    if text.is_ascii() {
        let mut out = "(".to_owned();
        for b in text.bytes() {
            literal_byte(&mut out, b);
        }
        out.push(')');
        out
    } else {
        let mut out = "<FEFF".to_owned();
        for unit in text.encode_utf16() {
            let _ = write!(out, "{unit:04X}");
        }
        out.push('>');
        out
    }
}

fn date(time: WallTime) -> String {
    let seconds = time.unix_us.div_euclid(1_000_000);
    // Convert Unix days to the proleptic Gregorian calendar in 400-year eras.
    let days = seconds.div_euclid(86_400) + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let march_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * march_month + 2) / 5 + 1;
    let month = (march_month + 2) % 12 + 1;
    let year = era * 400 + year_of_era + i64::from(month <= 2);
    if !(0..=9999).contains(&year) {
        return "D:19700101000000Z".into();
    }
    let h = seconds.div_euclid(3600).rem_euclid(24);
    let m = seconds.div_euclid(60).rem_euclid(60);
    let s = seconds.rem_euclid(60);
    format!("D:{year:04}{month:02}{day:02}{h:02}{m:02}{s:02}Z")
}

struct Document {
    bytes: Vec<u8>,
    offsets: Vec<usize>,
}

impl Document {
    fn new() -> Self {
        Self {
            bytes: b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec(),
            offsets: Vec::new(),
        }
    }

    fn begin(&mut self) {
        self.offsets.push(self.bytes.len());
        let _ = writeln!(self.bytes, "{} 0 obj", self.offsets.len());
    }

    fn obj(&mut self, contents: String) {
        self.begin();
        self.bytes.extend_from_slice(contents.as_bytes());
        self.bytes.extend_from_slice(b"\nendobj\n");
    }

    fn stream(&mut self, contents: &str, compress: bool) -> HostResult<()> {
        self.begin();
        if compress {
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
            encoder
                .write_all(contents.as_bytes())
                .and_then(|()| encoder.write_all(b"Q\n"))
                .map_err(HostError::from_io)?;
            let compressed = encoder.finish().map_err(HostError::from_io)?;
            let _ = writeln!(
                self.bytes,
                "<< /Length {} /Filter /FlateDecode >>\nstream",
                compressed.len()
            );
            self.bytes.extend(compressed);
        } else {
            let _ = writeln!(self.bytes, "<< /Length {} >>\nstream", contents.len() + 2);
            self.bytes.extend_from_slice(contents.as_bytes());
            self.bytes.extend_from_slice(b"Q\n");
        }
        self.bytes.extend_from_slice(b"\nendstream\nendobj\n");
        Ok(())
    }

    fn finish(mut self) -> Vec<u8> {
        let start = self.bytes.len();
        let size = self.offsets.len() + 1;
        let _ = write!(self.bytes, "xref\n0 {size}\n0000000000 65535 f \n");
        for offset in self.offsets {
            let _ = writeln!(self.bytes, "{offset:010} 00000 n ");
        }
        let _ = write!(
            self.bytes,
            "trailer\n<< /Size {size} /Root 1 0 R /Info 3 0 R >>\nstartxref\n{start}\n%%EOF\n"
        );
        self.bytes
    }
}
