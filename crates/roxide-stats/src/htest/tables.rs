use std::{collections::HashMap, convert::Infallible};

use roxide_base::random::with_rng;
use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_math::{
    core::Flags,
    dist::{continuous::pchisq, discrete::dhyper},
    rng::Rng,
    roots::zeroin,
    special::lgamma,
};
use roxide_value::{NA_REAL, SEXP_NIL, ValueApi};

use super::{Alternative, UPPER, print::strings};
use crate::distn::support::{
    attr, limit, logical, number, numbers, set_attr, set_dim, set_dimnames,
};

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

pub(super) fn fisher(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let alt = Alternative::read(ctx, call, args.get(2))?;
    let or = number(ctx, call, args.get(1))?;
    let level = number(ctx, call, args.get(3))?;
    let b = number(ctx, call, args.get(4))?;
    let simulate = args.get(4) != SEXP_NIL;
    let workspace = number(ctx, call, args.get(5))?;
    let input = args.get(0);
    let x = numbers(ctx, call, input)?;
    let Some((rows, cols)) = matrix_shape(ctx, call, input)? else {
        return Err(ctx.stop(call.err_call(), "'x' must be a matrix"));
    };
    let total: f64 = x.iter().sum();
    if rows < 2
        || cols < 2
        || !total.is_finite()
        || total > f64::from(i32::MAX)
        || x.iter().any(|&x| x < 0.0 || x.fract() != 0.0)
    {
        return Err(ctx.stop(call.err_call(), "Fisher test table size limit exceeded"));
    }
    let x: Vec<i32> = x.into_iter().map(|x| x as i32).collect();
    let margin = margins(&x, rows, cols);
    if rows == 2 && cols == 2 {
        let distribution = Odds::new(ctx, call, &margin, x[0])?;
        let p = distribution.probability(or, alt);
        let estimate = if distribution.at == 0 {
            0.0
        } else if distribution.at + 1 == distribution.log_base.len() {
            f64::INFINITY
        } else {
            distribution.solve(
                |d| d.iter().enumerate().map(|(i, p)| i as f64 * p).sum::<f64>(),
                distribution.at as f64,
                true,
            )
        };
        let mut lo = 0.0;
        let mut hi = f64::INFINITY;
        if args.get(3) != SEXP_NIL {
            if ctx.session().xlength(args.get(3))? != 1 || !(level > 0.0 && level < 1.0) {
                return Err(ctx.stop(
                    call.err_call(),
                    "'conf.level' must be a single number between 0 and 1",
                ));
            }
            let a = alt.alpha(level);
            if alt != Alternative::Less && distribution.at > 0 {
                lo = distribution.solve(|p| p[distribution.at..].iter().sum(), a, true);
            }
            if alt != Alternative::Greater && distribution.at + 1 < distribution.log_base.len() {
                hi = distribution.solve(|p| p[..=distribution.at].iter().sum(), a, false);
            }
        }
        return Ok(ctx.session_mut().alloc_real_from(&[p, estimate, lo, hi])?);
    }
    if total == 0.0 && !simulate {
        return Err(ctx.stop(call.err_call(), "FEXACT error 3.\nAll elements of TABLE are zero.\nPRT and PRE are set to missing values."));
    }
    let score = |x: &[i32]| {
        x.iter()
            .map(|&v| lgamma(f64::from(v) + 1.0, &mut Flags::default()))
            .sum::<f64>()
    };
    let threshold = score(&x) - 1.0000001_f64.ln();
    let p = if simulate {
        if total > 1_000_000.0 {
            return Err(ctx.stop(call.err_call(), "simulation size limit exceeded"));
        }
        let margin = Margins {
            rows: margin.rows.into_iter().filter(|&v| v != 0).collect(),
            cols: margin.cols.into_iter().filter(|&v| v != 0).collect(),
        };
        if margin.rows.len() < 2 {
            return Err(ctx.stop(call.err_call(), "need 2 or more non-zero row marginals"));
        }
        if margin.cols.len() < 2 {
            return Err(ctx.stop(call.err_call(), "need 2 or more non-zero column marginals"));
        }
        simulate_tables(ctx, call, &margin, b, |sample| score(sample) >= threshold)?
    } else {
        if rows.saturating_mul(cols) > 256
            || total > 100_000.0
            || workspace < 1.0
            || !workspace.is_finite()
        {
            return Err(ctx.stop(
                call.err_call(),
                "Fisher exact enumeration size limit exceeded",
            ));
        }
        let hybrid = if args.get(6) == SEXP_NIL {
            None
        } else {
            let parameters = numbers(ctx, call, args.get(6))?;
            match parameters.as_slice() {
                &[expect, percent, emin]
                    if expect > 0.0 && (0.0..=100.0).contains(&percent) && emin >= 0.0 =>
                {
                    Some(Hybrid {
                        expect,
                        percent,
                        emin,
                    })
                }
                _ => return Err(ctx.stop(call.err_call(), "invalid Fisher hybrid parameters")),
            }
        };
        limit::<f64>(ctx, call, total as usize + 1)?;
        let mut log_factorial = Vec::with_capacity(total as usize + 1);
        log_factorial.push(0.0);
        for n in 1..=total as usize {
            log_factorial.push(log_factorial[n - 1] + (n as f64).ln());
        }
        let mut rows: Vec<_> = margin.rows.into_iter().filter(|&v| v != 0).collect();
        let mut cols: Vec<_> = margin.cols.into_iter().filter(|&v| v != 0).collect();
        if rows.len() > cols.len() {
            std::mem::swap(&mut rows, &mut cols);
        }
        rows.sort_unstable();
        cols.sort_unstable();
        let mut enumeration = Enumeration {
            log_factorial,
            cols,
            // Fixed-margin probability comparisons allow for roundoff in the log factorials.
            threshold: score(&x) - 3.45254e-7,
            budget: (workspace as usize).saturating_mul(10).min(2_000_000),
            bounds: HashMap::new(),
            bounds_limit: ctx.host().limits().max_vector_bytes
                / (96 + size_of::<i32>() * rows.len()),
            hybrid,
        };
        enumeration
            .tail(&mut rows, 0, 0.0, 1.0)
            .ok_or_else(|| {
                ctx.stop(
                    call.err_call(),
                    "Fisher exact enumeration size limit exceeded",
                )
            })?
            .clamp(0.0, 1.0)
    };
    Ok(ctx.session_mut().alloc_real_from(&[p])?)
}

