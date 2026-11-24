//! Draw grid grobs in device coordinates and measure their local geometry.

use roxide_eval::{Args, Call, Ctx, EvalResult, InternalError, Unwind};
use roxide_gc::SexpId;
use roxide_value::{SEXP_NIL, SexpType, ValueApi};

use crate::engine::{Device, GContext, GraphicsExt, Rect, Rgba};
use crate::grdevices::parse_rgba;
use crate::stroke::{LINE_TYPES, dash_pattern, line_end, line_join};

use super::{
    fields::*,
    measure::{self, Space, Units},
    unit::string_metric,
    viewport::Transform,
};

mod bounds;

use bounds::{Bounds, Box2, boundary};

type Point = (f64, f64);

fn finite((x, y): Point) -> bool {
    x.is_finite() && y.is_finite()
}

fn delta(a: Point, b: Point) -> Point {
    (a.0 - b.0, a.1 - b.1)
}

fn scalar(ctx: &mut Ctx, value: SexpId, i: usize) -> EvalResult {
    let n = ctx.session().xlength(value)?;
    if n <= 1 {
        return Ok(value);
    }
    if ctx.session().type_of(value)? == SexpType::Str {
        let id = ctx.session().str_slice(value)?[i % n];
        Ok(ctx.session_mut().alloc_str_from(&[id])?)
    } else {
        let val = real(ctx, value, i, f64::NAN)?;
        Ok(ctx.session_mut().scalar_real(val)?)
    }
}

fn color(ctx: &mut Ctx, value: SexpId, i: usize) -> EvalResult<Rgba> {
    let value = scalar(ctx, value, i)?;
    parse_rgba(ctx, value)
}

fn style(ctx: &mut Ctx, gp: SexpId, i: usize) -> EvalResult<GContext> {
    let mut gc = measure::font(ctx, gp, i)?;
    let col = get(ctx, gp, "col")?;
    let fill = get(ctx, gp, "fill")?;
    let alpha = num(ctx, gp, "alpha", i, 1.0)?;
    gc.col = color(ctx, col, i)?;
    gc.fill = color(ctx, fill, i)?;
    gc.col.a = (f64::from(gc.col.a) * alpha).round() as u8;
    gc.fill.a = (f64::from(gc.fill.a) * alpha).round() as u8;
    gc.lwd = num(ctx, gp, "lwd", i, 1.0)? * num(ctx, gp, "lex", i, 1.0)?;
    let lty = get(ctx, gp, "lty")?;
    let lty = scalar(ctx, lty, i)?;
    let (lty, _) = ctx.session_mut().coerce(lty, SexpType::Str)?;
    let lty = text(ctx, lty, 0)?.unwrap_or_default();
    let lty = match lty.as_bytes() {
        [digit @ b'0'..=b'6'] => LINE_TYPES[usize::from(digit - b'0')],
        _ => lty.as_str(),
    };
    if lty == "blank" {
        gc.col = Rgba::TRANSPARENT;
    }
    gc.lty = dash_pattern(lty);
    let end = get(ctx, gp, "lineend")?;
    gc.lend = line_end(text(ctx, end, i)?.as_deref().unwrap_or_default());
    let join = get(ctx, gp, "linejoin")?;
    gc.ljoin = line_join(text(ctx, join, i)?.as_deref().unwrap_or_default());
    gc.lmitre = num(ctx, gp, "linemitre", i, 10.0)?;
    Ok(gc)
}

#[derive(Clone, Copy)]
enum Shape {
    Empty,
    Rect,
    Circle,
    Text,
    Segments,
    Lines,
    Polyline,
    Polygon,
    Path,
    Points,
    Raster,
    Null,
}

struct Position {
    x: Units,
    y: Units,
}

impl Position {
    fn max_len(&self) -> usize {
        self.x.len.max(self.y.len)
    }
}

struct Extent {
    width: Units,
    height: Units,
}

