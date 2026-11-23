//! Allocate fixed and relative grid layout tracks in inches.

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_value::ValueApi;

#[derive(Clone, Copy)]
enum Track {
    Fixed(f64),
    Relative { weight: f64, respected: bool },
}

fn axis(fixed: Vec<f64>, weights: Vec<f64>, respect: Vec<f64>) -> Option<Vec<Track>> {
    if fixed.len() != weights.len() || fixed.len() != respect.len() {
        return None;
    }
    Some(
        fixed
            .into_iter()
            .zip(weights)
            .zip(respect)
            .map(|((inches, weight), mark)| {
                if weight.is_nan() {
                    Track::Fixed(inches)
                } else {
                    Track::Relative {
                        weight,
                        respected: mark != 0.0,
                    }
                }
            })
            .collect(),
    )
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

pub(super) fn tracks(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let size = super::numbers(ctx, call, args.get(0))?;
    let [width, height] = size.as_slice() else {
        return Err(ctx.stop(call.err_call(), "invalid layout size"));
    };
    let mut axes = Vec::with_capacity(2);
    for offset in [1, 4] {
        let fixed = super::numbers(ctx, call, args.get(offset))?;
        let weights = super::numbers(ctx, call, args.get(offset + 1))?;
        let respect = super::numbers(ctx, call, args.get(offset + 2))?;
        axes.push(
            axis(fixed, weights, respect)
                .ok_or_else(|| ctx.stop(call.err_call(), "invalid layout tracks"))?,
        );
    }
    if axes[0].len().saturating_add(axes[1].len())
        > ctx.host().limits().max_vector_bytes / size_of::<f64>()
    {
        return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
    }
    let (x_space, x_weight) = space_and_weight(&axes[0], *width);
    let (y_space, y_weight) = space_and_weight(&axes[1], *height);
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
    let result: Vec<_> = distribute(&axes[0], x_space, scale)
        .chain(distribute(&axes[1], y_space, scale))
        .collect();
    Ok(ctx.session_mut().alloc_real_from(&result)?)
}
