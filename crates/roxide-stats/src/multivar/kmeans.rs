use std::collections::HashSet;

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_value::SEXP_NIL;

use crate::distn::support::{limit, number, set_attr};

use super::support::{Matrix, area, choice, dimnames, ints, list, matrix, real, strings};

fn distinct_rows(x: &Matrix) -> Vec<i32> {
    let mut seen = HashSet::new();
    let mut indices = Vec::new();
    for i in 0..x.rows {
        let key: Vec<_> = (0..x.cols)
            .map(|j| {
                let v = x.at(i, j);
                if v == 0.0 { 0 } else { v.to_bits() }
            })
            .collect();
        if seen.insert(key) {
            indices.push(i as i32 + 1);
        }
    }
    indices
}

pub(super) fn unique_rows(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = Matrix::read(ctx, call, args.get(0))?;
    limit::<Vec<u64>>(ctx, call, x.rows)?;
    ints(ctx, &distinct_rows(&x))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Algorithm {
    HartiganWong,
    Lloyd,
    MacQueen,
}

struct Fit<'a> {
    x: &'a Matrix,
    centers: Matrix,
    cluster: Vec<usize>,
    sizes: Vec<usize>,
}

impl Fit<'_> {
    fn distance(&self, row: usize, cluster: usize) -> f64 {
        (0..self.x.cols)
            .map(|col| (self.x.at(row, col) - self.centers.at(cluster, col)).powi(2))
            .sum()
    }

    fn nearest(&self, row: usize, except: Option<usize>) -> usize {
        let mut best = (None, f64::INFINITY);
        for k in 0..self.centers.rows {
            if except != Some(k) {
                let d = self.distance(row, k);
                if !d.is_nan() && (best.0.is_none() || d < best.1) {
                    best = (Some(k), d);
                }
            }
        }
        best.0.unwrap_or(0)
    }

    fn update_means(&mut self) {
        self.centers.values.fill(0.0);
        self.sizes.fill(0);
        let k = self.centers.rows;
        for (i, &group) in self.cluster.iter().enumerate() {
            self.sizes[group] += 1;
            for j in 0..self.x.cols {
                self.centers.values[j * k + group] += self.x.at(i, j);
            }
        }
        for col in self.centers.values.chunks_exact_mut(k) {
            for (value, &size) in col.iter_mut().zip(&self.sizes) {
                *value /= size as f64;
            }
        }
    }

    fn transfer(&mut self, i: usize, to: usize) {
        let from = self.cluster[i];
        let old_from = self.sizes[from] as f64;
        let old_to = self.sizes[to] as f64;
        self.sizes[from] -= 1;
        self.sizes[to] += 1;
        self.cluster[i] = to;
        let k = self.centers.rows;
        for j in 0..self.x.cols {
            let x = self.x.at(i, j);
            let a = &mut self.centers.values[j * k + from];
            *a = (*a * old_from - x) / (old_from - 1.0);
            let b = &mut self.centers.values[j * k + to];
            *b = (*b * old_to + x) / (old_to + 1.0);
        }
    }

    fn lloyd(&mut self, limit: usize) -> usize {
        self.cluster.fill(usize::MAX);
        for iter in 1..=limit {
            let mut changed = false;
            for i in 0..self.x.rows {
                let next = self.nearest(i, None);
                changed |= self.cluster[i] != next;
                self.cluster[i] = next;
            }
            if !changed {
                return iter;
            }
            self.update_means();
        }
        limit + 1
    }

    fn macqueen(&mut self, limit: usize) -> usize {
        self.update_means();
        for iter in 1..=limit {
            let mut changed = false;
            for i in 0..self.x.rows {
                let next = self.nearest(i, None);
                if self.cluster[i] != next {
                    self.transfer(i, next);
                    changed = true;
                }
            }
            if !changed {
                return iter;
            }
        }
        limit + 1
    }

    fn removal_gain(&self, i: usize, group: usize) -> f64 {
        let size = self.sizes[group] as f64;
        self.distance(i, group) * size / (size - 1.0)
    }

    fn addition_cost(&self, i: usize, group: usize) -> f64 {
        let size = self.sizes[group] as f64;
        self.distance(i, group) * size / (size + 1.0)
    }

    fn hartigan_wong(
        &mut self,
        limit: usize,
        other: &mut [usize],
        mut trace: impl FnMut(usize, usize),
    ) -> (usize, i32) {
        let n = self.x.rows;
        let k = self.centers.rows;
        let quick_limit = n.saturating_mul(50);
        let mut unchanged = 0;
        for iter in 1..=limit {
            // The optimal-transfer pass checks every prospective destination.
            for (i, alternative) in other.iter_mut().enumerate() {
                let from = self.cluster[i];
                unchanged += 1;
                if self.sizes[from] > 1 {
                    let mut to = *alternative;
                    let mut cost = self.addition_cost(i, to);
                    for group in 0..k {
                        if group != from {
                            let candidate = self.addition_cost(i, group);
                            if candidate < cost {
                                to = group;
                                cost = candidate;
                            }
                        }
                    }
                    if cost < self.removal_gain(i, from) {
                        self.transfer(i, to);
                        *alternative = from;
                        unchanged = 0;
                    } else {
                        *alternative = to;
                    }
                }
                if unchanged == n {
                    trace(iter, unchanged);
                    return (iter, 0);
                }
            }
            trace(iter, unchanged);

            // The quick-transfer pass revisits each point's alternate cluster.
            // Stop when one pass makes no transfers or the step limit is reached.
            let mut still = 0;
            for step in 0..quick_limit {
                let i = step % n;
                let from = self.cluster[i];
                let to = other[i];
                still += 1;
                if self.sizes[from] > 1 && self.addition_cost(i, to) < self.removal_gain(i, from) {
                    self.transfer(i, to);
                    other[i] = from;
                    still = 0;
                    unchanged = 0;
                }
                if still == n {
                    break;
                }
                if step + 1 == quick_limit {
                    return (iter, 4);
                }
            }
            if k == 2 {
                return (iter, 0);
            }
        }
        (limit + 1, 2)
    }
}