enum Geometry {
    Box { at: Position, extent: Extent },
    Circle { at: Position, radius: Units },
    Segments { start: Position, end: Position },
    Points { at: Position, size: Units },
    Position(Position),
}

impl Geometry {
    fn position(&self) -> &Position {
        match self {
            Self::Box { at, .. }
            | Self::Circle { at, .. }
            | Self::Points { at, .. }
            | Self::Position(at) => at,
            Self::Segments { start, .. } => start,
        }
    }

    fn max_len(&self) -> usize {
        let len = self.position().max_len();
        match self {
            Self::Box { extent, .. } => len.max(extent.width.len).max(extent.height.len),
            Self::Circle { radius, .. } => len.max(radius.len),
            Self::Segments { end, .. } => len.max(end.max_len()),
            Self::Points { size, .. } => len.max(size.len),
            Self::Position(_) => len,
        }
    }
}

struct Grob {
    value: SexpId,
    shape: Shape,
    space: Space,
    transform: Transform,
    rotation: f64,
    justification: [f64; 2],
}

impl Grob {
    fn read(ctx: &mut Ctx, value: SexpId, node: SexpId, just: SexpId) -> EvalResult<Self> {
        let class = attr(ctx, value, "class")?;
        let kind = text(ctx, class, 0)?;
        let shape = match kind.as_deref() {
            Some("rect") => Shape::Rect,
            Some("circle") => Shape::Circle,
            Some("text") => Shape::Text,
            Some("segments") => Shape::Segments,
            Some("lines") => Shape::Lines,
            Some("polyline") => Shape::Polyline,
            Some("polygon") => Shape::Polygon,
            Some("pathgrob") => Shape::Path,
            Some("points") => Shape::Points,
            Some("rastergrob") => Shape::Raster,
            Some("null") => Shape::Null,
            _ => Shape::Empty,
        };
        Ok(Self {
            value,
            shape,
            space: Space::read(ctx, node)?,
            transform: Transform::read(ctx, node)?,
            rotation: num(ctx, node, "rotation", 0, 0.0)?,
            justification: [real(ctx, just, 0, 0.5)?, real(ctx, just, 1, 0.5)?],
        })
    }

    fn units(&self, ctx: &mut Ctx, name: &str) -> EvalResult<Units> {
        let value = get(ctx, self.value, name)?;
        Units::read(ctx, value)
    }

    fn position(&self, ctx: &mut Ctx, x: &str, y: &str) -> EvalResult<Position> {
        Ok(Position {
            x: self.units(ctx, x)?,
            y: self.units(ctx, y)?,
        })
    }

    fn inches(
        &self,
        ctx: &mut Ctx,
        units: &Units,
        axis: usize,
        pos: bool,
        i: usize,
    ) -> EvalResult<f64> {
        let item = units.at(ctx, i)?;
        self.space.measure(ctx, item, axis, pos, i)
    }

    fn point(&self, ctx: &mut Ctx, at: &Position, i: usize) -> EvalResult<Point> {
        Ok((
            self.inches(ctx, &at.x, 0, true, i)?,
            self.inches(ctx, &at.y, 1, true, i)?,
        ))
    }

    fn just(&self, ctx: &mut Ctx, i: usize) -> EvalResult<Point> {
        Ok((
            num(ctx, self.value, "hjust", i, self.justification[0])?,
            num(ctx, self.value, "vjust", i, self.justification[1])?,
        ))
    }

    fn radius(&self, ctx: &mut Ctx, units: &Units, i: usize) -> EvalResult<f64> {
        Ok(self
            .inches(ctx, units, 0, false, i)?
            .min(self.inches(ctx, units, 1, false, i)?)
            .abs())
    }

