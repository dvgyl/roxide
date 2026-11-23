//! Convert strokes to closed contours for the scanline filler.

use super::{
    super::output::{self, Point, valid_point},
    fill::Frame,
};
use crate::engine::{GContext, LineEnd, LineJoin, Rect};

pub(super) fn paint(
    frame: &mut Frame,
    contours: &[&[Point]],
    closed: bool,
    scale: f64,
    gc: &GContext,
) {
    let Some(width) = output::line_width(gc).map(|width| width * scale) else {
        return;
    };
    let radius = width / 2.0;
    if !radius.is_finite() || radius <= 0.0 {
        return;
    }

    let mut dash: Vec<_> = output::dashes(gc, width).collect();
    if !dash.len().is_multiple_of(2) {
        dash.extend_from_within(..);
    }
    let period: f64 = dash.iter().sum();
    if period < 0.25 || !period.is_finite() {
        dash.clear();
    }
    let mitre = if gc.lmitre.is_finite() {
        gc.lmitre.max(1.0)
    } else {
        10.0
    };
    let mut stroke = Stroke {
        radius,
        cap: gc.lend,
        join: gc.ljoin,
        mitre,
        polygons: Vec::new(),
    };
    for contour in contours {
        if !contour.iter().copied().all(valid_point) {
            continue;
        }
        let mut points = contour.to_vec();
        points.dedup();
        if closed && points.len() > 1 && points.first() == points.last() {
            points.pop();
        }
        if dash.is_empty() {
            stroke.outline(&points, closed);
        } else {
            stroke.dashed(&points, closed, &dash, period, frame.clip);
        }
    }
    let paths: Vec<_> = stroke.polygons.iter().map(Vec::as_slice).collect();
    frame.fill(&paths, true, gc.col);
}

pub(super) fn disk((x, y): Point, r: f64) -> Vec<Point> {
    let count = ((r.sqrt() * 5.0).ceil() as usize).clamp(12, 512);
    (0..count)
        .map(|i| {
            let (sin, cos) = (i as f64 * std::f64::consts::TAU / count as f64).sin_cos();
            (x + r * cos, y + r * sin)
        })
        .collect()
}

struct Stroke {
    radius: f64,
    cap: LineEnd,
    join: LineJoin,
    mitre: f64,
    polygons: Vec<Vec<Point>>,
}

impl Stroke {
    fn outline(&mut self, pts: &[Point], closed: bool) {
        let Some((&first, rest)) = pts.split_first() else {
            return;
        };
        let Some(&last) = rest.last() else {
            if self.cap == LineEnd::Round {
                self.polygons.push(disk(first, self.radius));
            }
            return;
        };
        for p in pts.windows(2) {
            self.segment(p[0], p[1]);
        }
        for p in pts.windows(3) {
            self.corner(p[0], p[1], p[2]);
        }
        if closed {
            self.segment(last, first);
            self.corner(pts[pts.len() - 2], last, first);
            self.corner(last, first, pts[1]);
        } else {
            self.cap(first, pts[1]);
            self.cap(last, pts[pts.len() - 2]);
        }
    }

    fn segment(&mut self, a: Point, b: Point) {
        if let Some(dir) = direction(a, b) {
            let normal = mul((-dir.1, dir.0), self.radius);
            self.polygons.push(vec![
                sub(a, normal),
                sub(b, normal),
                add(b, normal),
                add(a, normal),
            ]);
        }
    }

    fn cap(&mut self, at: Point, inside: Point) {
        match self.cap {
            LineEnd::Butt => {}
            LineEnd::Round => self.polygons.push(disk(at, self.radius)),
            LineEnd::Square => {
                if let Some(dir) = direction(inside, at) {
                    self.segment(at, add(at, mul(dir, self.radius)));
                }
            }
        }
    }

    fn corner(&mut self, a: Point, b: Point, c: Point) {
        let (Some(u), Some(v)) = (direction(a, b), direction(b, c)) else {
            return;
        };
        let cross = u.0 * v.1 - u.1 * v.0;
        if self.join == LineJoin::Round {
            self.polygons.push(disk(b, self.radius));
            return;
        }
        if cross.abs() < 1e-12 {
            return;
        }
        let side = -cross.signum() * self.radius;
        let p = add(b, mul((-u.1, u.0), side));
        let q = add(b, mul((-v.1, v.0), side));
        let mut outline = vec![b, p];
        if self.join == LineJoin::Mitre {
            let delta = sub(q, p);
            let tip = add(p, mul(u, (delta.0 * v.1 - delta.1 * v.0) / cross));
            if sub(tip, b).0.hypot(sub(tip, b).1) <= self.radius * self.mitre {
                outline.push(tip);
            }
        }
        outline.push(q);
        if cross < 0.0 {
            outline.reverse();
        }
        self.polygons.push(outline);
    }

