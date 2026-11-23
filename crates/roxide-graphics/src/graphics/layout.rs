use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_value::{SexpType, ValueApi};

use super::{active, attr, nums, par::state, recorded_call, strings};

#[derive(Clone, Copy)]
enum Track {
    Relative(f64),
    Centimetres(f64),
}

impl Track {
    fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let (number, fixed) = match text.strip_suffix("cm") {
            Some(number) => (number.trim_end(), true),
            None => (text, false),
        };
        let x: f64 = number.parse().ok()?;
        if !x.is_finite() || x < 0.0 {
            return None;
        }
        Some(if fixed {
            Self::Centimetres(x)
        } else {
            Self::Relative(x)
        })
    }

    fn inches(self, scale: f64) -> f64 {
        match self {
            Self::Relative(x) => x * scale,
            Self::Centimetres(x) => x / 2.54,
        }
    }
}

#[derive(Clone)]
pub(super) struct Layout {
    pub(super) rows: usize,
    pub(super) cols: usize,
    bounds: Vec<[usize; 4]>,
    widths: Vec<Track>,
    heights: Vec<Track>,
    respect: Respect,
}

#[derive(Clone)]
enum Respect {
    None,
    All,
    Cells { rows: Vec<bool>, cols: Vec<bool> },
}

impl Layout {
    pub(super) fn len(&self) -> usize {
        self.bounds.len()
    }

    pub(super) fn cell(&self, index: usize) -> [usize; 2] {
        let [l, _, b, _] = self.bounds[index];
        [b + 1, l + 1]
    }

    pub(super) fn figure(&self, index: usize, [width, height]: [f64; 2]) -> [f64; 4] {
        let [left, right, row, last_row] = self.bounds[index];
        let sx = scale(&self.widths, width);
        let sy = scale(&self.heights, height);
        let common = sx.min(sy);
        let (xs, ys) = match &self.respect {
            Respect::None => (
                edges(&self.widths, width, sx, None),
                edges(&self.heights, height, sy, None),
            ),
            Respect::All => (
                edges(&self.widths, width, common, None),
                edges(&self.heights, height, common, None),
            ),
            Respect::Cells { rows, cols } => (
                edges(&self.widths, width, common, Some(cols)),
                edges(&self.heights, height, common, Some(rows)),
            ),
        };
        [
            xs[left],
            xs[right + 1],
            1.0 - ys[last_row + 1],
            1.0 - ys[row],
        ]
    }
}

fn scale(tracks: &[Track], available: f64) -> f64 {
    let mut fixed = 0.0;
    let mut relative = 0.0;
    for track in tracks {
        match track {
            Track::Relative(x) => relative += x,
            Track::Centimetres(x) => fixed += x / 2.54,
        }
    }
    if relative == 0.0 {
        f64::INFINITY
    } else {
        (available - fixed) / relative
    }
}

fn edges(tracks: &[Track], available: f64, scale: f64, marked: Option<&[bool]>) -> Vec<f64> {
    let scale = if scale.is_finite() { scale } else { 0.0 };
    let mut sizes: Vec<_> = tracks.iter().map(|track| track.inches(scale)).collect();
    let spare = (available - sizes.iter().sum::<f64>()).max(0.0);
    if let Some(marked) = marked {
        // The surplus calculation uses the numeric centimetre value for fixed tracks.
        let weights: Vec<_> = tracks
            .iter()
            .zip(marked)
            .map(|(track, marked)| match track {
                Track::Centimetres(x) => Some(*x),
                Track::Relative(x) if !marked => Some(*x),
                _ => None,
            })
            .collect();
        let divisor = scale * weights.iter().flatten().sum::<f64>();
        if divisor > 0.0 {
            for (size, weight) in sizes.iter_mut().zip(weights) {
                if weight.is_some() {
                    *size *= 1.0 + spare / divisor;
                }
            }
        }
    }
    let mut pos = (available - sizes.iter().sum::<f64>()) * 0.5;
    let mut output = vec![pos / available];
    for size in sizes {
        pos += size;
        output.push(pos / available);
    }
    output
}