    fn geometry(&self, ctx: &mut Ctx) -> EvalResult<Geometry> {
        if matches!(self.shape, Shape::Segments) {
            return Ok(Geometry::Segments {
                start: self.position(ctx, "x0", "y0")?,
                end: self.position(ctx, "x1", "y1")?,
            });
        }
        let at = self.position(ctx, "x", "y")?;
        Ok(match self.shape {
            Shape::Rect | Shape::Raster => Geometry::Box {
                at,
                extent: Extent {
                    width: self.units(ctx, "width")?,
                    height: self.units(ctx, "height")?,
                },
            },
            Shape::Circle => Geometry::Circle {
                at,
                radius: self.units(ctx, "r")?,
            },
            Shape::Points => Geometry::Points {
                at,
                size: self.units(ctx, "size")?,
            },
            _ => Geometry::Position(at),
        })
    }

    fn rect(&self, ctx: &mut Ctx, extent: &Extent, (x, y): Point, i: usize) -> EvalResult<Box2> {
        let (jx, jy) = self.just(ctx, i)?;
        let mut w = if extent.width.len == 0 {
            self.space.size[0]
        } else {
            self.inches(ctx, &extent.width, 0, false, i)?
        };
        let mut h = if extent.height.len == 0 {
            self.space.size[1]
        } else {
            self.inches(ctx, &extent.height, 1, false, i)?
        };
        if matches!(self.shape, Shape::Raster) && (extent.width.len == 0 || extent.height.len == 0)
        {
            let image = get(ctx, self.value, "raster")?;
            let d = attr(ctx, image, "dim")?;
            let ratio = real(ctx, d, 1, 1.0)? / real(ctx, d, 0, 1.0)?;
            if extent.width.len == 0 && (extent.height.len != 0 || w > h * ratio) {
                w = h * ratio;
            }
            if extent.height.len == 0 {
                h = w / ratio;
            }
        }
        Ok(Box2::new(x - w * jx, y - h * jy, w, h))
    }

    fn text_box(
        &self,
        ctx: &mut Ctx,
        at: Point,
        label: &str,
        gc: &GContext,
        i: usize,
    ) -> EvalResult<TextBox> {
        let (jx, jy) = self.just(ctx, i)?;
        let rot = num(ctx, self.value, "rot", i, 0.0)?;
        let Some(slot) = ctx.ext::<GraphicsExt>().active() else {
            return Err(ctx.stop(SEXP_NIL, "no active graphics device"));
        };
        let device = slot.device.as_ref();
        let width = string_metric(device, gc, label, Some(14)) / 72.0;
        let height = string_metric(device, gc, label, Some(15)) / 72.0;
        let ascent = string_metric(device, gc, label, Some(16)) / 72.0;
        let descent = string_metric(device, gc, label, Some(17)) / 72.0;
        let (sin, cos) = rot.to_radians().sin_cos();
        let bounds = Box2::new(-jx * width, -jy * height, width, height);
        let corners = bounds
            .corners()
            .map(|(x, y)| (at.0 + x * cos - y * sin, at.1 + x * sin + y * cos));
        let baseline = (at.0 + jy * height * sin, at.1 - jy * height * cos);
        Ok(TextBox {
            corners,
            baseline,
            rot,
            jx,
            ascent,
            descent,
        })
    }

    fn device_point(&self, point: Point) -> Point {
        let (x, y) = self.transform.point(point);
        (x * 72.0, y * 72.0)
    }
}

struct TextBox {
    corners: [Point; 4],
    baseline: Point,
    rot: f64,
    jx: f64,
    ascent: f64,
    descent: f64,
}

fn checked_size(ctx: &mut Ctx, n: usize) -> EvalResult<usize> {
    if n > ctx.host().limits().max_vector_bytes / size_of::<Point>() {
        return Err(ctx.stop(SEXP_NIL, "vector size limit exceeded"));
    }
    Ok(n)
}