struct Odds {
    log_base: Vec<f64>,
    at: usize,
}

impl Odds {
    fn new(ctx: &mut Ctx, call: &Call, m: &Margins<i32>, at: i32) -> EvalResult<Self> {
        let min = (m.rows[0] - m.cols[1]).max(0);
        let max = m.rows[0].min(m.cols[0]);
        let len = (max - min) as usize + 1;
        if len > 100_000 {
            return Err(ctx.stop(
                call.err_call(),
                "Fisher exact test support size limit exceeded",
            ));
        }
        limit::<f64>(ctx, call, len)?;
        let log_base = (min..=max)
            .map(|k| {
                dhyper(
                    f64::from(k),
                    f64::from(m.cols[0]),
                    f64::from(m.cols[1]),
                    f64::from(m.rows[0]),
                    true,
                    &mut Flags::default(),
                )
            })
            .collect();
        Ok(Self {
            log_base,
            at: (at - min) as usize,
        })
    }

    fn weights(&self, odds: f64) -> Vec<f64> {
        let log_or = odds.ln();
        let last = self.log_base.len() - 1;
        let mut weights = vec![0.0; self.log_base.len()];
        if odds == 0.0 || odds.is_infinite() {
            weights[if odds == 0.0 { 0 } else { last }] = 1.0;
            return weights;
        }
        let max = self
            .log_base
            .iter()
            .enumerate()
            .map(|(i, l)| l + i as f64 * log_or)
            .fold(f64::NEG_INFINITY, f64::max);
        for (i, (p, l)) in weights.iter_mut().zip(&self.log_base).enumerate() {
            *p = (l + i as f64 * log_or - max).exp();
        }
        let sum: f64 = weights.iter().sum();
        for p in &mut weights {
            *p /= sum;
        }
        weights
    }

