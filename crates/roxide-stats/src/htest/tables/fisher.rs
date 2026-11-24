use std::{collections::HashMap, convert::Infallible};

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_math::{
    core::Flags,
    dist::{continuous::pchisq, discrete::dhyper},
    roots::zeroin,
    special::lgamma,
};
use roxide_value::{SEXP_NIL, ValueApi};

use super::{Margins, margins, matrix_shape, simulate_tables};
use crate::{
    distn::support::{limit, number, numbers},
    htest::{Alternative, UPPER},
};

pub(in crate::htest) fn fisher(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
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
    let mut margin = margins(&x, rows, cols);
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
        margin.remove_zeros();
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
        margin.remove_zeros();
        let Margins { mut rows, mut cols } = margin;
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