fn groups(
    ctx: &mut Ctx,
    grob: SexpId,
    key: &str,
    lengths: &str,
    n: usize,
) -> EvalResult<Vec<Vec<usize>>> {
    let runs = get(ctx, grob, lengths)?;
    let ids = get(ctx, grob, key)?;
    if runs != SEXP_NIL {
        let (runs, _) = ctx.session_mut().coerce(runs, SexpType::Real)?;
        let lengths: Vec<_> = ctx.session().real(runs)?.iter().collect();
        let mut groups = Vec::new();
        let mut start = 0_usize;
        for len in lengths {
            if !len.is_finite() || len < 0.0 || len.fract() != 0.0 {
                return Err(ctx.stop(SEXP_NIL, "invalid group lengths"));
            }
            let stop = start.saturating_add(len as usize);
            if stop > n {
                return Err(ctx.stop(SEXP_NIL, "invalid group lengths"));
            }
            groups.push((start..stop).collect());
            start = stop;
        }
        if start != n {
            return Err(ctx.stop(SEXP_NIL, "invalid group lengths"));
        }
        Ok(groups)
    } else if ids != SEXP_NIL {
        if ctx.session().xlength(ids)? != n {
            return Err(ctx.stop(SEXP_NIL, "invalid group identifiers"));
        }
        let mut groups = Vec::<Vec<usize>>::new();
        let mut positions = std::collections::HashMap::new();
        for i in 0..n {
            let id = real(ctx, ids, i, f64::NAN)?;
            let index = *positions.entry(id.to_bits()).or_insert(groups.len());
            if index == groups.len() {
                groups.push(Vec::new());
            }
            groups[index].push(i);
        }
        Ok(groups)
    } else {
        Ok(vec![(0..n).collect()])
    }
}

fn device(ctx: &mut Ctx) -> EvalResult<&mut (dyn Device + 'static)> {
    ctx.ext::<GraphicsExt>()
        .active_mut()
        .map(|slot| slot.device.as_mut())
        .ok_or_else(|| {
            Unwind::Internal(InternalError {
                message: "no active graphics device".into(),
            })
        })
}

struct Arrow {
    length: Units,
    settings: SexpId,
}

impl Arrow {
    fn read(ctx: &mut Ctx, grob: SexpId) -> EvalResult<Option<Self>> {
        let settings = get(ctx, grob, "arrow")?;
        if settings == SEXP_NIL {
            return Ok(None);
        }
        let length = get(ctx, settings, "length")?;
        Ok(Some(Self {
            length: Units::read(ctx, length)?,
            settings,
        }))
    }

    fn draw(
        &self,
        ctx: &mut Ctx,
        g: &Grob,
        pts: &[Point],
        i: usize,
        gc: &GContext,
    ) -> EvalResult<()> {
        let length = g.radius(ctx, &self.length, i)? * 72.0;
        let angle = num(ctx, self.settings, "angle", i, 30.0)?.to_radians();
        let ends = num(ctx, self.settings, "ends", i, 2.0)? as i32;
        let closed = num(ctx, self.settings, "type", i, 1.0)? == 2.0;
        if pts.len() < 2 || !length.is_finite() || !angle.is_finite() {
            return Ok(());
        }
        let last = pts.len() - 1;
        let dev = device(ctx)?;
        for (end, tip, previous) in [(1, pts[0], pts[1]), (2, pts[last], pts[last - 1])] {
            if ends != end && ends != 3 {
                continue;
            }
            let (dx, dy) = delta(previous, tip);
            if dx == 0.0 && dy == 0.0 {
                continue;
            }
            let direction = dy.atan2(dx);
            let wing = |angle: f64| (tip.0 + length * angle.cos(), tip.1 + length * angle.sin());
            let arrow = [wing(direction - angle), tip, wing(direction + angle)];
            if closed {
                dev.polygon(&arrow, gc);
            } else {
                dev.polyline(&arrow, gc);
            }
        }
        Ok(())
    }
}