fn tracks(
    ctx: &mut Ctx,
    call: &Call,
    arg: roxide_gc::SexpId,
    n: usize,
    name: &str,
) -> EvalResult<Vec<Track>> {
    let input = strings(ctx, arg)?;
    (0..n)
        .map(|i| match input.get(i) {
            None => Ok(Track::Relative(1.0)),
            Some(value) => value
                .as_deref()
                .and_then(Track::parse)
                .ok_or_else(|| ctx.stop(call.err_call(), format!("invalid '{name}' value"))),
        })
        .collect()
}

pub(super) fn layout(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let dim = attr(ctx, args.get(0), "dim")?;
    let dims = nums(ctx, dim)?;
    let [rows, cols] = dims.as_slice() else {
        return Err(ctx.stop(call.err_call(), "'mat' must be a matrix"));
    };
    let (rows, cols) = (*rows as usize, *cols as usize);
    if rows == 0 || cols == 0 || rows > 200 || cols > 200 || rows * cols > 10007 {
        return Err(ctx.stop(call.err_call(), "too many rows or columns in layout"));
    }
    let ids = nums(ctx, args.get(0))?;
    if ids.len() != rows * cols || ids.iter().any(|x| !(0.0..10008.0).contains(x)) {
        return Err(ctx.stop(call.err_call(), "invalid layout matrix"));
    }
    let count = ids.iter().fold(0, |max, &id| max.max(id as usize));
    let mut bounds = vec![[cols, 0, rows, 0]; count];
    for (i, &id) in ids.iter().enumerate() {
        if let Some(b) = (id as usize)
            .checked_sub(1)
            .and_then(|id| bounds.get_mut(id))
        {
            let (r, c) = (i % rows, i / rows);
            *b = [b[0].min(c), b[1].max(c), b[2].min(r), b[3].max(r)];
        }
    }
    if let Some(i) = bounds.iter().position(|b| b[0] == cols) {
        return Err(ctx.stop(call.err_call(), format!("layout matrix must contain at least one reference\nto each of the values {{1 ... {count}}}\nmissing value {i}", i = i + 1)));
    }
    if count == 0 {
        return Err(ctx.stop(
            call.err_call(),
            "layout matrix must contain a positive figure number",
        ));
    }
    let size = attr(ctx, args.get(3), "dim")?;
    let marked = nums(ctx, args.get(3))?;
    let respect = if ctx.session().type_of(size)? == SexpType::Nil {
        match marked.as_slice() {
            [0.0] => Respect::None,
            [1.0] => Respect::All,
            _ => return Err(ctx.stop(call.err_call(), "invalid 'respect' argument")),
        }
    } else {
        if nums(ctx, size)? != [rows as f64, cols as f64] || marked.len() != ids.len() {
            return Err(ctx.stop(
                call.err_call(),
                "'respect' must be logical or matrix with same dimension as 'mat'",
            ));
        }
        let mut respects_rows = vec![false; rows];
        let mut respects_cols = vec![false; cols];
        for (i, x) in marked.into_iter().enumerate() {
            if x != 0.0 {
                respects_rows[i % rows] = true;
                respects_cols[i / rows] = true;
            }
        }
        Respect::Cells {
            rows: respects_rows,
            cols: respects_cols,
        }
    };
    let layout = Layout {
        rows,
        cols,
        bounds,
        respect,
        widths: tracks(ctx, call, args.get(1), cols, "widths")?,
        heights: tracks(ctx, call, args.get(2), rows, "heights")?,
    };
    let values = [args.get(0), args.get(1), args.get(2), args.get(3)];
    let entry = recorded_call(ctx, "C_rx_layout", &values)?;
    let slot = active(ctx, call, true)?;
    state(slot)?.set_layout(layout);
    slot.par_state = entry;
    slot.display_list.push(entry);
    Ok(ctx.session_mut().alloc_int_from(&[count as i32])?)
}