pub(super) fn fit(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = Matrix::read(ctx, call, args.get(0))?;
    let centers = Matrix::read(ctx, call, args.get(1))?;
    let k = centers.rows;
    let n = x.rows;
    let iter_limit = number(ctx, call, args.get(2))?;
    if !iter_limit.is_finite() || iter_limit < 1.0 {
        return Err(ctx.stop(call.err_call(), "'iter.max' must be positive"));
    }
    let iter_limit = iter_limit.min(1_000_000.0) as usize;
    let mut method = match choice(ctx, args.get(3)) {
        Some("Hartigan-Wong") => Algorithm::HartiganWong,
        Some("Lloyd" | "Forgy") => Algorithm::Lloyd,
        Some("MacQueen") => Algorithm::MacQueen,
        _ => return Err(ctx.stop(call.err_call(), "invalid k-means algorithm")),
    };
    if k == 1 {
        method = Algorithm::MacQueen;
    }
    if centers.cols != x.cols {
        return Err(ctx.stop(
            call.err_call(),
            "must have same number of columns in 'x' and 'centers'",
        ));
    }
    if k == 0 || k > n || (method == Algorithm::HartiganWong && k == n) {
        return Err(ctx.stop(
            call.err_call(),
            "number of cluster centres must lie between 1 and nrow(x)",
        ));
    }
    for (matrix, argument) in [(&x, 1), (&centers, 2)] {
        if matrix.values.iter().any(|x| !x.is_finite()) {
            return Err(ctx.stop(
                call.err_call(),
                format!("NA/NaN/Inf in foreign function call (arg {argument})"),
            ));
        }
    }
    limit::<Vec<u64>>(ctx, call, k)?;
    if distinct_rows(&centers).len() != k {
        return Err(ctx.stop(call.err_call(), "initial centers are not distinct"));
    }
    limit::<usize>(ctx, call, n)?;
    let mut fit = Fit {
        x: &x,
        centers,
        cluster: vec![0; n],
        sizes: vec![0; k],
    };
    for i in 0..n {
        fit.cluster[i] = fit.nearest(i, None);
    }
    let (iter, fault) = match method {
        Algorithm::Lloyd => {
            let iter = fit.lloyd(iter_limit);
            let fault = (iter > iter_limit).then_some(2);
            (iter, fault)
        }
        Algorithm::MacQueen => (fit.macqueen(iter_limit), None),
        Algorithm::HartiganWong => {
            let mut other = (0..n)
                .map(|i| fit.nearest(i, Some(fit.cluster[i])))
                .collect::<Vec<_>>();
            fit.update_means();
            if fit.sizes.contains(&0) {
                return Err(ctx.stop(
                    call.err_call(),
                    "empty cluster: try a better set of initial centers",
                ));
            }
            let trace = number(ctx, call, args.get(4))?;
            let (iter, fault) = fit.hartigan_wong(iter_limit, &mut other, |iter, unchanged| {
                if trace >= 1.0 {
                    let _ = ctx.host().write_out(
                        format!("KMNS(*, k={k}): iter={iter:3}, indx={unchanged}\n").as_bytes(),
                    );
                }
            });
            fit.update_means();
            (iter, Some(fault))
        }
    };
    if fit.sizes.contains(&0) {
        ctx.warn_now(
            call.err_call(),
            "empty cluster: try a better set of initial centers",
        )?;
    }
    if fault == Some(4) {
        ctx.warn_now(
            call.err_call(),
            format!(
                "Quick-TRANSfer stage steps exceeded maximum (= {})",
                n.saturating_mul(50)
            ),
        )?;
    }
    if iter > iter_limit {
        let suffix = if iter_limit == 1 { "" } else { "s" };
        ctx.warn_now(
            call.err_call(),
            format!("did not converge in {iter_limit} iteration{suffix}"),
        )?;
    }

    let mut within = vec![0.0; k];
    for (i, &group) in fit.cluster.iter().enumerate() {
        within[group] += fit.distance(i, group);
    }
    let total_within = within.iter().sum::<f64>();
    let total = x
        .values
        .chunks(n)
        .map(|col| {
            let mean = col.iter().sum::<f64>() / n as f64;
            col.iter().map(|v| (v - mean).powi(2)).sum::<f64>()
        })
        .sum::<f64>();

    let (rows, cols) = dimnames(ctx, args.get(0))?;
    let labels = strings(ctx, (1..=k).map(|i| i.to_string()))?;
    let cluster = ints(
        ctx,
        &fit.cluster
            .iter()
            .map(|&i| i as i32 + 1)
            .collect::<Vec<_>>(),
    )?;
    set_attr(ctx, cluster, "names", rows)?;
    area::<f64>(ctx, call, k, x.cols)?;
    let centers = real(ctx, &fit.centers.values)?;
    matrix(ctx, call, centers, (k, x.cols), (labels, cols))?;
    let totss = real(ctx, &[total])?;
    let withinss = real(ctx, &within)?;
    let total_within = real(ctx, &[total_within])?;
    let between = real(ctx, &[total - within.iter().sum::<f64>()])?;
    let sizes = ints(
        ctx,
        &fit.sizes.iter().map(|&x| x as i32).collect::<Vec<_>>(),
    )?;
    let iter = ints(ctx, &[iter as i32])?;
    let fault = if let Some(fault) = fault {
        ints(ctx, &[fault])?
    } else {
        SEXP_NIL
    };
    list(
        ctx,
        &[
            ("cluster", cluster),
            ("centers", centers),
            ("totss", totss),
            ("withinss", withinss),
            ("tot.withinss", total_within),
            ("betweenss", between),
            ("size", sizes),
            ("iter", iter),
            ("ifault", fault),
        ],
        Some("kmeans"),
    )
}
