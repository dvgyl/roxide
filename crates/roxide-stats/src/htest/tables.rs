use roxide_base::random::with_rng;
use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_math::{
    core::Flags,
    dist::{continuous::pchisq, discrete::dhyper},
    rng::Rng,
};
use roxide_value::{NA_REAL, SEXP_NIL, ValueApi};

use super::{UPPER, print::strings};
use crate::distn::support::{
    attr, limit, logical, number, numbers, set_attr, set_dim, set_dimnames,
};

mod fisher;

pub(super) use fisher::fisher;

pub(super) fn table(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let y = args.get(1);
    let nx = attr(ctx, x, "levels")?;
    let ny = attr(ctx, y, "levels")?;
    let rows = ctx.session().xlength(nx)?;
    let cols = ctx.session().xlength(ny)?;
    if rows < 2 || cols < 2 {
        return Err(ctx.stop(call.err_call(), "'x' and 'y' must have at least 2 levels"));
    }
    let len = rows.saturating_mul(cols);
    limit::<i32>(ctx, call, len)?;
    let xs = ctx.session().int(x)?;
    let ys = ctx.session().int(y)?;
    if xs.len().min(ys.len()) > i32::MAX as usize {
        return Err(ctx.stop(call.err_call(), "table count limit exceeded"));
    }
    let mut cells = vec![0_i32; len];
    for (i, j) in xs.iter().zip(ys.iter()) {
        if i > 0 && j > 0 && i as usize <= rows && j as usize <= cols {
            cells[(j as usize - 1) * rows + i as usize - 1] += 1;
        }
    }
    ctx.scoped(|ctx| {
        let result = ctx.session_mut().alloc_int_from(&cells)?;
        ctx.protect(result);
        set_dim(ctx, call, result, rows, cols)?;
        set_dimnames(ctx, result, nx, ny)?;
        let class = strings(ctx, ["table"])?;
        ctx.protect(class);
        set_attr(ctx, result, "class", class)?;
        Ok(result)
    })
}

fn matrix_shape(ctx: &mut Ctx, call: &Call, x: SexpId) -> EvalResult<Option<(usize, usize)>> {
    let d = attr(ctx, x, "dim")?;
    if d == SEXP_NIL {
        return Ok(None);
    }
    let dim = ctx.session().int(d)?;
    if dim.len() != 2 {
        return Ok(None);
    }
    let dims: Vec<_> = dim.iter().collect();
    if dims[0] <= 0
        || dims[1] <= 0
        || (dims[0] as usize).checked_mul(dims[1] as usize) != Some(ctx.session().xlength(x)?)
    {
        return Err(ctx.stop(call.err_call(), "invalid contingency table dimensions"));
    }
    Ok(Some((dims[0] as usize, dims[1] as usize)))
}

struct Margins<T> {
    rows: Vec<T>,
    cols: Vec<T>,
}

fn margins<T>(x: &[T], rows: usize, cols: usize) -> Margins<T>
where
    T: Copy + Default + std::ops::AddAssign,
{
    let mut result = Margins {
        rows: vec![T::default(); rows],
        cols: vec![T::default(); cols],
    };
    for (j, column) in x.chunks_exact(rows).enumerate() {
        for (i, &x) in column.iter().enumerate() {
            result.rows[i] += x;
            result.cols[j] += x;
        }
    }
    result
}

pub(super) fn replicates(ctx: &mut Ctx, call: &Call, b: f64, work: usize) -> EvalResult<usize> {
    if !b.is_finite() || b < 1.0 || b > 1_000_000.0 || b as usize > 50_000_000 / work.max(1) {
        return Err(ctx.stop(call.err_call(), "simulation size limit exceeded"));
    }
    Ok(b as usize)
}

fn chisq_score<T: Copy + Into<f64>>(x: &[T], e: &[f64], adjust: f64) -> f64 {
    x.iter()
        .zip(e)
        .map(|(&x, &e)| ((x.into() - e).abs() - adjust).powi(2) / e)
        .sum()
}

