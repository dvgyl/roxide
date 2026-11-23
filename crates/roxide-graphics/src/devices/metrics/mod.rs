//! Helvetica metrics and Roman Simplex stroke geometry.

mod helvetica_afm;
mod hershey;

use crate::engine::GContext;

use helvetica_afm::{BOLD, BOLD_KERN, BOLD_OBLIQUE, OBLIQUE, REGULAR, REGULAR_KERN};

pub(super) fn face(gc: &GContext) -> usize {
    match gc.fontface {
        2..=4 => usize::from(gc.fontface) - 1,
        _ => 0,
    }
}

pub(super) fn size(gc: &GContext, pointsize: f64) -> f64 {
    let ps = if gc.ps.is_finite() && gc.ps > 0.0 {
        gc.ps
    } else {
        pointsize
    };
    ps * gc.cex
}

pub(super) fn latin1(c: char) -> u8 {
    match c as u32 {
        32..=126 | 160..=255 => c as u8,
        _ => b'?',
    }
}

fn glyph(c: u8, gc: &GContext) -> (u16, i16, i16) {
    let index = if c < 127 { c - 32 } else { c - 65 };
    [REGULAR, BOLD, OBLIQUE, BOLD_OBLIQUE][face(gc)][usize::from(index)]
}

pub(super) fn kern(left: u8, right: u8, gc: &GContext) -> i16 {
    let key = u16::from_be_bytes([left, right]);
    let pairs = if matches!(face(gc), 1 | 3) {
        BOLD_KERN
    } else {
        REGULAR_KERN
    };
    pairs
        .binary_search_by_key(&key, |p| p.0)
        .map_or(0, |i| pairs[i].1)
}

pub(super) fn width(s: &str, gc: &GContext, pointsize: f64, kerning: bool) -> f64 {
    let mut previous = None;
    let mut units = 0.0;
    for c in s.chars().map(latin1) {
        if kerning && let Some(left) = previous {
            units += f64::from(kern(left, c, gc));
        }
        units += f64::from(glyph(c, gc).0);
        previous = Some(c);
    }
    units * (size(gc, pointsize) / 1000.0)
}

pub(super) fn info(c: char, gc: &GContext, pointsize: f64) -> (f64, f64, f64) {
    let scale = size(gc, pointsize) / 1000.0;
    let (width, ascent, descent) = glyph(latin1(c), gc);
    (
        f64::from(ascent) * scale,
        f64::from(descent) * scale,
        f64::from(width) * scale,
    )
}

pub(super) fn text_origin(at: (f64, f64), width: f64, rot: f64, hadj: f64) -> (f64, f64) {
    if hadj == 0.0 {
        return at;
    }
    let (sin, cos) = rot.to_radians().sin_cos();
    (at.0 - width * hadj * cos, at.1 - width * hadj * sin)
}

pub(super) fn strokes(
    c: char,
    advance: f64,
    size: f64,
    italic: bool,
) -> impl Iterator<Item = Option<(f64, f64)>> {
    let c = match latin1(c) {
        160 => b' ',
        173 => b'-',
        c @ 32..=126 => c,
        _ => b'?',
    };
    let raw = hershey::SIMPLEX[usize::from(c - 32)];
    let left = f64::from(raw[0]);
    let span = f64::from(raw[1]) - left;
    raw[2..].as_chunks::<2>().0.iter().map(move |pair| {
        if pair == b" R" {
            None
        } else {
            // Roman Simplex has a baseline at y=9 and a cap height of 21 units.
            let y = (f64::from(b'[') - f64::from(pair[1])) * size * (0.718 / 21.0);
            let x = (f64::from(pair[0]) - left) * advance / span;
            Some((x + if italic { y * 0.21 } else { 0.0 }, y))
        }
    })
}