    fn dashed(&mut self, pts: &[Point], closed: bool, pattern: &[f64], period: f64, clip: Rect) {
        let mut fragments: Vec<Vec<Point>> = Vec::new();
        let mut run = Vec::new();
        let mut phase = 0.0;
        let Some((&first, _)) = pts.split_first() else {
            return;
        };
        let edges = pts
            .iter()
            .zip(pts.iter().cycle().skip(1))
            .take(pts.len().saturating_sub(usize::from(!closed)));
        let margin = self.radius * self.mitre.max(2.0) + 1.0;
        for (&a, &b) in edges {
            let delta = sub(b, a);
            let length = delta.0.hypot(delta.1);
            if length == 0.0 || !length.is_finite() {
                continue;
            }
            let Some((start, end)) = visible(a, delta, clip, margin) else {
                push_run(&mut fragments, &mut run);
                phase = (phase + length.rem_euclid(period)).rem_euclid(period);
                continue;
            };
            let dir = mul(delta, 1.0 / length);
            let (mut at, end) = (start * length, end * length);
            let skip = (phase + at.rem_euclid(period)).rem_euclid(period);
            if start > 0.0 {
                push_run(&mut fragments, &mut run);
            }
            let mut rest = skip;
            let mut part = 0usize;
            // Skip the off-screen portion without walking its dash marks.
            while part + 1 < pattern.len() && rest >= pattern[part] {
                rest -= pattern[part];
                part += 1;
            }
            let mut remaining = pattern[part] - rest;
            let mut stalled = 0;
            while at < end {
                let next = (at + remaining).min(end);
                let on = part.is_multiple_of(2);
                if next > at {
                    stalled = 0;
                    if on {
                        if run.is_empty() {
                            run.push(if at == 0.0 { a } else { add(a, mul(dir, at)) });
                        }
                        run.push(if next == length {
                            b
                        } else {
                            add(a, mul(dir, next))
                        });
                    }
                } else {
                    stalled += 1;
                    if stalled == pattern.len() {
                        break;
                    }
                }
                if next < end || remaining <= end - at {
                    if on {
                        push_run(&mut fragments, &mut run);
                    }
                    part = (part + 1) % pattern.len();
                    remaining = pattern[part];
                }
                at = next;
            }
            if end < length {
                push_run(&mut fragments, &mut run);
            }
            phase = (phase + length.rem_euclid(period)).rem_euclid(period);
        }
        if closed && !run.is_empty() && run.last() == Some(&first) {
            if let Some(head) = fragments
                .first_mut()
                .filter(|head| head.first() == Some(&first))
            {
                run.pop();
                run.append(head);
                *head = std::mem::take(&mut run);
            } else if run.first() == Some(&first) {
                run.pop();
                self.outline(&run, true);
                return;
            }
        }
        push_run(&mut fragments, &mut run);
        for fragment in fragments {
            self.outline(&fragment, false);
        }
    }
}

fn push_run(fragments: &mut Vec<Vec<Point>>, run: &mut Vec<Point>) {
    if !run.is_empty() {
        fragments.push(std::mem::take(run));
    }
}

fn visible(a: Point, d: Point, clip: Rect, margin: f64) -> Option<(f64, f64)> {
    let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
    for (origin, step, min, max) in [(a.0, d.0, clip.x0, clip.x1), (a.1, d.1, clip.y0, clip.y1)] {
        if step == 0.0 {
            if origin < min - margin || origin > max + margin {
                return None;
            }
        } else {
            let p = (min - margin - origin) / step;
            let q = (max + margin - origin) / step;
            lo = lo.max(p.min(q));
            hi = hi.min(p.max(q));
        }
    }
    (lo <= hi).then_some((lo, hi))
}

fn direction(a: Point, b: Point) -> Option<Point> {
    let d = sub(b, a);
    let len = d.0.hypot(d.1);
    (len.is_finite() && len > 0.0).then(|| (d.0 / len, d.1 / len))
}

fn add(a: Point, b: Point) -> Point {
    (a.0 + b.0, a.1 + b.1)
}
fn sub(a: Point, b: Point) -> Point {
    (a.0 - b.0, a.1 - b.1)
}
fn mul(a: Point, k: f64) -> Point {
    (a.0 * k, a.1 * k)
}
