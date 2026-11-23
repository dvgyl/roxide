use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_math::{core::Flags, dist::discrete::dhyper, special::gamma::lgamma};
use roxide_value::ValueApi;

use super::Alternative;
use crate::distn::support::{limit, logical, number, numbers};

pub(super) fn test(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let mut x = numbers(ctx, call, args.get(0))?;
    let mut y = numbers(ctx, call, args.get(1))?;
    let two = logical(ctx, call, args.get(2))? == Some(true);
    let alt = Alternative::read(ctx, call, args.get(3))?;
    let exact = logical(ctx, call, args.get(4))?;
    let b = number(ctx, call, args.get(5))?;
    let simulate = args.get(5) != roxide_value::SEXP_NIL;
    x.retain(|v| !v.is_nan());
    if two {
        y.retain(|v| !v.is_nan());
    }
    if x.is_empty() || (two && y.is_empty()) {
        return Err(ctx.stop(call.err_call(), "not enough data"));
    }
    if x.len().saturating_add(y.len()) > 1_000_000 {
        return Err(ctx.stop(
            call.err_call(),
            "Kolmogorov-Smirnov test size limit exceeded",
        ));
    }
    let n = x.len();
    if !two {
        let ties = x.windows(2).any(|pair| pair[0] == pair[1]);
        if y.is_empty() || y.iter().any(|v| !v.is_finite()) {
            return Err(ctx.stop(
                call.err_call(),
                "invalid one-sample distribution function values",
            ));
        }
        let dist = (0..n.max(y.len())).fold(0.0_f64, |distance, i| {
            let f = y[i % y.len()];
            let rank = (i % n) as f64 / n as f64;
            let less = f - rank;
            let greater = rank + 1.0 / n as f64 - f;
            distance.max(direction(alt, less, greater))
        });
        let exact = exact.unwrap_or(n < 100 && !ties);
        let p = if exact && alt != Alternative::TwoSided {
            if n > 100_000 {
                return Err(ctx.stop(
                    call.err_call(),
                    "exact Kolmogorov-Smirnov test size limit exceeded",
                ));
            }
            one_sided(n, dist)
        } else if exact {
            let size = (2.0 * (n as f64 * dist).floor() + 1.0) as usize;
            if size > 259 && dist < 0.5 {
                return Err(ctx.stop(
                    call.err_call(),
                    "exact Kolmogorov-Smirnov test size limit exceeded",
                ));
            }
            limit::<f64>(ctx, call, size.min(259).pow(2))?;
            one_two_sided(n, dist, size)
        } else {
            asymptotic(n as f64, dist, alt)
        };
        return Ok(ctx
            .session_mut()
            .alloc_real_from(&[dist, p, f64::from(exact)])?);
    }

    x.sort_by(f64::total_cmp);
    y.sort_by(f64::total_cmp);
    let m = y.len();
    limit::<usize>(ctx, call, n + m)?;
    let (groups, threshold) = pooled(&x, &y, alt);
    let ties = groups.iter().any(|&g| g > 1);
    let exact = exact.unwrap_or(n.saturating_mul(m) < 10_000);
    let d = if threshold == 0 && alt == Alternative::Less {
        -0.0
    } else {
        threshold as f64 / (n as f64 * m as f64)
    };
    let (p, mode) = if exact && !simulate {
        if n.saturating_mul(m) > 5_000_000 {
            return Err(ctx.stop(
                call.err_call(),
                "exact Kolmogorov-Smirnov test size limit exceeded",
            ));
        }
        limit::<f64>(ctx, call, n.min(m) + 1)?;
        (permutation_tail(&groups, n, m, alt, threshold), 1.0)
    } else if simulate {
        limit::<i32>(ctx, call, groups.len().saturating_mul(2))?;
        let p = super::tables::simulate_partition(ctx, call, n, m, &groups, b, |cells| {
            let mut count = 0;
            let mut total = 0;
            for (&first, &g) in cells.iter().step_by(2).zip(&groups) {
                count += first as usize;
                total += g;
                if difference(n, m, count, total, alt) >= threshold {
                    return true;
                }
            }
            false
        })?;
        (p, if exact { 3.0 } else { 2.0 })
    } else {
        if ties {
            ctx.warn_now(
                call.err_call(),
                "p-value will be approximate in the presence of ties",
            )?;
        }
        (
            asymptotic(n as f64 * m as f64 / (n + m) as f64, d, alt),
            0.0,
        )
    };
    Ok(ctx.session_mut().alloc_real_from(&[d, p, mode])?)
}

