use std::f64::consts::PI;

use super::{Call, Ctx, Drawing, EvalResult, GContext, Point, Rect, Rgba, finite, n, recycle};

#[derive(Clone, Copy)]
pub(super) struct Endpoints<'a> {
    x0: &'a [f64],
    y0: &'a [f64],
    x1: &'a [f64],
    y1: &'a [f64],
}

pub(super) enum EndpointCommand<'a> {
    Segments(Endpoints<'a>),
    Arrows {
        endpoints: Endpoints<'a>,
        length: f64,
        angle: f64,
        code: i32,
    },
    Rect {
        endpoints: Endpoints<'a>,
        density: &'a [f64],
        angle: &'a [f64],
    },
}

impl<'a> EndpointCommand<'a> {
    pub(super) fn parse(
        ctx: &mut Ctx,
        call: &Call,
        op: &str,
        values: &'a [Vec<f64>; 7],
    ) -> EvalResult<Option<Self>> {
        let [x0, y0, x1, y1, fifth, sixth, seventh] = values;
        let endpoints = Endpoints { x0, y0, x1, y1 };
        Ok(Some(match op {
            "segments" => Self::Segments(endpoints),
            "arrows" => {
                let code = n(seventh, 0, 2.0);
                if !(0.0..=3.0).contains(&code) {
                    return Err(ctx.stop(call.err_call(), "invalid arrow head specification"));
                }
                Self::Arrows {
                    endpoints,
                    length: n(fifth, 0, 0.25) * 72.0,
                    angle: n(sixth, 0, 30.0).to_radians(),
                    code: code as i32,
                }
            }
            "rect" => Self::Rect {
                endpoints,
                density: fifth,
                angle: sixth,
            },
            _ => return Ok(None),
        }))
    }
}

impl Drawing<'_> {
    pub(super) fn endpoints(&mut self, command: EndpointCommand<'_>) -> bool {
        let Endpoints { x0, y0, x1, y1 } = match command {
            EndpointCommand::Segments(endpoints)
            | EndpointCommand::Arrows { endpoints, .. }
            | EndpointCommand::Rect { endpoints, .. } => endpoints,
        };
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
            match command {
                EndpointCommand::Segments(_) => self.d.polyline(&[a, b], &gc),
                EndpointCommand::Arrows {
                    length,
                    angle,
                    code,
                    ..
                } => {
                    self.d.polyline(&[a, b], &gc);
                    indeterminate |= self.arrow_heads(a, b, length, angle, code, &gc);
                }
                EndpointCommand::Rect { density, angle, .. } => {
                    gc.fill = recycle(&self.style.col, i)
                        .copied()
                        .unwrap_or(Rgba::TRANSPARENT);
                    gc.col = recycle(&self.style.border, i)
                        .copied()
                        .unwrap_or_else(|| self.p.color("fg"));
                    let density = n(density, i, -1.0);
                    let r = Rect::normalized(a.0, a.1, b.0, b.1);
                    if density >= 0.0 {
                        let pts = [(r.x0, r.y0), (r.x1, r.y0), (r.x1, r.y1), (r.x0, r.y1)];
                        self.fill_polygon(&pts, density, n(angle, i, 45.0), &gc);
                    } else {
                        self.d.rect(r, &gc);
                    }
                }
            }
        }
        indeterminate
    }

    fn arrow_heads(
        &mut self,
        a: Point,
        b: Point,
        length: f64,
        angle: f64,
        code: i32,
        gc: &GContext,
    ) -> bool {
        if length <= 0.0 || code == 0 {
            return false;
        }
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        if dx.hypot(dy) < 0.072 {
            return true;
        }
        let dir = dy.atan2(dx);
        for (bit, p, d) in [(1, a, dir), (2, b, dir + PI)] {
            if code & bit != 0 {
                let step = |t: f64| (p.0 + length * t.cos(), p.1 + length * t.sin());
                self.d.polyline(&[step(d - angle), p, step(d + angle)], gc);
            }
        }
        false
    }
}
