use std::f64::consts::{PI, TAU};

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{CharId, SEXP_NIL, SexpType, ValueApi};

use crate::engine::{Device, GContext, LineEnd, LineJoin, Rect, Rgba};

use super::{
    active,
    coords::{rect, require_frame, user_point},
    element, flag, names, nums,
    par::{GPar, Geometry, Param, choice, line_type, parse_color, state},
    record, string, strings,
};

type Point = (f64, f64);

pub(super) fn context(p: &GPar) -> GContext {
    let mut gc = GContext {
        col: p.color("col"),
        lwd: p.real("lwd"),
        cex: p.real("cex"),
        ps: p.real("ps"),
        fontface: p.real("font") as u8,
        fontfamily: p.text("family").to_owned(),
        lineheight: p.real("lheight"),
        lmitre: p.real("lmitre"),
        lend: cap(p.text("lend")),
        ljoin: join(p.text("ljoin")),
        ..GContext::default()
    };
    dashes(&mut gc, p.text("lty"));
    gc
}

fn cap(x: &str) -> LineEnd {
    match x {
        "butt" => LineEnd::Butt,
        "square" => LineEnd::Square,
        _ => LineEnd::Round,
    }
}

fn join(x: &str) -> LineJoin {
    match x {
        "mitre" => LineJoin::Mitre,
        "bevel" => LineJoin::Bevel,
        _ => LineJoin::Round,
    }
}

fn dashes(gc: &mut GContext, lty: &str) {
    let pattern = match lty {
        "solid" | "blank" => "",
        "dashed" => "44",
        "dotted" => "13",
        "dotdash" => "1343",
        "longdash" => "73",
        "twodash" => "2262",
        other => other,
    };
    gc.lty = pattern
        .chars()
        .filter_map(|c| c.to_digit(16))
        .map(f64::from)
        .collect();
}

fn recycle<T>(x: &[T], i: usize) -> Option<&T> {
    if x.is_empty() {
        None
    } else {
        x.get(i % x.len())
    }
}

fn n(x: &[f64], i: usize, default: f64) -> f64 {
    recycle(x, i).copied().unwrap_or(default)
}

fn scalar_at(ctx: &mut Ctx, x: SexpId, i: usize) -> EvalResult {
    if ctx.session().type_of(x)? == SexpType::Str {
        let value = ctx
            .session()
            .str_slice(x)?
            .get(i)
            .copied()
            .unwrap_or(CharId::NA);
        Ok(ctx.session_mut().alloc_str_from(&[value])?)
    } else {
        let value = nums(ctx, x)?.get(i).copied().unwrap_or(f64::NAN);
        Ok(ctx.session_mut().alloc_real_from(&[value])?)
    }
}

fn colors(ctx: &mut Ctx, x: SexpId) -> EvalResult<Vec<Rgba>> {
    if x == SEXP_NIL {
        return Ok(Vec::new());
    }
    if ctx.session().type_of(x)? != SexpType::Str {
        let x = nums(ctx, x)?;
        return x
            .into_iter()
            .map(|value| {
                let scalar = ctx.session_mut().alloc_real_from(&[value])?;
                ctx.scoped(|ctx| {
                    ctx.protect(scalar);
                    parse_color(ctx, scalar)
                })
            })
            .collect();
    }
    (0..ctx.session().xlength(x)?)
        .map(|i| {
            ctx.scoped(|ctx| {
                let scalar = scalar_at(ctx, x, i)?;
                ctx.protect(scalar);
                parse_color(ctx, scalar)
            })
        })
        .collect()
}

#[derive(Clone)]
enum Symbol {
    Code(i32),
    Text(String),
    Skip,
}

fn symbols(ctx: &mut Ctx, x: SexpId) -> EvalResult<Vec<Symbol>> {
    if ctx.session().type_of(x)? == SexpType::Str {
        Ok(strings(ctx, x)?
            .into_iter()
            .map(|s| match s.and_then(|s| s.chars().next()) {
                Some(c) => Symbol::Text(c.to_string()),
                None => Symbol::Skip,
            })
            .collect())
    } else {
        Ok(nums(ctx, x)?
            .into_iter()
            .map(|x| {
                if x.is_finite() {
                    Symbol::Code(x as i32)
                } else {
                    Symbol::Skip
                }
            })
            .collect())
    }
}