fn draw_lines(ctx: &mut Ctx, g: &Grob, xy: &Position, n: usize) -> EvalResult<()> {
    let arrow = Arrow::read(ctx, g.value)?;
    let mut points = Vec::with_capacity(n);
    for i in 0..n {
        points.push(g.device_point(g.point(ctx, xy, i)?));
    }
    let indices = if matches!(g.shape, Shape::Lines) {
        vec![(0..n).collect()]
    } else {
        groups(ctx, g.value, "id", "id.lengths", n)?
    };
    if matches!(g.shape, Shape::Path) {
        let subpaths = groups(ctx, g.value, "pathId", "pathId.lengths", indices.len())?;
        let rule = get(ctx, g.value, "rule")?;
        let winding = text(ctx, rule, 0)?.as_deref() != Some("evenodd");
        for (i, ids) in subpaths.iter().enumerate() {
            let contours: Vec<Vec<Point>> = ids
                .iter()
                .map(|&id| {
                    indices[id]
                        .iter()
                        .map(|&index| points[index])
                        .filter(|&p| finite(p))
                        .collect()
                })
                .collect();
            let paths: Vec<_> = contours
                .iter()
                .map(Vec::as_slice)
                .filter(|p| p.len() >= 3)
                .collect();
            let gc = style(ctx, g.space.gp, i)?;
            device(ctx)?.path(&paths, winding, &gc);
        }
    } else {
        for (i, group) in indices.iter().enumerate() {
            let pts: Vec<_> = group.iter().map(|&index| points[index]).collect();
            let gc = style(ctx, g.space.gp, i)?;
            for part in pts.split(|&pt| !finite(pt)) {
                if matches!(g.shape, Shape::Polygon) {
                    if part.len() >= 3 {
                        device(ctx)?.polygon(part, &gc);
                    }
                } else if part.len() >= 2 {
                    device(ctx)?.polyline(part, &gc);
                    if let Some(arrow) = &arrow {
                        arrow.draw(ctx, g, part, i, &gc)?;
                    }
                }
            }
        }
    }
    Ok(())
}

fn draw_box(dev: &mut dyn Device, g: &Grob, bounds: Box2, gc: &GContext) {
    let points = bounds.corners().map(|p| g.device_point(p));
    if !points.into_iter().all(finite) {
        return;
    }
    if g.rotation % 90.0 == 0.0 {
        dev.rect(
            Rect::normalized(points[0].0, points[0].1, points[2].0, points[2].1),
            gc,
        );
    } else {
        dev.polygon(&points, gc);
    }
}

fn draw_raster(ctx: &mut Ctx, g: &Grob, bounds: Box2, mut gc: GContext) -> EvalResult<()> {
    let image = get(ctx, g.value, "raster")?;
    let dim = attr(ctx, image, "dim")?;
    let rows = real(ctx, dim, 0, 0.0)? as usize;
    let cols = real(ctx, dim, 1, 0.0)? as usize;
    if rows.checked_mul(cols) != Some(ctx.session().xlength(image)?) || rows == 0 || cols == 0 {
        return Ok(());
    }
    let (w, h) = (bounds.width() / cols as f64, bounds.height() / rows as f64);
    gc.col = Rgba::TRANSPARENT;
    for col in 0..cols {
        for row in 0..rows {
            gc.fill = color(ctx, image, col * rows + row)?;
            if gc.fill.is_transparent() {
                continue;
            }
            let x = bounds.low.0 + col as f64 * w;
            let y = bounds.low.1 + (rows - row - 1) as f64 * h;
            draw_box(device(ctx)?, g, Box2::new(x, y, w, h), &gc);
        }
    }
    Ok(())
}