pub(super) fn chisq(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let input = args.get(0);
    let x = numbers(ctx, call, input)?;
    let total: f64 = x.iter().sum();
    if x.len() < 2
        || total <= 0.0
        || !total.is_finite()
        || x.iter().any(|&v| v < 0.0 || !v.is_finite())
    {
        return Err(ctx.stop(
            call.err_call(),
            "all entries of 'x' must be nonnegative and finite",
        ));
    }
    let correct = logical(ctx, call, args.get(2))? == Some(true);
    let simulate = args.get(3) != SEXP_NIL;
    let b = number(ctx, call, args.get(3))?;
    let mut probabilities = Vec::new();
    let mut table_margins = None;
    let (expected, variance, df, adjustment) = if let Some((r, c)) = matrix_shape(ctx, call, input)?
    {
        let m = margins(&x, r, c);
        let mut expected = Vec::with_capacity(x.len());
        let mut variance = Vec::with_capacity(x.len());
        for &col in &m.cols {
            for &row in &m.rows {
                let mean = (row * col) / total;
                expected.push(mean);
                variance.push(mean * (1.0 - row / total) * (1.0 - col / total));
            }
        }
        let positive = m.rows.iter().chain(&m.cols).all(|&v| v > 0.0);
        if simulate && !positive {
            ctx.warn_now(
                call.err_call(),
                "cannot compute simulated p-value with zero marginals",
            )?;
        }
        let correction = if correct && r == 2 && c == 2 && !(simulate && positive) {
            x.iter()
                .zip(&expected)
                .map(|(&x, &e)| (x - e).abs())
                .fold(0.5, f64::min)
        } else {
            0.0
        };
        if positive {
            table_margins = Some(m);
        }
        (expected, variance, ((r - 1) * (c - 1)) as f64, correction)
    } else {
        probabilities = numbers(ctx, call, args.get(1))?;
        if probabilities.len() != x.len()
            || probabilities.iter().any(|&p| !p.is_finite() || p < 0.0)
        {
            return Err(ctx.stop(call.err_call(), "invalid probabilities"));
        }
        (
            probabilities.iter().map(|&p| total * p).collect(),
            probabilities
                .iter()
                .map(|&p| total * p * (1.0 - p))
                .collect(),
            x.len() as f64 - 1.0,
            0.0,
        )
    };
    let statistic = chisq_score(&x, &expected, adjustment);
    let simulation = simulate && (table_margins.is_some() || !probabilities.is_empty());
    let p = if simulation {
        let observed = statistic * (1.0 - 64.0 * f64::EPSILON);
        if let Some(m) = table_margins {
            if total > 1_000_000.0 {
                return Err(ctx.stop(call.err_call(), "simulation size limit exceeded"));
            }
            let mut m = m.integer();
            // The last column receives the counts that are left after integer conversion.
            let mut remaining = m.rows.iter().sum::<i32>();
            if let Some((last, first)) = m.cols.split_last_mut() {
                for col in first {
                    *col = (*col).min(remaining);
                    remaining -= *col;
                }
                *last = remaining;
            }
            let mut undefined = !observed.is_finite();
            let p = simulate_tables(ctx, call, &m, b, |sample| {
                let score = chisq_score(sample, &expected, 0.0);
                undefined |= !score.is_finite();
                score >= observed
            })?;
            if undefined { NA_REAL } else { p }
        } else {
            let count = total as usize;
            let b = replicates(ctx, call, b, count.saturating_add(x.len()))?;
            limit::<u64>(ctx, call, count)?;
            let sum: f64 = probabilities.iter().sum();
            if !sum.is_finite() || sum <= 0.0 {
                return Err(ctx.stop(call.err_call(), "invalid probabilities"));
            }
            for p in &mut probabilities {
                *p /= sum;
            }
            let (hits, undefined) = with_rng(ctx, call, |rng| {
                let mut hits = 0;
                let mut undefined = !observed.is_finite();
                let mut sample = vec![0_i32; probabilities.len()];
                for _ in 0..b {
                    sample.fill(0);
                    for category in rng.sample_prob_with_replacement(&probabilities, count) {
                        sample[category as usize - 1] += 1;
                    }
                    let score = chisq_score(&sample, &expected, 0.0);
                    undefined |= !score.is_finite();
                    hits += usize::from(score >= observed);
                }
                (hits, undefined)
            })?;
            if undefined {
                NA_REAL
            } else {
                (hits + 1) as f64 / (b + 1) as f64
            }
        }
    } else {
        if expected.iter().any(|&e| e < 5.0) {
            ctx.warn_now(
                call.err_call(),
                "Chi-squared approximation may be incorrect",
            )?;
        }
        pchisq(statistic, df, UPPER, &mut Flags::default())
    };
    ctx.scoped(|ctx| {
        let out = ctx.session_mut().alloc_list(6)?;
        ctx.protect(out);
        let stat = ctx.session_mut().scalar_real(statistic)?;
        ctx.session_mut().list_set(out, 0, stat)?;
        let df = if simulation {
            ctx.session_mut().scalar_lgl(roxide_value::RLogical::NA)?
        } else if probabilities.is_empty() {
            ctx.session_mut().scalar_int(df as i32)?
        } else {
            ctx.session_mut().scalar_real(df)?
        };
        ctx.session_mut().list_set(out, 1, df)?;
        let pv = ctx.session_mut().scalar_real(p)?;
        ctx.session_mut().list_set(out, 2, pv)?;
        let mode = ctx.session_mut().scalar_int(if simulation {
            2
        } else {
            i32::from(adjustment > 0.0)
        })?;
        ctx.session_mut().list_set(out, 3, mode)?;
        for (i, values) in [expected, variance].iter().enumerate() {
            let value = ctx.session_mut().alloc_real_from(values)?;
            ctx.session_mut().list_set(out, i + 4, value)?;
            for name in ["names", "dim", "dimnames"] {
                let a = attr(ctx, input, name)?;
                if a != SEXP_NIL {
                    set_attr(ctx, value, name, a)?;
                }
            }
        }
        Ok(out)
    })
}

