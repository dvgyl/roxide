//! Adaptive Gauss–Kronrod quadrature with epsilon extrapolation.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use crate::VecFn;

const X21: [f64; 11] = [
    0.9956571630258081,
    0.9739065285171717,
    0.9301574913557082,
    0.8650633666889845,
    0.7808177265864169,
    0.6794095682990244,
    0.5627571346686047,
    0.4333953941292472,
    0.2943928627014602,
    0.14887433898163122,
    0.0,
];
const K21: [f64; 11] = [
    0.011694638867371874,
    0.032558162307964725,
    0.054755896574351995,
    0.07503967481091995,
    0.0931254545836976,
    0.10938715880229764,
    0.12349197626206585,
    0.13470921731147333,
    0.14277593857706009,
    0.14773910490133849,
    0.1494455540029169,
];
const G10: [f64; 5] = [
    0.06667134430868814,
    0.1494513491505806,
    0.21908636251598204,
    0.26926671930999635,
    0.29552422471475287,
];
const X15: [f64; 8] = [
    0.9914553711208126,
    0.9491079123427585,
    0.8648644233597691,
    0.7415311855993945,
    0.5860872354676911,
    0.4058451513773972,
    0.20778495500789848,
    0.0,
];
const K15: [f64; 8] = [
    0.022935322010529224,
    0.06309209262997856,
    0.10479001032225018,
    0.14065325971552592,
    0.1690047266392679,
    0.19035057806478542,
    0.20443294007529889,
    0.20948214108472782,
];
const G7: [f64; 4] = [
    0.1294849661688697,
    0.27970539148927664,
    0.3818300505051189,
    0.4179591836734694,
];

/// Quadrature status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// The error estimate meets the tolerance.
    Ok,
    /// The subinterval limit was reached.
    Subdivisions,
    /// Floating-point precision limits the result.
    Roundoff,
    /// The integrand is ill behaved near a point.
    BadBehavior,
    /// The integral appears divergent or too slowly convergent.
    Divergent,
    /// The callback returned a non-finite value.
    NonFinite,
}

impl Status {
    /// Message associated with this status.
    pub fn message(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::Subdivisions => "maximum number of subdivisions reached",
            Self::Roundoff => "roundoff error was detected",
            Self::BadBehavior => "extremely bad integrand behaviour",
            Self::Divergent => "the integral is probably divergent",
            Self::NonFinite => "non-finite function value",
        }
    }
}

/// Integral estimate and quadrature diagnostics.
#[derive(Debug, Clone, Copy)]
pub struct QuadOut {
    /// Integral estimate.
    pub value: f64,
    /// Estimated absolute error.
    pub abs_error: f64,
    /// Number of subintervals.
    pub subdivisions: usize,
    /// Reason why the quadrature stopped.
    pub status: Status,
}

#[derive(Clone, Copy)]
enum Domain {
    Finite,
    Infinite { bound: f64, sign: f64, both: bool },
}

#[derive(Clone, Copy)]
struct Panel {
    a: f64,
    b: f64,
    value: f64,
    error: f64,
    variation: f64,
    magnitude: f64,
}