#[derive(Default)]
struct Style {
    col: Vec<Rgba>,
    bg: Vec<Rgba>,
    border: Vec<Rgba>,
    cex: Vec<f64>,
    lwd: Vec<f64>,
    font: Vec<f64>,
    srt: Vec<f64>,
    adj: Vec<f64>,
    padj: Vec<f64>,
    lty: Vec<Option<String>>,
    pch: Vec<Symbol>,
    family: Option<String>,
    cap: Option<LineEnd>,
    join: Option<LineJoin>,
    xpd: Option<Option<bool>>,
}

impl Style {
    fn parse(ctx: &mut Ctx, call: &Call, x: SexpId) -> EvalResult<Self> {
        let mut style = Self::default();
        for (i, name) in names(ctx, x)?.iter().enumerate() {
            let value = element(ctx, x, i)?;
            if value == SEXP_NIL {
                continue;
            }
            match name.as_deref() {
                Some("col") => style.col = colors(ctx, value)?,
                Some("bg") => style.bg = colors(ctx, value)?,
                Some("border") => style.border = colors(ctx, value)?,
                Some("cex") => style.cex = nums(ctx, value)?,
                Some("lwd") => style.lwd = nums(ctx, value)?,
                Some("font") => style.font = nums(ctx, value)?,
                Some("srt") => style.srt = nums(ctx, value)?,
                Some("adj") => style.adj = nums(ctx, value)?,
                Some("padj") => style.padj = nums(ctx, value)?,
                Some("pch") => style.pch = symbols(ctx, value)?,
                Some("family") => style.family = Some(string(ctx, value)?),
                Some("lend") => {
                    style.cap = choice(ctx, value, &["round", "butt", "square"])?
                        .as_deref()
                        .map(cap)
                }
                Some("ljoin") => {
                    style.join = choice(ctx, value, &["round", "mitre", "bevel"])?
                        .as_deref()
                        .map(join)
                }
                Some("xpd") => style.xpd = Some(flag(ctx, value)?),
                Some("lty") => {
                    for i in 0..ctx.session().xlength(value)? {
                        let selected = ctx.scoped(|ctx| {
                            let scalar = scalar_at(ctx, value, i)?;
                            ctx.protect(scalar);
                            line_type(ctx, scalar)
                        })?;
                        style.lty.push(selected);
                    }
                }
                Some(
                    "type" | "xlab" | "ylab" | "main" | "sub" | "xaxt" | "yaxt" | "las" | "ann",
                )
                | None => {}
                Some(name) if !name.is_empty() => {
                    ctx.warn_now(
                        call.err_call(),
                        format!("\"{name}\" is not a graphical parameter"),
                    )?;
                }
                _ => {}
            }
        }
        Ok(style)
    }

    fn color(&self, i: usize, default: Rgba) -> Rgba {
        recycle(&self.col, i).copied().unwrap_or(default)
    }

    fn line_type<'a>(&'a self, p: &'a GPar, i: usize) -> &'a str {
        recycle(&self.lty, i)
            .and_then(Option::as_deref)
            .unwrap_or_else(|| p.text("lty"))
    }

    fn gc(&self, base: &GContext, p: &GPar, i: usize) -> GContext {
        let mut gc = base.clone();
        gc.col = self.color(i, gc.col);
        gc.fill = recycle(&self.bg, i).copied().unwrap_or(gc.fill);
        gc.cex *= n(&self.cex, i, 1.0);
        gc.lwd = n(&self.lwd, i, gc.lwd);
        gc.fontface = n(&self.font, i, f64::from(gc.fontface)) as u8;
        if let Some(family) = &self.family {
            gc.fontfamily.clone_from(family);
        }
        if let Some(cap) = self.cap {
            gc.lend = cap;
        }
        if let Some(join) = self.join {
            gc.ljoin = join;
        }
        let lty = self.line_type(p, i);
        dashes(&mut gc, lty);
        if lty == "blank" {
            gc.col = Rgba::TRANSPARENT;
        }
        gc
    }
}

fn finite((x, y): Point) -> bool {
    x.is_finite() && y.is_finite()
}

fn path_parts(pts: &[Point]) -> impl Iterator<Item = &[Point]> {
    pts.split(|&p| !finite(p)).filter(|p| !p.is_empty())
}