    fn probability(&self, odds: f64, alt: Alternative) -> f64 {
        let p = self.weights(odds);
        let at = self.at;
        match alt {
            Alternative::Less => p[..=at].iter().sum(),
            Alternative::Greater => p[at..].iter().sum(),
            Alternative::TwoSided => p.iter().filter(|&&v| v <= p[at] * 1.0000001).sum(),
        }
    }

    fn solve(&self, statistic: impl Fn(&[f64]) -> f64, target: f64, increasing: bool) -> f64 {
        let f = |odds| statistic(&self.weights(odds)) - target;
        let center = f(1.0);
        if center == 0.0 {
            return 1.0;
        }
        let inverse = (center < 0.0) == increasing;
        let f = |x: f64| if inverse { f(x.recip()) } else { f(x) };
        let root = zeroin(
            |x| Ok::<_, Infallible>(f(x)),
            0.0,
            1.0,
            f(0.0),
            center,
            f64::EPSILON.sqrt().sqrt(),
            1000,
        );
        match root {
            Ok(out) => {
                if inverse {
                    out.root.max(f64::EPSILON).recip()
                } else {
                    out.root
                }
            }
            Err(never) => match never {},
        }
    }
}

struct Hybrid {
    expect: f64,
    percent: f64,
    emin: f64,
}

impl Hybrid {
    fn log_mode(&self, rows: &[i32], cols: &[i32], total: i32) -> Option<(f64, f64)> {
        if rows.contains(&0) || cols.len() < 2 {
            return None;
        }
        let total = f64::from(total);
        let mut large = 0;
        for &row in rows {
            for &col in cols {
                let expected = f64::from(row) * f64::from(col) / total;
                if expected < self.emin {
                    return None;
                }
                large += usize::from(expected >= self.expect);
            }
        }
        let r = rows.len();
        let c = cols.len();
        if 100.0 * (large as f64) < self.percent * (r * c) as f64 {
            return None;
        }
        let df = ((r - 1) * (c - 1)) as f64;
        let log_sum = |margin: &[i32]| margin.iter().map(|&v| f64::from(v).ln()).sum::<f64>();
        // The normal density on the free cells uses the fixed-margin covariance.
        let log_mode = ((r * c - 1) as f64 * total.ln()
            - (c - 1) as f64 * log_sum(rows)
            - (r - 1) as f64 * log_sum(cols)
            - df * std::f64::consts::TAU.ln())
            / 2.0;
        Some((log_mode, df))
    }
}

struct Enumeration {
    log_factorial: Vec<f64>,
    cols: Vec<i32>,
    threshold: f64,
    budget: usize,
    bounds: HashMap<(usize, Vec<i32>), (f64, f64)>,
    bounds_limit: usize,
    hybrid: Option<Hybrid>,
}

impl Enumeration {
    fn fact(&self, n: i32) -> f64 {
        self.log_factorial[n as usize]
    }

    fn margin_score(&self, margin: &[i32]) -> f64 {
        margin.iter().map(|&n| self.fact(n)).sum()
    }

    fn min_score(&self, margin: &[i32], parts: i32) -> f64 {
        margin
            .iter()
            .map(|&n| {
                let q = n / parts;
                let r = n % parts;
                f64::from(parts - r) * self.fact(q) + f64::from(r) * self.fact(q + i32::from(r > 0))
            })
            .sum()
    }