impl Margins<f64> {
    fn integer(&self) -> Margins<i32> {
        Margins {
            rows: self.rows.iter().map(|&x| x as i32).collect(),
            cols: self.cols.iter().map(|&x| x as i32).collect(),
        }
    }
}

impl Margins<i32> {
    fn remove_zeros(&mut self) {
        self.rows.retain(|&v| v != 0);
        self.cols.retain(|&v| v != 0);
    }
}

pub(super) fn simulate_partition(
    ctx: &mut Ctx,
    call: &Call,
    first: usize,
    second: usize,
    groups: &[usize],
    b: f64,
    hit: impl FnMut(&[i32]) -> bool,
) -> EvalResult<f64> {
    let margins = Margins {
        rows: vec![first as i32, second as i32],
        cols: groups.iter().map(|&size| size as i32).collect(),
    };
    simulate_tables(ctx, call, &margins, b, hit)
}

fn simulate_tables(
    ctx: &mut Ctx,
    call: &Call,
    margins: &Margins<i32>,
    b: f64,
    mut hit: impl FnMut(&[i32]) -> bool,
) -> EvalResult<f64> {
    let r = margins.rows.len();
    let c = margins.cols.len();
    let sum: i32 = margins.rows.iter().sum();
    let work = (r * c).saturating_mul(1 + (f64::from(sum).sqrt() as usize).min(1000));
    let b = replicates(ctx, call, b, work)?;
    let hits = with_rng(ctx, call, |rng| {
        let mut remaining = margins.cols.clone();
        let mut table = vec![0; r * c];
        let mut hits = 0;
        for _ in 0..b {
            remaining.copy_from_slice(&margins.cols);
            let mut total = sum;
            for (i, &row) in margins.rows[..r - 1].iter().enumerate() {
                let mut slots = row;
                let mut pool = total;
                for j in 0..c - 1 {
                    let col = remaining[j];
                    let count = if pool == 0 {
                        0
                    } else {
                        sample_cell(rng, slots, col, pool)
                    };
                    table[j * r + i] = count;
                    slots -= count;
                    pool -= col;
                    remaining[j] -= count;
                }
                table[(c - 1) * r + i] = slots;
                remaining[c - 1] -= slots;
                total -= row;
            }
            for (j, &col) in remaining.iter().enumerate() {
                table[j * r + r - 1] = col;
            }
            hits += usize::from(hit(&table));
        }
        hits
    })?;
    Ok((hits + 1) as f64 / (b + 1) as f64)
}

fn sample_cell(rng: &mut Rng, row: i32, col: i32, sum: i32) -> i32 {
    let low = (row - (sum - col)).max(0);
    let high = row.min(col);
    let mode = ((f64::from(row) * f64::from(col) / f64::from(sum)).round() as i32).clamp(low, high);
    let mut remaining = rng.unif_rand();
    let mass = |cell| {
        dhyper(
            f64::from(cell),
            f64::from(col),
            f64::from(sum - col),
            f64::from(row),
            false,
            &mut Flags::default(),
        )
    };
    let p = mass(mode);
    if remaining <= p {
        return mode;
    }
    remaining -= p;
    let mut up = p;
    let mut down = p;
    for step in 1..=(high - mode).max(mode - low) {
        let upper = mode + step;
        let lower = mode - step;
        if upper <= high {
            let k = f64::from(upper - 1);
            up *= (f64::from(col) - k) * (f64::from(row) - k)
                / ((k + 1.0) * (f64::from(sum - col - row) + k + 1.0));
            remaining -= up;
            if remaining <= 0.0 {
                return upper;
            }
        }
        if lower >= low {
            let k = f64::from(lower + 1);
            down *= k * (f64::from(sum - col - row) + k)
                / ((f64::from(col) - k + 1.0) * (f64::from(row) - k + 1.0));
            remaining -= down;
            if remaining <= 0.0 {
                return lower;
            }
        }
    }
    mode
}
