//! Allocate fixed and relative grid layout tracks in inches.

use roxide_eval::{Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{SEXP_NIL, ValueApi};

use super::{
    fields::*,
    measure::{Space, Units},
};

#[derive(Clone, Copy)]
enum Track {
    Fixed(f64),
    Relative { weight: f64, respected: bool },
}

fn space_and_weight(tracks: &[Track], size: f64) -> (f64, f64) {
    tracks
        .iter()
        .fold((size, 0.0), |(space, total), track| match track {
            Track::Fixed(inches) => (space - inches, total),
            Track::Relative { weight, .. } => (space, total + weight),
        })
}

fn distribute(tracks: &[Track], available: f64, scale: f64) -> impl Iterator<Item = f64> + '_ {
    let (space, weight) =
        tracks
            .iter()
            .fold((available, 0.0), |(space, weight), track| match track {
                Track::Fixed(_) => (space, weight),
                Track::Relative {
                    weight: units,
                    respected: true,
                } => (space - units * scale, weight),
                Track::Relative {
                    weight: units,
                    respected: false,
                } => (space, weight + units),
            });
    let free_scale = if weight == 0.0 {
        0.0
    } else {
        space.max(0.0) / weight
    };
    tracks.iter().map(move |track| match track {
        Track::Fixed(inches) => *inches,
        Track::Relative { weight, respected } => {
            weight * if *respected { scale } else { free_scale }
        }
    })
}

fn distribute_axes(axes: &[Vec<Track>; 2], [width, height]: [f64; 2]) -> [Vec<f64>; 2] {
    let (x_space, x_weight) = space_and_weight(&axes[0], width);
    let (y_space, y_weight) = space_and_weight(&axes[1], height);
    let x_scale = if x_weight == 0.0 {
        f64::INFINITY
    } else {
        x_space / x_weight
    };
    let y_scale = if y_weight == 0.0 {
        f64::INFINITY
    } else {
        y_space / y_weight
    };
    let scale = x_scale.min(y_scale);
    let scale = if scale.is_finite() { scale } else { 0.0 };
    [
        distribute(&axes[0], x_space, scale).collect(),
        distribute(&axes[1], y_space, scale).collect(),
    ]
}

pub(super) fn tracks(ctx: &mut Ctx, layout: SexpId, space: Space) -> EvalResult {
    if layout == SEXP_NIL {
        return Ok(SEXP_NIL);
    }
    let cols = num(ctx, layout, "ncol", 0, 0.0)? as usize;
    let rows = num(ctx, layout, "nrow", 0, 0.0)? as usize;
    let respect = num(ctx, layout, "valid.respect", 0, 0.0)? as i32;
    let matrix = get(ctx, layout, "respect.mat")?;
    let limit = ctx.host().limits().max_vector_bytes;
    if cols.saturating_add(rows) > limit / size_of::<Track>()
        || rows.saturating_mul(cols) > limit / size_of::<i32>()
    {
        return Err(ctx.stop(SEXP_NIL, "vector size limit exceeded"));
    }
    let mut marks = [vec![respect == 1; cols], vec![respect == 1; rows]];
    if respect == 2 {
        let marks_matrix = ctx.session().int(matrix)?;
        for (i, mark) in marks_matrix.iter().take(rows * cols).enumerate() {
            if mark != 0 {
                marks[0][i / rows] = true;
                marks[1][i % rows] = true;
            }
        }
    }
    let mut axes = [Vec::with_capacity(cols), Vec::with_capacity(rows)];
    for (axis, name) in ["widths", "heights"].into_iter().enumerate() {
        let value = get(ctx, layout, name)?;
        let units = Units::read(ctx, value)?;
        for (i, &respected) in marks[axis].iter().enumerate() {
            let unit = units.at(ctx, i)?;
            let weight = unit.weight(ctx, 0)?;
            axes[axis].push(if weight.is_nan() {
                Track::Fixed(space.measure(ctx, unit, axis, false, i)?)
            } else {
                Track::Relative { weight, respected }
            });
        }
    }
    let [widths, heights] = distribute_axes(&axes, space.size);
    let widths = ctx.session_mut().alloc_real_from(&widths)?;
    let heights = ctx.session_mut().alloc_real_from(&heights)?;
    named(ctx, &[("widths", widths), ("heights", heights)])
}