fn direction(alt: Alternative, less: f64, greater: f64) -> f64 {
    match alt {
        Alternative::TwoSided => less.max(greater),
        Alternative::Less => less,
        Alternative::Greater => greater,
    }
}

fn difference(n: usize, m: usize, nx: usize, total: usize, alt: Alternative) -> u64 {
    let delta = nx as i64 * m as i64 - (total - nx) as i64 * n as i64;
    match alt {
        Alternative::TwoSided => delta.unsigned_abs(),
        Alternative::Less => (-delta).max(0) as u64,
        Alternative::Greater => delta.max(0) as u64,
    }
}

fn pooled(x: &[f64], y: &[f64], alt: Alternative) -> (Vec<usize>, u64) {
    let (mut i, mut j, mut threshold) = (0, 0, 0);
    let mut groups = Vec::new();
    while i < x.len() || j < y.len() {
        let a = x.get(i).copied().unwrap_or(f64::INFINITY);
        let b = y.get(j).copied().unwrap_or(f64::INFINITY);
        let cutoff = a.min(b);
        let before = i + j;
        while i < x.len() && x[i] == cutoff {
            i += 1;
        }
        while j < y.len() && y[j] == cutoff {
            j += 1;
        }
        groups.push(i + j - before);
        threshold = threshold.max(difference(x.len(), y.len(), i, i + j, alt));
    }
    (groups, threshold)
}

fn permutation_tail(groups: &[usize], n: usize, m: usize, alt: Alternative, threshold: u64) -> f64 {
    if threshold == 0 {
        return 1.0;
    }
    let (n, m, alt) = if n <= m {
        (n, m, alt)
    } else {
        (
            m,
            n,
            match alt {
                Alternative::TwoSided => alt,
                Alternative::Less => Alternative::Greater,
                Alternative::Greater => Alternative::Less,
            },
        )
    };
    let mut reached = vec![0.0; n + 1];
    let mut next = reached.clone();
    reached[0] = 1.0;
    let mut tail = 0.0;
    let mut used: usize = 0;
    let mut flags = Flags::default();
    for &g in groups {
        next.fill(0.0);
        for (i, &prob) in reached
            .iter()
            .enumerate()
            .take(n.min(used) + 1)
            .skip(used.saturating_sub(m))
        {
            if prob == 0.0 {
                continue;
            }
            let yy = m - (used - i);
            let xx = n - i;
            for j in g.saturating_sub(yy)..=g.min(xx) {
                let weight = if g == 1 {
                    (if j == 0 { yy } else { xx }) as f64 / (xx + yy) as f64
                } else {
                    dhyper(j as f64, xx as f64, yy as f64, g as f64, false, &mut flags)
                };
                if difference(n, m, i + j, used + g, alt) >= threshold {
                    tail += prob * weight;
                } else {
                    next[i + j] += prob * weight;
                }
            }
        }
        used += g;
        std::mem::swap(&mut reached, &mut next);
    }
    tail.min(1.0)
}

fn asymptotic(n: f64, d: f64, alt: Alternative) -> f64 {
    let z = d * n.sqrt();
    if z == 0.0 {
        return 1.0;
    }
    if alt != Alternative::TwoSided {
        return (-2.0 * z * z).exp();
    }
    if z < 1.0 {
        // The oracle uses the first transformed-series term below one.
        let exponent = -(std::f64::consts::PI / z).powi(2) / 8.0;
        let cdf = (2.0 * std::f64::consts::PI).sqrt() * exponent.exp() / z;
        return 1.0 - cdf;
    }
    let mut p = 0.0;
    for k in 1..=16 {
        let term = 2.0 * (-2.0 * (z * f64::from(k)).powi(2)).exp();
        p += if k % 2 == 1 { term } else { -term };
        if term <= 1e-6 {
            break;
        }
    }
    p
}