    fn tail(&mut self, rows: &mut [i32], column: usize, score: f64, mass: f64) -> Option<f64> {
        let cols = &self.cols[column..];
        let row_score = self.margin_score(rows);
        if cols.len() == 1 {
            return Some(if score + row_score >= self.threshold {
                mass
            } else {
                0.0
            });
        }
        let col_score = self.margin_score(cols);
        // Equal parts minimize the sum of log factorials for each margin.
        let minimum = self
            .min_score(rows, cols.len() as i32)
            .max(self.min_score(cols, rows.len() as i32));
        if score + minimum >= self.threshold {
            return Some(mass);
        }
        // Combining cells can only increase the sum of log factorials.
        if score + row_score.min(col_score) < self.threshold {
            return Some(0.0);
        }
        let total = rows.iter().sum::<i32>();
        let approximation = if column == 0 {
            None
        } else {
            self.hybrid
                .as_ref()
                .and_then(|h| h.log_mode(rows, cols, total))
        };
        if approximation.is_some() || cols.len() == 2 {
            let (min, max) = self.score_bounds(rows, column)?;
            if score + min >= self.threshold {
                return Some(mass);
            }
            if score + max < self.threshold {
                return Some(0.0);
            }
        }
        let cols = &self.cols[column..];
        if let Some((log_mode, df)) = approximation {
            let log_normalizer = row_score + col_score - self.fact(total);
            let deviance = 2.0 * (self.threshold - score - log_normalizer + log_mode);
            return Some(mass * pchisq(deviance, df, UPPER, &mut Flags::default()));
        }
        let count = cols[0];
        let log_normalizer =
            row_score + self.fact(count) + self.fact(total - count) - self.fact(total);
        self.column(rows, column, 0, count, score, log_normalizer, mass)
    }

    fn score_bounds(&mut self, rows: &[i32], col: usize) -> Option<(f64, f64)> {
        if col + 1 == self.cols.len() {
            let score = self.margin_score(rows);
            return Some((score, score));
        }
        let mut rows = rows.to_vec();
        rows.sort_unstable();
        let key = (col, rows.clone());
        if let Some(&bounds) = self.bounds.get(&key) {
            return Some(bounds);
        }
        let bounds = self.column_bounds(&mut rows, col, 0, self.cols[col])?;
        if self.bounds.len() < self.bounds_limit {
            self.bounds.insert(key, bounds);
        }
        Some(bounds)
    }

    fn column_bounds(
        &mut self,
        rows: &mut [i32],
        col: usize,
        row: usize,
        left: i32,
    ) -> Option<(f64, f64)> {
        self.budget = self.budget.checked_sub(1)?;
        if row == rows.len() {
            return self.score_bounds(rows, col + 1);
        }
        let count = rows[row];
        let after = rows[row + 1..].iter().sum::<i32>();
        let mut bounds = (f64::INFINITY, f64::NEG_INFINITY);
        for value in (left - after).max(0)..=count.min(left) {
            rows[row] = count - value;
            let (min, max) = self.column_bounds(rows, col, row + 1, left - value)?;
            let cell = self.fact(value);
            bounds.0 = bounds.0.min(min + cell);
            bounds.1 = bounds.1.max(max + cell);
        }
        rows[row] = count;
        Some(bounds)
    }

    #[allow(clippy::too_many_arguments)]
    fn column(
        &mut self,
        rows: &mut [i32],
        col: usize,
        row: usize,
        left: i32,
        score: f64,
        log_mass: f64,
        mass: f64,
    ) -> Option<f64> {
        self.budget = self.budget.checked_sub(1)?;
        if row == rows.len() {
            return self.tail(rows, col + 1, score, mass * log_mass.exp());
        }
        let count = rows[row];
        let after = rows[row + 1..].iter().sum::<i32>();
        let mut probability = 0.0;
        for value in (left - after).max(0)..=count.min(left) {
            rows[row] = count - value;
            let cell = self.fact(value);
            let contribution = self.column(
                rows,
                col,
                row + 1,
                left - value,
                score + cell,
                log_mass - cell - self.fact(count - value),
                mass,
            )?;
            probability += contribution;
        }
        rows[row] = count;
        Some(probability)
    }
}