impl Domain {
    fn panel<F: VecFn>(self, f: &mut F, a: f64, b: f64) -> Result<Panel, F::Err> {
        let (nodes, kronrod, gauss): (&[f64], &[f64], &[f64]) = match self {
            Self::Finite => (&X21, &K21, &G10),
            Self::Infinite { .. } => (&X15, &K15, &G7),
        };
        let pairs = nodes.len() - 1;
        let n = 2 * pairs + 1;
        let center = (a + b) * 0.5;
        let half = (b - a) * 0.5;
        let mut x = [0.0; 21];
        let mut y = [0.0; 21];
        x[0] = center;
        for (position, i) in (1..pairs)
            .step_by(2)
            .chain((0..pairs).step_by(2))
            .enumerate()
        {
            let offset = half * nodes[i];
            x[2 * position + 1] = center - offset;
            x[2 * position + 2] = center + offset;
        }
        match self {
            Self::Finite => f.eval(&x[..n], &mut y[..n])?,
            Self::Infinite { bound, sign, both } => {
                let mut points = x.map(|t| bound + sign * ((1.0 - t) / t));
                f.eval(&points[..n], &mut y[..n])?;
                if both {
                    points[..n].iter_mut().for_each(|v| *v = -*v);
                    let mut reflected = [0.0; 21];
                    f.eval(&points[..n], &mut reflected[..n])?;
                    for (value, other) in y[..n].iter_mut().zip(reflected) {
                        *value += other;
                    }
                }
                for (value, t) in y[..n].iter_mut().zip(x) {
                    *value = (*value / t) / t;
                }
            }
        }
        let mut high = kronrod[pairs] * y[0];
        let mut low = gauss.get(pairs / 2).map_or(0.0, |weight| weight * y[0]);
        let mut magnitude = high.abs();
        let ordering = || {
            (1..pairs)
                .step_by(2)
                .chain((0..pairs).step_by(2))
                .enumerate()
        };
        for (pos, i) in ordering() {
            let left = y[2 * pos + 1];
            let right = y[2 * pos + 2];
            let sum = left + right;
            high = kronrod[i].mul_add(sum, high);
            magnitude = kronrod[i].mul_add(left.abs() + right.abs(), magnitude);
            if i % 2 == 1 {
                low = gauss[i / 2].mul_add(sum, low);
            }
        }
        let mean = high * 0.5;
        let mut variation = kronrod[pairs] * (y[0] - mean).abs();
        for (pos, i) in ordering() {
            variation = kronrod[i].mul_add(
                (y[2 * pos + 1] - mean).abs() + (y[2 * pos + 2] - mean).abs(),
                variation,
            );
        }
        variation *= half.abs();
        magnitude *= half.abs();
        let mut error = ((high - low) * half).abs();
        if variation != 0.0 && error != 0.0 {
            error = variation * (200.0 * error / variation).powf(1.5).min(1.0);
        }
        error = error.max(50.0 * f64::EPSILON * magnitude);
        Ok(Panel {
            a,
            b,
            value: high * half,
            error,
            variation,
            magnitude,
        })
    }
}

/// Apply the 21-point Gauss–Kronrod rule to one finite interval.
pub fn qk21<F: VecFn>(f: &mut F, a: f64, b: f64) -> Result<QuadOut, F::Err> {
    let panel = Domain::Finite.panel(f, a, b)?;
    Ok(QuadOut {
        value: panel.value,
        abs_error: panel.error,
        subdivisions: 1,
        status: if panel.value.is_finite() {
            Status::Ok
        } else {
            Status::NonFinite
        },
    })
}

/// Integrate on a finite interval with adaptive subdivision.
pub fn dqags<F: VecFn>(
    f: &mut F,
    a: f64,
    b: f64,
    abs_tol: f64,
    rel_tol: f64,
    limit: usize,
) -> Result<QuadOut, F::Err> {
    adaptive(f, Domain::Finite, a, b, abs_tol, rel_tol, limit)
}

/// Integrate on a semi-infinite or doubly infinite interval.
pub fn dqagi<F: VecFn>(
    f: &mut F,
    a: f64,
    b: f64,
    abs_tol: f64,
    rel_tol: f64,
    limit: usize,
) -> Result<QuadOut, F::Err> {
    let domain = if a.is_finite() {
        Domain::Infinite {
            bound: a,
            sign: 1.0,
            both: false,
        }
    } else if b.is_finite() {
        Domain::Infinite {
            bound: b,
            sign: -1.0,
            both: false,
        }
    } else {
        Domain::Infinite {
            bound: 0.0,
            sign: 1.0,
            both: true,
        }
    };
    adaptive(f, domain, 0.0, 1.0, abs_tol, rel_tol, limit)
}

fn epsilon(sequence: &[f64]) -> Option<f64> {
    let mut current = sequence.to_vec();
    let mut previous = vec![0.0; current.len() + 1];
    let mut estimate = None;
    while current.len() > 1 {
        let next: Vec<_> = current
            .windows(2)
            .enumerate()
            .map(|(i, pair)| previous[i + 1] + (pair[1] - pair[0]).recip())
            .collect();
        if next.len() % 2 == sequence.len() % 2
            && let Some(value) = next.last().filter(|x| x.is_finite())
        {
            estimate = Some(*value);
        }
        previous = current;
        current = next;
    }
    estimate
}