fn one_sided(size: usize, d: f64) -> f64 {
    if d <= 0.0 {
        return 1.0;
    }
    if d >= 1.0 {
        return 0.0;
    }
    let n = size as f64;
    let mut flags = Flags::default();
    let n_fact = lgamma(n + 1.0, &mut flags);
    let mut sum = (n * (-d).ln_1p()).exp();
    for j in 1..=((1.0 - d) * n).floor() as usize {
        let j = j as f64;
        let a = d + j / n;
        if a >= 1.0 {
            continue;
        }
        let log_choose = n_fact - lgamma(j + 1.0, &mut flags) - lgamma(n - j + 1.0, &mut flags);
        sum += (d.ln() + log_choose + (j - 1.0) * a.ln() + (n - j) * (-a).ln_1p()).exp();
    }
    sum.min(1.0)
}

fn one_two_sided(n: usize, d: f64, m: usize) -> f64 {
    if d >= 0.5 {
        return (2.0 * one_sided(n, d)).min(1.0);
    }
    if d <= 0.5 / n as f64 {
        return 1.0;
    }
    // Durbin's matrix representation, with the boundary terms of Marsaglia et al.
    let k = m.div_ceil(2);
    let h = k as f64 - n as f64 * d;
    let mut factorial = vec![1.0; m + 1];
    for j in 1..=m {
        factorial[j] = factorial[j - 1] / j as f64;
    }
    let mut base = vec![0.0; m * m];
    for i in 0..m {
        for j in 0..=(i + 1).min(m - 1) {
            let power = i + 1 - j;
            let mut v = 1.0;
            if j == 0 {
                v -= h.powi(power as i32);
            }
            if i == m - 1 {
                v -= h.powi(power as i32);
            }
            if i == m - 1 && j == 0 && h > 0.5 {
                v += (2.0 * h - 1.0).powi(m as i32);
            }
            base[i * m + j] = v * factorial[power];
        }
    }
    let mut matrix = vec![0.0; m * m];
    for i in 0..m {
        matrix[i * m + i] = 1.0;
    }
    let mut logs = 0.0;
    let mut base_logs = 0.0;
    let mut power = n;
    loop {
        if power % 2 == 1 {
            let (result, exponent) = multiply(&matrix, &base, m);
            matrix = result;
            logs += base_logs + exponent;
        }
        power /= 2;
        if power == 0 {
            break;
        }
        let (result, exponent) = multiply(&base, &base, m);
        base = result;
        base_logs = 2.0 * base_logs + exponent;
    }
    let mut cdf = matrix[(k - 1) * m + k - 1];
    if logs == 0.0 {
        for i in 1..=n {
            cdf *= i as f64 / n as f64;
        }
        return (1.0 - cdf).clamp(0.0, 1.0);
    }
    let log_factor = (1..=n).map(|i| (i as f64 / n as f64).ln()).sum::<f64>();
    (-(cdf.ln() + logs + log_factor).exp_m1()).clamp(0.0, 1.0)
}

fn multiply(a: &[f64], b: &[f64], m: usize) -> (Vec<f64>, f64) {
    let mut output = vec![0.0; a.len()];
    for i in 0..m {
        let row = &mut output[i * m..(i + 1) * m];
        for j in 0..m {
            let v = a[i * m + j];
            if v != 0.0 {
                for (x, &y) in row.iter_mut().zip(&b[j * m..(j + 1) * m]) {
                    *x += v * y;
                }
            }
        }
    }
    let max = output.iter().copied().fold(0.0, f64::max);
    if max > 1e100 || (0.0 < max && max < 1e-100) {
        for value in &mut output {
            *value /= max;
        }
        (output, max.ln())
    } else {
        (output, 0.0)
    }
}