fn marker(d: &mut dyn Device, center: Point, pch: &Symbol, gc: &GContext) {
    let mut gc = gc.clone();
    gc.lty.clear();
    let (x, y) = center;
    let r = gc.ps * gc.cex * 0.27;
    if !r.is_finite() || r <= 0.0 {
        return;
    }
    let square = [
        (x - r, y - r),
        (x + r, y - r),
        (x + r, y + r),
        (x - r, y + r),
    ];
    let diamond = [(x - r, y), (x, y - r), (x + r, y), (x, y + r)];
    let tri = |up: f64| {
        [
            (x - r, y - 0.6 * r * up),
            (x + r, y - 0.6 * r * up),
            (x, y + 1.2 * r * up),
        ]
    };
    let plus = [[(x - r, y), (x + r, y)], [(x, y - r), (x, y + r)]];
    let cross = [[square[0], square[2]], [square[1], square[3]]];
    if let Symbol::Code(15..=20) = pch {
        gc.fill = gc.col;
        gc.col = Rgba::TRANSPARENT;
    } else if !matches!(pch, Symbol::Code(21..=25)) {
        gc.fill = Rgba::TRANSPARENT;
    }
    match pch {
        Symbol::Code(0 | 15 | 22) => d.polygon(&square, &gc),
        Symbol::Code(1 | 16 | 19 | 21) => d.circle(center, r, &gc),
        Symbol::Code(2 | 17 | 24) => d.polygon(&tri(1.0), &gc),
        Symbol::Code(5 | 18 | 23) => d.polygon(&diamond, &gc),
        Symbol::Code(6 | 25) => d.polygon(&tri(-1.0), &gc),
        Symbol::Code(3 | 4 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 14) => {
            let Symbol::Code(code) = pch else {
                return;
            };
            match code {
                7 | 12 | 14 => d.polygon(&square, &gc),
                9 => d.polygon(&diamond, &gc),
                10 | 13 => d.circle(center, r, &gc),
                11 => {
                    d.polygon(&tri(1.0), &gc);
                    d.polygon(&tri(-1.0), &gc);
                }
                _ => {}
            }
            if matches!(code, 3 | 8 | 9 | 10 | 12) {
                for line in plus {
                    d.polyline(&line, &gc);
                }
            }
            if matches!(code, 4 | 7 | 8 | 13) {
                for line in cross {
                    d.polyline(&line, &gc);
                }
            }
            if *code == 14 {
                d.polygon(&tri(1.0), &gc);
            }
        }
        Symbol::Code(20) => d.circle(center, r * 0.65, &gc),
        Symbol::Code(46) => d.rect(
            Rect::normalized(x - 0.5, y - 0.5, x + 0.5, y + 0.5),
            &GContext {
                fill: gc.col,
                ..gc.clone()
            },
        ),
        Symbol::Code(code) if (32..=255).contains(code) || *code < 0 => {
            if let Some(c) = char::from_u32(code.unsigned_abs()) {
                label(d, center, &c.to_string(), 0.0, 0.5, 0.5, &gc);
            }
        }
        Symbol::Text(s) => label(d, center, s, 0.0, 0.5, 0.5, &gc),
        _ => {}
    }
}

fn label(d: &mut dyn Device, at: Point, s: &str, rot: f64, adj: f64, vadj: f64, gc: &GContext) {
    let (sin, cos) = rot.to_radians().sin_cos();
    let step = gc.ps * gc.cex * 1.2 * gc.lineheight;
    let shift =
        d.metric_info('M', gc).0 * vadj - (s.split('\n').count() - 1) as f64 * step * (1.0 - vadj);
    for (i, s) in s.split('\n').enumerate() {
        let offset = shift + i as f64 * step;
        d.text((at.0 + sin * offset, at.1 - cos * offset), s, rot, adj, gc);
    }
}

struct Drawing<'a> {
    d: &'a mut dyn Device,
    p: &'a GPar,
    g: Geometry,
    style: Style,
    base: GContext,
}

