//! Scanline coverage and source-over blending.

use super::super::output::{Point, valid_point};
use crate::engine::{Rect, Rgba};

pub(super) struct Frame {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) pixels: Vec<[u8; 4]>,
    pub(super) clip: Rect,
}

impl Frame {
    pub(super) fn reset(&mut self, bg: Rgba) {
        self.clip = Rect::normalized(0.0, 0.0, f64::from(self.width), f64::from(self.height));
        self.pixels.fill(premultiply(bg));
    }

    pub(super) fn fill(&mut self, contours: &[&[Point]], winding: bool, color: Rgba) {
        if color.is_transparent() || self.clip.x0 >= self.clip.x1 || self.clip.y0 >= self.clip.y1 {
            return;
        }
        let mut bottom = self.clip.y1;
        let mut top = self.clip.y0;
        let mut left = self.clip.x1;
        let mut right = self.clip.x0;
        let mut edges = Vec::new();
        for points in contours
            .iter()
            .filter(|pts| pts.len() >= 3 && pts.iter().copied().all(valid_point))
        {
            for (&a, &b) in points.iter().zip(points.iter().cycle().skip(1)) {
                left = left.min(a.0);
                right = right.max(a.0);
                if a.1 != b.1 {
                    let (low, high, sign) = if a.1 < b.1 { (a, b, 1) } else { (b, a, -1) };
                    bottom = bottom.min(low.1);
                    top = top.max(high.1);
                    edges.push((low, high, sign));
                }
            }
        }
        let bottom = bottom.max(self.clip.y0).floor() as usize;
        let top = top.min(self.clip.y1).ceil() as usize;
        let left = left.max(self.clip.x0).floor() as usize;
        let right = right.min(self.clip.x1).ceil() as usize;
        if bottom >= top || left >= right || edges.is_empty() {
            return;
        }

        edges.sort_unstable_by(|a, b| a.0.1.total_cmp(&b.0.1));
        let mut next = 0;
        let mut active: Vec<(Point, Point, i32)> = Vec::new();
        let mut crossings = Vec::new();
        let mut coverage = vec![0.0; right - left];
        let src = premultiply(color);
        for row in bottom..top {
            coverage.fill(0.0);
            let lo = (row as f64).max(self.clip.y0);
            let hi = (row as f64 + 1.0).min(self.clip.y1);
            let step = (hi - lo) / 4.0;
            for sample in 0..4 {
                let y = lo + (f64::from(sample) + 0.5) * step;
                active.retain(|(_, high, _)| high.1 > y);
                while let Some(&(low, high, sign)) = edges.get(next).filter(|edge| edge.0.1 <= y) {
                    if high.1 > y {
                        active.push((low, high, sign));
                    }
                    next += 1;
                }
                crossings.clear();
                for &(low, high, sign) in &active {
                    let t = (y - low.1) / (high.1 - low.1);
                    let x = low.0 * (1.0 - t) + high.0 * t;
                    if x.is_finite() {
                        crossings.push((x, sign));
                    }
                }
                crossings.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
                let mut count = 0i64;
                let mut previous = f64::NEG_INFINITY;
                for &(x, sign) in &crossings {
                    if (winding && count != 0) || (!winding && count % 2 != 0) {
                        let start = previous.max(self.clip.x0);
                        let end = x.min(self.clip.x1);
                        add_span(&mut coverage, left, start, end, step);
                    }
                    count += i64::from(sign);
                    previous = x;
                }
            }
            let index = (self.height as usize - 1 - row) * self.width as usize + left;
            for (dst, &area) in self.pixels[index..index + coverage.len()]
                .iter_mut()
                .zip(&coverage)
            {
                if area > 0.0 {
                    blend(dst, src, f64::from(area));
                }
            }
        }
    }
}

fn add_span(coverage: &mut [f32], left: usize, start: f64, end: f64, step: f64) {
    if end <= start {
        return;
    }
    let first = (start.floor() as usize).saturating_sub(left);
    let last = (end.ceil() as usize)
        .saturating_sub(left)
        .min(coverage.len());
    for (index, area) in coverage.iter_mut().enumerate().take(last).skip(first) {
        let pixel = (left + index) as f64;
        *area += ((end.min(pixel + 1.0) - start.max(pixel)) * step) as f32;
    }
}

fn multiply(value: u8, alpha: u8) -> u8 {
    ((u16::from(value) * u16::from(alpha) + 127) / 255) as u8
}

fn premultiply(c: Rgba) -> [u8; 4] {
    [
        multiply(c.r, c.a),
        multiply(c.g, c.a),
        multiply(c.b, c.a),
        c.a,
    ]
}

fn blend(dst: &mut [u8; 4], src: [u8; 4], area: f64) {
    let coverage = (area.min(1.0) * 255.0).round() as u8;
    let remaining = 255 - multiply(src[3], coverage);
    for (component, source) in dst.iter_mut().zip(src) {
        *component = multiply(source, coverage).saturating_add(multiply(*component, remaining));
    }
}