fn adaptive<F: VecFn>(
    f: &mut F,
    domain: Domain,
    a: f64,
    b: f64,
    abs_tol: f64,
    rel_tol: f64,
    limit: usize,
) -> Result<QuadOut, F::Err> {
    let first = domain.panel(f, a, b)?;
    let (mut value, mut error) = (first.value, first.error);
    let tolerance = |value: f64| abs_tol.max(rel_tol * value.abs());
    let mut panels = vec![first];
    let mut sequence = vec![value];
    let mut history = Vec::new();
    let mut shortest = (b - a).abs() * 0.375;
    let mut roundoff = 0;
    let mut best = None;
    let mut direct_converged = false;
    let status = loop {
        if !value.is_finite() || !error.is_finite() {
            break Status::NonFinite;
        }
        if panels.len() >= limit {
            break Status::Subdivisions;
        }
        if error == 0.0
            || (error <= tolerance(value) && (panels.len() > 1 || error != first.variation))
        {
            direct_converged = true;
            break Status::Ok;
        }
        if roundoff >= 10
            || panels.len() == 1
                && error > tolerance(value)
                && error <= 100.0 * f64::EPSILON * first.magnitude
        {
            break Status::Roundoff;
        }
        let largest = |wide_only: bool| {
            panels
                .iter()
                .enumerate()
                .filter(|(_, p)| {
                    !wide_only || (p.b - p.a).abs() > shortest && p.error > tolerance(value)
                })
                .max_by(|(i, a), (j, b)| a.error.total_cmp(&b.error).then_with(|| j.cmp(i)))
                .map(|(i, _)| i)
        };
        let Some(mut i) = largest(false) else {
            break Status::BadBehavior;
        };
        if panels.len() >= 2 && (panels[i].b - panels[i].a).abs() <= shortest {
            if let Some(larger) = largest(true) {
                i = larger;
            } else {
                if sequence.len() == 50 {
                    sequence.remove(0);
                }
                sequence.push(value);
                let infinite = matches!(domain, Domain::Infinite { .. });
                // The dominant tail interval changes as the infinite domain
                // is subdivided. Use the latest three estimates for that tail.
                let start = if infinite {
                    sequence.len().saturating_sub(3)
                } else {
                    (sequence.len() + 1) % 2
                };
                if let Some(extrapolated) = epsilon(&sequence[start..]) {
                    if history.len() >= 3 {
                        let history_error = history
                            .iter()
                            .map(|x| (extrapolated - x).abs())
                            .sum::<f64>();
                        // Higher-order estimates can agree while retaining the same bias.
                        // Compare with the extrapolation of the latest three subdivisions.
                        let local = epsilon(&sequence[sequence.len() - 3..]).unwrap_or(value);
                        let estimate = if infinite {
                            history_error
                        } else {
                            history_error.max((extrapolated - local).abs())
                        }
                        .max(5.0 * f64::EPSILON * extrapolated.abs());
                        if best.is_none_or(|(_, error)| estimate < error) {
                            best = Some((extrapolated, estimate));
                        }
                        if estimate <= tolerance(extrapolated) {
                            break Status::Ok;
                        }
                        history.remove(0);
                    }
                    history.push(extrapolated);
                }
                shortest *= 0.5;
            }
        }
        let panel = panels[i];
        let midpoint = panel.a * 0.5 + panel.b * 0.5;
        if midpoint == panel.a || midpoint == panel.b {
            break Status::BadBehavior;
        }
        let left = domain.panel(f, panel.a, midpoint)?;
        let right = domain.panel(f, midpoint, panel.b)?;
        let area = left.value + right.value;
        let local_error = left.error + right.error;
        if left.error != left.variation
            && right.error != right.variation
            && (panel.value - area).abs() <= 1e-5 * area.abs()
            && local_error >= 0.99 * panel.error
        {
            roundoff += 1;
        }
        value = (value + area) - panel.value;
        error = (error + local_error) - panel.error;
        panels[i] = left;
        panels.push(right);
        if panels.len() == 2 {
            sequence.push(value);
        }
    };
    let mut status = status;
    if let Some((estimate, uncertainty)) = best.filter(|&(v, e)| {
        !direct_converged
            && e < error
            && (v == 0.0 || value == 0.0 || e / v.abs() <= error / value.abs())
    }) {
        let ratio = estimate / value;
        let cancellation = estimate.abs().max(value.abs()) <= 0.01 * first.magnitude;
        if !cancellation && (!(0.01..=100.0).contains(&ratio) || error > value.abs()) {
            status = Status::Divergent;
        }
        (value, error) = (estimate, uncertainty);
    } else {
        value = panels.iter().map(|p| p.value).sum();
    }
    Ok(QuadOut {
        value,
        abs_error: error,
        subdivisions: panels.len(),
        status,
    })
}