fn point_symbol(dev: &mut dyn Device, (x, y): Point, size: f64, pch: i32, gc: &mut GContext) {
    let r = size * 0.375;
    let square = [
        (x - r, y - r),
        (x + r, y - r),
        (x + r, y + r),
        (x - r, y + r),
    ];
    let diamond = [(x - r, y), (x, y - r), (x + r, y), (x, y + r)];
    let triangle = [(x - r, y - r), (x + r, y - r), (x, y + r)];
    let inverted = [(x - r, y + r), (x + r, y + r), (x, y - r)];
    if matches!(pch, 15..=20) {
        gc.fill = gc.col;
    } else if !matches!(pch, 21..=25) {
        gc.fill = Rgba::TRANSPARENT;
    }
    match pch {
        0 | 15 | 22 => dev.polygon(&square, gc),
        1 | 16 | 19 | 21 => dev.circle((x, y), r, gc),
        20 => dev.circle((x, y), r * 0.65, gc),
        2 | 17 | 24 => dev.polygon(&triangle, gc),
        3 => {
            dev.polyline(&[(x - r, y), (x + r, y)], gc);
            dev.polyline(&[(x, y - r), (x, y + r)], gc);
        }
        4 => {
            dev.polyline(&[square[0], square[2]], gc);
            dev.polyline(&[square[1], square[3]], gc);
        }
        5 | 18 | 23 => dev.polygon(&diamond, gc),
        6 | 25 => dev.polygon(&inverted, gc),
        32..=255 => {
            if let Some(ch) = char::from_u32(pch as u32) {
                dev.text((x, y - r / 2.0), &ch.to_string(), 0.0, 0.5, gc);
            }
        }
        _ => {}
    }
}

pub(super) fn draw(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let g = Grob::read(ctx, args.get(0), args.get(1), args.get(2))?;
    if matches!(g.shape, Shape::Empty | Shape::Null) {
        return Ok(SEXP_NIL);
    }
    let geometry = g.geometry(ctx)?;
    let label = if matches!(g.shape, Shape::Text) {
        get(ctx, g.value, "label")?
    } else {
        SEXP_NIL
    };
    let n = geometry.max_len().max(ctx.session().xlength(label)?);
    let n = checked_size(ctx, n)?;
    if matches!(
        g.shape,
        Shape::Lines | Shape::Polyline | Shape::Polygon | Shape::Path
    ) {
        draw_lines(ctx, &g, geometry.position(), n)?;
        return Ok(SEXP_NIL);
    }
    let arrow = Arrow::read(ctx, g.value)?;
    let mut text_bounds: Vec<Box2> = Vec::new();
    let overlap = num(ctx, g.value, "check.overlap", 0, 0.0)? == 1.0;
    for i in 0..n {
        let at = g.point(ctx, geometry.position(), i)?;
        if !finite(at) {
            continue;
        }
        let mut gc = style(ctx, g.space.gp, i)?;
        match &geometry {
            Geometry::Box { extent, .. } => {
                let bounds = g.rect(ctx, extent, at, i)?;
                if matches!(g.shape, Shape::Raster) {
                    draw_raster(ctx, &g, bounds, gc)?;
                } else {
                    draw_box(device(ctx)?, &g, bounds, &gc);
                }
            }
            Geometry::Circle { radius, .. } => {
                let r = g.radius(ctx, radius, i)? * 72.0;
                if r.is_finite() {
                    device(ctx)?.circle(g.device_point(at), r, &gc);
                }
            }
            Geometry::Segments { end, .. } => {
                let end = g.point(ctx, end, i)?;
                if finite(end) {
                    let points = [g.device_point(at), g.device_point(end)];
                    device(ctx)?.polyline(&points, &gc);
                    if let Some(arrow) = &arrow {
                        arrow.draw(ctx, &g, &points, i, &gc)?;
                    }
                }
            }
            Geometry::Points { size, .. } => {
                let size = g.radius(ctx, size, i)? * 72.0;
                let symbols = get(ctx, g.value, "pch")?;
                if ctx.session().type_of(symbols)? == SexpType::Str {
                    if let Some(label) = text(ctx, symbols, i)? {
                        device(ctx)?.text(g.device_point(at), &label, g.rotation, 0.5, &gc);
                    }
                } else if size.is_finite() {
                    let pch = real(ctx, symbols, i, 1.0)?;
                    if pch.is_finite() {
                        point_symbol(device(ctx)?, g.device_point(at), size, pch as i32, &mut gc);
                    }
                }
            }
            Geometry::Position(_) if matches!(g.shape, Shape::Text) => {
                let Some(label) = text(ctx, label, i)? else {
                    continue;
                };
                let text_box = g.text_box(ctx, at, &label, &gc, i)?;
                if overlap {
                    let mut bounds = Bounds::default();
                    bounds.add(text_box.corners);
                    if let Some(bounds) = bounds.0 {
                        if text_bounds.iter().any(|old| bounds.overlaps(*old)) {
                            continue;
                        }
                        text_bounds.push(bounds);
                    }
                }
                device(ctx)?.text(
                    g.device_point(text_box.baseline),
                    &label,
                    g.rotation + text_box.rot,
                    text_box.jx,
                    &gc,
                );
            }
            _ => {}
        }
    }
    Ok(SEXP_NIL)
}