impl Drawing<'_> {
    fn gc(&self, i: usize) -> GContext {
        self.style.gc(&self.base, self.p, i)
    }

    fn point(&self, x: f64, y: f64) -> Point {
        user_point(self.p, self.g, x, y)
    }

    fn xy(&self, x: &[f64], y: &[f64]) -> Vec<Point> {
        (0..x.len().max(y.len()))
            .map(|i| self.point(n(x, i, f64::NAN), n(y, i, f64::NAN)))
            .collect()
    }

    fn plot_xy(&mut self, x: &[f64], y: &[f64], kind: &str) {
        let pts = self.xy(x, y);
        let gc = self.gc(0);
        let pch = match self.p.value("pch") {
            Some(Param::Int(x)) => Symbol::Code(*x),
            Some(Param::Text(s)) => Symbol::Text(s.clone()),
            _ => Symbol::Skip,
        };
        if matches!(kind, "l" | "b" | "c" | "o" | "s" | "S") {
            for part in path_parts(&pts).filter(|part| part.len() > 1) {
                match kind {
                    "s" | "S" => {
                        let mut steps = vec![part[0]];
                        for pair in part.windows(2) {
                            let [a, b] = [pair[0], pair[1]];
                            steps.push(if kind == "s" { (b.0, a.1) } else { (a.0, b.1) });
                            steps.push(b);
                        }
                        self.d.polyline(&steps, &gc);
                    }
                    "b" | "c" => {
                        let gap = self.p.char_size(1) * 18.0;
                        for pair in part.windows(2) {
                            let [a, b] = [pair[0], pair[1]];
                            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                            let distance = dx.hypot(dy);
                            if distance > 2.0 * gap {
                                let f = gap / distance;
                                self.d.polyline(
                                    &[(a.0 + f * dx, a.1 + f * dy), (b.0 - f * dx, b.1 - f * dy)],
                                    &gc,
                                );
                            }
                        }
                    }
                    _ => self.d.polyline(part, &gc),
                }
            }
        }
        for (i, pt) in pts.into_iter().enumerate().filter(|(_, p)| finite(*p)) {
            if matches!(kind, "p" | "b" | "o") {
                let mut gc = self.gc(i);
                gc.col = self.style.color(i, self.base.col);
                marker(self.d, pt, recycle(&self.style.pch, i).unwrap_or(&pch), &gc);
            }
            if kind == "h" {
                let zero = self.point(n(x, i, 0.0), 0.0);
                if finite(zero) {
                    self.d.polyline(&[zero, pt], &gc);
                }
            }
        }
    }

    fn fill_polygon(&mut self, pts: &[Point], density: f64, angle: f64, gc: &GContext) {
        if density.is_nan() || density < 0.0 {
            self.d.polygon(pts, gc);
            return;
        }
        let border = GContext {
            fill: Rgba::TRANSPARENT,
            ..gc.clone()
        };
        if density > 0.0 && density.is_finite() && angle.is_finite() && pts.len() >= 3 {
            let origin = self.point(
                f64::from(u8::from(self.p.log[0])),
                f64::from(u8::from(self.p.log[1])),
            );
            let (sin, cos) = angle.to_radians().sin_cos();
            let project =
                |(x, y): Point| (-sin * (x - origin.0) + cos * (y - origin.1)) * density / 72.0;
            let levels: Vec<_> = pts.iter().copied().map(project).collect();
            let min = levels.iter().copied().fold(f64::INFINITY, f64::min);
            let max = levels.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let first = min.ceil();
            let count = (max.floor() - first + 1.0).max(0.0);
            let hatch = GContext {
                col: if gc.fill.is_transparent() {
                    self.p.color("fg")
                } else {
                    gc.fill
                },
                ..border.clone()
            };
            // Pair the intersections along each line to fill concave polygons.
            let mut intersections = Vec::new();
            let limit = 1_000_000 / pts.len();
            for line in 0..count.min(limit as f64) as usize {
                let level = first + line as f64;
                intersections.clear();
                for i in 0..pts.len() {
                    let j = (i + 1) % pts.len();
                    let (a, b) = (levels[i], levels[j]);
                    if (a <= level && b > level) || (b <= level && a > level) {
                        let t = (level - a) / (b - a);
                        intersections.push((
                            pts[i].0 + t * (pts[j].0 - pts[i].0),
                            pts[i].1 + t * (pts[j].1 - pts[i].1),
                        ));
                    }
                }
                intersections.sort_unstable_by(|a, b| {
                    (a.0 * cos + a.1 * sin).total_cmp(&(b.0 * cos + b.1 * sin))
                });
                for pair in intersections.as_chunks::<2>().0 {
                    self.d.polyline(pair, &hatch);
                }
            }
        }
        self.d.polygon(pts, &border);
    }

    fn endpoints(&mut self, data: &[Vec<f64>], kind: &str) -> bool {
        let x0 = &data[0];
        let y0 = &data[1];
        let x1 = &data[2];
        let y1 = &data[3];
        if [x0, y0, x1, y1].iter().any(|x| x.is_empty()) {
            return false;
        }
        let mut indeterminate = false;
        for i in 0..x0.len().max(y0.len()).max(x1.len()).max(y1.len()) {
            let a = self.point(n(x0, i, 0.0), n(y0, i, 0.0));
            let b = self.point(n(x1, i, 0.0), n(y1, i, 0.0));
            if !finite(a) || !finite(b) {
                continue;
            }
            let mut gc = self.gc(i);
            if kind == "rect" {
                gc.fill = recycle(&self.style.col, i)
                    .copied()
                    .unwrap_or(Rgba::TRANSPARENT);
                gc.col = recycle(&self.style.border, i)
                    .copied()
                    .unwrap_or_else(|| self.p.color("fg"));
                let density = n(&data[4], i, -1.0);
                if density >= 0.0 {
                    let r = Rect::normalized(a.0, a.1, b.0, b.1);
                    let pts = [(r.x0, r.y0), (r.x1, r.y0), (r.x1, r.y1), (r.x0, r.y1)];
                    self.fill_polygon(&pts, density, n(&data[5], i, 45.0), &gc);
                } else {
                    self.d.rect(Rect::normalized(a.0, a.1, b.0, b.1), &gc);
                }
            } else {
                self.d.polyline(&[a, b], &gc);
            }
            if kind == "arrows" {
                let length = n(&data[4], 0, 0.25) * 72.0;
                let angle = n(&data[5], 0, 30.0).to_radians();
                let code = n(&data[6], 0, 2.0) as i32;
                if length <= 0.0 || code == 0 {
                    continue;
                }
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                if dx.hypot(dy) < 0.072 {
                    indeterminate = true;
                    continue;
                }
                let dir = dy.atan2(dx);
                for (bit, p, d) in [(1, a, dir), (2, b, dir + PI)] {
                    if code & bit != 0 {
                        let step = |t: f64| (p.0 + length * t.cos(), p.1 + length * t.sin());
                        self.d.polyline(&[step(d - angle), p, step(d + angle)], &gc);
                    }
                }
            }
        }
        indeterminate
    }

    fn polygons(&mut self, pts: &[Point], kind: &str, winding: bool, data: &[Vec<f64>]) {
        if kind == "polypath" {
            let mut gc = self.gc(0);
            gc.fill = recycle(&self.style.col, 0)
                .copied()
                .unwrap_or(Rgba::TRANSPARENT);
            gc.col = recycle(&self.style.border, 0)
                .copied()
                .unwrap_or_else(|| self.p.color("fg"));
            let parts: Vec<_> = path_parts(pts).collect();
            self.d.path(&parts, winding, &gc);
        } else {
            for (i, part) in path_parts(pts).enumerate() {
                let mut gc = self.gc(i);
                gc.fill = recycle(&self.style.col, i)
                    .copied()
                    .unwrap_or(Rgba::TRANSPARENT);
                gc.col = recycle(&self.style.border, i)
                    .copied()
                    .unwrap_or_else(|| self.p.color("fg"));
                let density = n(&data[2], i, -1.0);
                if density >= 0.0 {
                    self.fill_polygon(part, density, n(&data[3], i, 45.0), &gc);
                } else if n(&data[4], 0, 0.0) == 1.0 {
                    self.d.path(&[part], false, &gc);
                } else {
                    self.d.polygon(part, &gc);
                }
            }
        }
    }

    fn text(
        &mut self,
        pts: &[Point],
        ss: &[Option<String>],
        adj: &[f64],
        pos: &[f64],
        offset: f64,
    ) {
        for (i, pt) in pts.iter().copied().enumerate().filter(|(_, p)| finite(*p)) {
            let Some(Some(s)) = recycle(ss, i) else {
                continue;
            };
            let mut gc = self.gc(i);
            gc.col = self.style.color(i, self.base.col);
            let rot = n(&self.style.srt, i, self.p.real("srt"));
            let gap = offset * self.p.char_size(1) * 72.0;
            let (at, hadj, vadj) = match n(pos, i, 0.0) as i32 {
                1 => ((pt.0, pt.1 - gap), 0.5, 1.0),
                2 => ((pt.0 - gap, pt.1), 1.0, 0.5),
                3 => ((pt.0, pt.1 + gap), 0.5, 0.0),
                4 => ((pt.0 + gap, pt.1), 0.0, 0.5),
                _ => (
                    pt,
                    n(adj, 0, n(&self.style.adj, i, self.p.real("adj"))),
                    adj.get(1).copied().unwrap_or(0.5),
                ),
            };
            label(self.d, at, s, rot, hadj, vadj, &gc);
        }
    }

    fn margin(&mut self, data: &[Vec<f64>], ss: &[Option<String>], outer: bool) {
        let area = if outer { self.g.inner } else { self.g.plot };
        let [l, r, b, t] = area.map(|x| x * 72.0);
        for (i, s) in ss.iter().enumerate() {
            let Some(s) = s else {
                continue;
            };
            let side = n(&data[0], i, 3.0) as usize;
            let line = n(&data[1], i, 0.0) * self.p.line() * 72.0;
            let adj = recycle(&data[3], i)
                .filter(|value| value.is_finite())
                .copied()
                .unwrap_or_else(|| n(&self.style.adj, i, self.p.real("adj")));
            let at = n(&data[2], i, f64::NAN);
            let las = n(&data[4], i, self.p.real("las")) as u8;
            let vertical = side == 2 || side == 4;
            let coord = if at.is_finite() && !outer {
                let point = self.point(at, at);
                if vertical { point.1 } else { point.0 }
            } else if vertical {
                b + (t - b) * if at.is_finite() { at } else { adj }
            } else {
                l + (r - l) * if at.is_finite() { at } else { adj }
            };
            let (pt, vadj) = match side {
                1 => ((coord, b - line), 1.0),
                2 => ((l - line, coord), 0.0),
                3 => ((coord, t + line), 0.0),
                4 => ((r + line, coord), 1.0),
                _ => continue,
            };
            let rot = if matches!((las, vertical), (0, true) | (2, false) | (3, _)) {
                90.0
            } else {
                0.0
            };
            let mut gc = self.gc(i);
            gc.col = self.style.color(i, self.base.col);
            let vadj = recycle(&self.style.padj, i)
                .filter(|value| value.is_finite())
                .copied()
                .unwrap_or(vadj);
            label(self.d, pt, s, rot, adj, vadj, &gc);
        }
    }

    fn draw_box(&mut self, area: &str, kind: &str) {
        let region = match area {
            "plot" => self.g.plot,
            "figure" => self.g.figure,
            "inner" => self.g.inner,
            "outer" => self.g.device,
            _ => return,
        };
        let [l, r, b, t] = region.map(|x| x * 72.0);
        let gc = self.gc(0);
        let edges: &[(f64, f64)] = match kind.to_ascii_lowercase().as_str() {
            "o" => &[(l, b), (r, b), (r, t), (l, t), (l, b)],
            "l" => &[(l, t), (l, b), (r, b)],
            "7" => &[(l, t), (r, t), (r, b)],
            "c" => &[(r, t), (l, t), (l, b), (r, b)],
            "u" => &[(l, t), (l, b), (r, b), (r, t)],
            "]" => &[(l, t), (r, t), (r, b), (l, b)],
            _ => return,
        };
        self.d.polyline(edges, &gc);
    }

    fn symbols(&mut self, pts: &[Point], measures: &[f64], scale: &[f64], shape: &str) {
        if pts.is_empty() {
            return;
        }
        let count = measures.len() / pts.len();
        let sx = n(scale, 0, 1.0);
        let sy = n(scale, 1, sx);
        for (i, &(x, y)) in pts.iter().enumerate().filter(|(_, p)| finite(**p)) {
            let size = |col| {
                measures
                    .get(col * pts.len() + i)
                    .copied()
                    .unwrap_or(f64::NAN)
            };
            if (0..count).any(|col| !size(col).is_finite()) {
                continue;
            }
            let gc = self.gc(i);
            let dx = sx * size(0) / 2.0;
            match shape {
                "circles" => self
                    .d
                    .circle((x, y), if dx == 0.0 { 1.0 } else { dx * 2.0 }, &gc),
                "stars" => {
                    let points: Vec<_> = (0..count)
                        .map(|col| {
                            let (sin, cos) = (col as f64 * TAU / count as f64).sin_cos();
                            (x + sx * size(col) * sin, y + sx * size(col) * cos)
                        })
                        .collect();
                    self.d.polygon(&points, &gc);
                }
                "squares" | "rectangles" | "thermometers" | "boxplots" => {
                    let dy = if shape == "squares" {
                        dx
                    } else {
                        sy * size(1) / 2.0
                    };
                    let (left, right, bottom, top) = (x - dx, x + dx, y - dy, y + dy);
                    let region = Rect::normalized(left, bottom, right, top);
                    self.d.rect(region, &gc);
                    if shape == "thermometers" {
                        let (lo, hi) = if count == 3 {
                            (0.0, size(2))
                        } else {
                            (size(2), size(3))
                        };
                        let fill = Rect::normalized(
                            left,
                            bottom + 2.0 * dy * lo.clamp(0.0, 1.0),
                            right,
                            bottom + 2.0 * dy * hi.clamp(0.0, 1.0),
                        );
                        self.d.rect(
                            fill,
                            &GContext {
                                fill: gc.col,
                                col: Rgba::TRANSPARENT,
                                ..gc
                            },
                        );
                    } else if shape == "boxplots" {
                        let median = bottom + 2.0 * dy * size(4);
                        let lower = bottom - sy * size(2);
                        let upper = top + sy * size(3);
                        for line in [
                            [(left, median), (right, median)],
                            [(x, bottom), (x, lower)],
                            [(x, top), (x, upper)],
                            [(x - dx / 2.0, lower), (x + dx / 2.0, lower)],
                            [(x - dx / 2.0, upper), (x + dx / 2.0, upper)],
                        ] {
                            self.d.polyline(&line, &gc);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn raster(&mut self, data: &[Vec<f64>], pixels: &[Rgba]) {
        let a = self.point(n(&data[0], 0, 0.0), n(&data[1], 0, 0.0));
        let b = self.point(n(&data[2], 0, 0.0), n(&data[3], 0, 0.0));
        if !finite(a) || !finite(b) {
            return;
        }
        let rows = n(&data[4], 0, 0.0) as usize;
        let cols = n(&data[4], 1, 0.0) as usize;
        if rows == 0 || cols == 0 || rows.checked_mul(cols) != Some(pixels.len()) {
            return;
        }
        let w = (b.0 - a.0) / cols as f64;
        let h = (b.1 - a.1) / rows as f64;
        let angle = n(&data[5], 0, 0.0).to_radians();
        let (sin, cos) = angle.sin_cos();
        let mut gc = self.base.clone();
        gc.col = Rgba::TRANSPARENT;
        for (i, &pixel) in pixels.iter().enumerate() {
            if pixel.is_transparent() {
                continue;
            }
            let x = w * (i / rows) as f64;
            let y = h * (rows - 1 - i % rows) as f64;
            gc.fill = pixel;
            if angle == 0.0 {
                self.d.rect(
                    Rect::normalized(a.0 + x, a.1 + y, a.0 + x + w, a.1 + y + h),
                    &gc,
                );
            } else {
                let vertices = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)]
                    .map(|(x, y)| (a.0 + x * cos - y * sin, a.1 + x * sin + y * cos));
                self.d.polygon(&vertices, &gc);
            }
        }
    }
}

pub(super) fn draw(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    require_frame(ctx, call)?;
    let op = string(ctx, args.get(0))?;
    let data = args.get(1);
    let options = args.get(2);
    let mut style = Style::parse(ctx, call, options)?;
    let mut values = Vec::with_capacity(7);
    let mut label_values = Vec::new();
    let mut mode = String::new();
    let mut variant = String::new();
    let mut raster = Vec::new();
    for i in 0..7 {
        let value = element(ctx, data, i)?;
        values.push(match (op.as_str(), i) {
            ("plot_xy" | "polypath", 2) | ("box", 0) | ("symbols", 4) => {
                mode = string(ctx, value)?;
                Vec::new()
            }
            ("box", 1) => {
                variant = string(ctx, value)?;
                Vec::new()
            }
            ("text", 2) | ("margin", 5) => {
                label_values = strings(ctx, value)?;
                Vec::new()
            }
            ("raster" | "native_raster", 6) => {
                let limit = ctx.host().limits().max_vector_bytes / 256;
                if ctx.session().xlength(value)? > limit {
                    return Err(ctx.stop(
                        call.err_call(),
                        "raster is too large for this graphics device",
                    ));
                }
                raster = if op == "native_raster" {
                    let (value, _) = ctx.session_mut().coerce(value, SexpType::Int)?;
                    ctx.session()
                        .int(value)?
                        .iter()
                        .map(|pixel| {
                            let [r, g, b, a] = pixel.to_le_bytes();
                            Rgba::new(r, g, b, a)
                        })
                        .collect()
                } else {
                    colors(ctx, value)?
                };
                Vec::new()
            }
            _ => nums(ctx, value)?,
        });
    }
    if op == "plot_xy" && !["p", "l", "b", "c", "o", "s", "S", "h", "n"].contains(&mode.as_str()) {
        return Err(ctx.stop(call.err_call(), format!("invalid plot type '{mode}'")));
    }
    if op == "arrows" && !(0.0..=3.0).contains(&n(&values[6], 0, 2.0)) {
        return Err(ctx.stop(call.err_call(), "invalid arrow head specification"));
    }
    let slot = active(ctx, call, false)?;
    let (g, base, crop, custom) = {
        let p = state(slot)?;
        let g = p.geometry();
        let xpd = style.xpd.unwrap_or_else(|| match p.value("xpd") {
            Some(Param::Flag(v)) => *v,
            _ => Some(false),
        });
        let crop = if op == "margin" || (op == "box" && mode != "plot") {
            if n(&values[6], 0, 0.0) != 0.0 || op == "box" {
                g.device
            } else {
                g.figure
            }
        } else {
            match xpd {
                Some(false) => g.plot,
                Some(true) => g.figure,
                None => g.device,
            }
        };
        (g, context(p), crop, p.clip)
    };
    if op == "clip" {
        let p = state(slot)?;
        let a = user_point(p, g, n(&values[0], 0, 0.0), n(&values[2], 0, 0.0));
        let b = user_point(p, g, n(&values[1], 0, 0.0), n(&values[3], 0, 0.0));
        if !finite(a) || !finite(b) {
            return Err(ctx.stop(call.err_call(), "invalid clipping limits"));
        }
        let r = Rect::normalized(a.0, a.1, b.0, b.1);
        p.clip = Some([r.x0 / 72.0, r.x1 / 72.0, r.y0 / 72.0, r.y1 / 72.0]);
        slot.device.clip(r);
    } else {
        slot.device.clip(rect(
            if style.xpd.is_none() && !matches!(op.as_str(), "margin" | "box") {
                custom.unwrap_or(crop)
            } else {
                crop
            },
        ));
        let p = slot
            .par_data
            .as_ref()
            .and_then(|data| data.downcast_ref::<GPar>())
            .ok_or_else(|| {
                roxide_eval::Unwind::Internal(roxide_eval::InternalError {
                    message: "base graphics state disappeared".into(),
                })
            })?;
        if op == "margin" && style.col.is_empty() {
            style.col.push(p.color("fg"));
        }
        let mut drawing = Drawing {
            d: slot.device.as_mut(),
            p,
            g,
            style,
            base,
        };
        match op.as_str() {
            "plot_xy" => drawing.plot_xy(&values[0], &values[1], &mode),
            "segments" | "arrows" | "rect" => {
                if drawing.endpoints(&values, &op) {
                    ctx.warn_now(
                        call.err_call(),
                        "zero-length arrow is of indeterminate angle and so skipped",
                    )?;
                }
            }
            "polygon" | "polypath" => {
                let pts = drawing.xy(&values[0], &values[1]);
                drawing.polygons(&pts, &op, mode == "winding", &values);
            }
            "text" => {
                let pts = drawing.xy(&values[0], &values[1]);
                drawing.text(
                    &pts,
                    &label_values,
                    &values[3],
                    &values[4],
                    n(&values[5], 0, 0.5),
                );
            }
            "margin" => drawing.margin(&values, &label_values, n(&values[6], 0, 0.0) != 0.0),
            "box" => drawing.draw_box(
                &mode,
                if variant.is_empty() {
                    p.text("bty")
                } else {
                    &variant
                },
            ),
            "symbols" => {
                let pts = drawing.xy(&values[0], &values[1]);
                drawing.symbols(&pts, &values[2], &values[3], &mode);
            }
            "raster" | "native_raster" => drawing.raster(&values, &raster),
            _ => {}
        }
    }
    record(ctx, call, "C_rx_draw", &[args.get(0), data, options])?;
    Ok(SEXP_NIL)
}