pub(super) fn bounds(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let g = Grob::read(ctx, args.get(0), args.get(1), args.get(2))?;
    let target_node = args.get(4);
    let target = Transform::read(ctx, target_node)?;
    let target_rotation = num(ctx, target_node, "rotation", 0, 0.0)?;
    let angle = real(ctx, args.get(3), 0, 0.0)? + target_rotation - g.rotation;
    let geometry = g.geometry(ctx)?;
    let label = if matches!(g.shape, Shape::Text) {
        get(ctx, g.value, "label")?
    } else {
        SEXP_NIL
    };
    let n = if matches!(g.shape, Shape::Empty) {
        0
    } else {
        geometry.max_len().max(ctx.session().xlength(label)?)
    };
    let n = checked_size(ctx, n)?;
    let mut bounds = Bounds::default();
    let mut points = Vec::new();
    let mut anchor = None;
    let (mut ascent, mut descent) = (0.0_f64, 0.0_f64);
    let mut circle = None;
    for i in 0..n {
        let at = g.point(ctx, geometry.position(), i)?;
        if !finite(at) {
            continue;
        }
        match &geometry {
            Geometry::Box { extent, .. } => points.extend(g.rect(ctx, extent, at, i)?.corners()),
            Geometry::Circle { radius, .. } => {
                let r = g.radius(ctx, radius, i)?;
                points.extend(Box2::new(at.0 - r, at.1 - r, 2.0 * r, 2.0 * r).corners());
                if n == 1 {
                    circle = Some((at, r));
                }
            }
            Geometry::Segments { end, .. } => points.extend([at, g.point(ctx, end, i)?]),
            Geometry::Position(_) if matches!(g.shape, Shape::Text) => {
                if anchor == Some(at) {
                    continue;
                }
                anchor = Some(at);
                let Some(label) = text(ctx, label, i)? else {
                    continue;
                };
                let gc = measure::font(ctx, g.space.gp, i)?;
                let text_box = g.text_box(ctx, at, &label, &gc, i)?;
                ascent = ascent.max(text_box.ascent);
                descent = descent.max(text_box.descent);
                points.extend(text_box.corners);
            }
            _ => points.push(at),
        }
    }
    bounds.add(points.iter().copied());
    let bounds = bounds
        .0
        .unwrap_or_else(|| Box2::new(g.space.size[0] / 2.0, g.space.size[1] / 2.0, 0.0, 0.0));
    let at = if let Some(((x, y), r)) = circle {
        let (sin, cos) = angle.to_radians().sin_cos();
        (x + r * cos, y + r * sin)
    } else {
        boundary(bounds, points, angle)
    };
    let (x, y) = g.transform.point(at);
    let m = target.0;
    let (dx, dy) = (x - m[2], y - m[5]);
    let result = [
        dx * m[0] + dy * m[3],
        dx * m[1] + dy * m[4],
        bounds.width(),
        bounds.height(),
        ascent,
        descent,
    ];
    Ok(ctx.session_mut().alloc_real_from(&result)?)
}
