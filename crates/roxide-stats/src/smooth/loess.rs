use super::{list, median, reals};
use crate::distn::support::{limit, numbers};
use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_math::{MatRef, linalg::spectral::svd};
use roxide_value::{NA_REAL, SEXP_NIL, ValueApi};
use std::collections::BTreeMap;

type Point = [f64; 4];

struct Model {
    x: Vec<Point>,
    y: Vec<f64>,
    weights: Vec<f64>,
    d: usize,
    degree: usize,
    span: f64,
    parametric: Vec<bool>,
    terms: Vec<(usize, usize)>,
    stats: usize,
    iterations: usize,
    interpolate: bool,
    normalize: bool,
    cell: f64,
}

impl Model {
    fn read(ctx: &mut Ctx, call: &Call, args: &Args) -> EvalResult<Self> {
        let x = numbers(ctx, call, args.get(0))?;
        let y = numbers(ctx, call, args.get(1))?;
        let weights = numbers(ctx, call, args.get(2))?;
        let options = numbers(ctx, call, args.get(3))?;
        let d = options.first().copied().unwrap_or(0.0) as usize;
        if !(1..=4).contains(&d)
            || y.is_empty()
            || x.len() != y.len() * d
            || weights.len() != y.len()
            || options.len() != 8 + 2 * d
            || !x.iter().chain(&y).all(|x| x.is_finite())
            || !weights.iter().all(|w| w.is_finite() && *w >= 0.0)
            || options[1] < 0.0
            || options[1] > 2.0
            || options[2] <= 0.0
            || !options[2].is_finite()
            || !(1.0..=1000.0).contains(&options[4])
            || !options[7].is_finite()
            || options[7] < 0.0
        {
            return Err(ctx.stop(call.err_call(), "invalid loess inputs"));
        }
        limit::<Point>(ctx, call, y.len())?;
        let degree = options[1] as usize;
        let parametric: Vec<_> = options[8..8 + d].iter().map(|x| *x != 0.0).collect();
        let drop_square: Vec<_> = options[8 + d..].iter().map(|x| *x != 0.0).collect();
        let terms = if degree < 2 {
            Vec::new()
        } else {
            (0..d)
                .flat_map(|i| (i..d).map(move |j| (i, j)))
                .filter(|&(i, j)| i != j || !drop_square[i])
                .collect()
        };
        let x = (0..y.len())
            .map(|i| {
                let mut row = [0.0; 4];
                for j in 0..d {
                    row[j] = x[j * y.len() + i];
                }
                row
            })
            .collect();
        Ok(Self {
            x,
            y,
            weights,
            d,
            degree,
            span: options[2],
            stats: options[3] as usize,
            iterations: options[4] as usize,
            interpolate: options[5] != 0.0,
            normalize: options[6] != 0.0,
            cell: options[7],
            parametric,
            terms,
        })
    }

    fn divisor(&mut self) -> Vec<f64> {
        let mut divisor = vec![1.0; self.d];
        if self.normalize && self.d > 1 {
            let trim = (0.1 * self.x.len() as f64).ceil() as usize;
            for (j, scale) in divisor.iter_mut().enumerate() {
                let mut values: Vec<_> = self.x.iter().map(|x| x[j]).collect();
                values.sort_unstable_by(f64::total_cmp);
                if values.len() <= 2 * trim + 1 {
                    continue;
                }
                let core = &values[trim..values.len() - trim];
                let mean = core.iter().sum::<f64>() / core.len() as f64;
                let sd = (core.iter().map(|x| (x - mean).powi(2)).sum::<f64>()
                    / (core.len() - 1) as f64)
                    .sqrt();
                if sd > 0.0 {
                    *scale = sd;
                    for x in &mut self.x {
                        x[j] /= sd;
                    }
                }
            }
        }
        divisor
    }

    fn local(&self, q: Point, robust: &[f64], result: &mut [f64], component: &[f64]) -> bool {
        let n = self.x.len();
        let m = ((n as f64 * self.span + 1e-5) as usize).clamp(1, n);
        let mut nearby: Vec<_> = self
            .x
            .iter()
            .enumerate()
            .map(|(i, x)| {
                let distance = (0..self.d)
                    .filter(|&j| !self.parametric[j])
                    .map(|j| (x[j] - q[j]).powi(2))
                    .sum::<f64>();
                (distance, i)
            })
            .collect();
        nearby.select_nth_unstable_by(m - 1, |a, b| a.0.total_cmp(&b.0));
        let radius = nearby[m - 1].0 * self.span.max(1.0).powf(2.0 / self.d as f64);
        let k = 1 + if self.degree == 0 {
            0
        } else {
            self.d + self.terms.len()
        };
        let mut design = vec![0.0; m * k];
        let mut w = vec![0.0; m];
        for (i, &(distance, row)) in nearby[..m].iter().enumerate() {
            let dist = if radius == 0.0 {
                0.0
            } else {
                (distance / radius).sqrt()
            };
            w[i] = ((1.0 - dist.powi(3)).max(0.0).powi(3) * self.weights[row] * robust[row]).sqrt();
            design[i] = w[i];
            if self.degree > 0 {
                let x = self.x[row];
                for j in 0..self.d {
                    design[(1 + j) * m + i] = w[i] * (x[j] - q[j]);
                }
                for (index, &(j, l)) in self.terms.iter().enumerate() {
                    design[(1 + self.d + index) * m + i] = w[i] * (x[j] - q[j]) * (x[l] - q[l]);
                }
            }
        }
        let mut scale = vec![1.0; k];
        for (column, norm) in design.chunks_exact_mut(m).zip(&mut scale) {
            let sum = column.iter().fold(0.0_f64, |norm, x| norm.hypot(*x));
            if sum > 0.0 {
                *norm = sum;
                for x in column {
                    *x /= sum;
                }
            }
        }
        let rank = m.min(k);
        let Ok(svd) = MatRef::new(&design, m, k).and_then(|a| svd(a, rank, rank)) else {
            return false;
        };
        let tol = svd.d[0] * 100.0 * f64::EPSILON;
        for l in (0..rank).filter(|&l| svd.d[l] > tol) {
            let projected = (0..k.min(component.len()))
                .map(|j| component[j] * svd.v[l * k + j] / scale[j])
                .sum::<f64>()
                / svd.d[l];
            for (i, &(_, row)) in nearby[..m].iter().enumerate() {
                result[row] += projected * svd.u[l * m + i] * w[i];
            }
        }
        true
    }
}

#[derive(Clone)]
struct Cell {
    lo: Point,
    hi: Point,
    split: Option<(usize, f64, usize)>,
}

struct Tree {
    cells: Vec<Cell>,
    vertices: Vec<Point>,
}

fn corners(lo: Point, hi: Point, d: usize) -> impl Iterator<Item = Point> {
    (0..1 << d).map(move |i| {
        let mut q = lo;
        for j in 0..d {
            if i & (1 << j) != 0 {
                q[j] = hi[j];
            }
        }
        q
    })
}

impl Tree {
    fn new(m: &Model) -> Self {
        let mut lo = [0.0; 4];
        let mut hi = [0.0; 4];
        for j in 0..m.d {
            lo[j] = m.x.iter().map(|x| x[j]).fold(f64::INFINITY, f64::min);
            hi[j] = m.x.iter().map(|x| x[j]).fold(f64::NEG_INFINITY, f64::max);
            let pad = 0.005 * (hi[j] - lo[j]).max(1e-10 * lo[j].abs().max(hi[j].abs()) + 1e-30);
            lo[j] -= pad;
            hi[j] += pad;
        }
        let root = Cell {
            lo,
            hi,
            split: None,
        };
        let mut tree = Self {
            cells: vec![root],
            vertices: corners(lo, hi, m.d).collect(),
        };
        let mut seen: BTreeMap<_, _> = tree
            .vertices
            .iter()
            .map(|v| (v.map(f64::to_bits), ()))
            .collect();
        let mut groups = vec![(0, (0..m.x.len()).collect::<Vec<_>>())];
        let threshold = (m.x.len() as f64 * m.span * m.cell).floor() as usize;
        let mut cursor = 0;
        while cursor < groups.len() {
            let (index, mut rows) = std::mem::take(&mut groups[cursor]);
            cursor += 1;
            if rows.len() <= threshold.max(1) {
                continue;
            }
            let spread = |j| {
                let low = rows
                    .iter()
                    .map(|&i| m.x[i][j])
                    .fold(f64::INFINITY, f64::min);
                let high = rows
                    .iter()
                    .map(|&i| m.x[i][j])
                    .fold(f64::NEG_INFINITY, f64::max);
                high - low
            };
            let mut axis = 0;
            let mut widest = 0.0;
            for j in 0..m.d {
                let width = if m.parametric[j] { 0.0 } else { spread(j) };
                if width > widest {
                    axis = j;
                    widest = width;
                }
            }
            if widest == 0.0 {
                continue;
            }
            rows.sort_unstable_by(|&a, &b| m.x[a][axis].total_cmp(&m.x[b][axis]));
            let middle = rows.len().div_ceil(2);
            let Some(cut) = (1..rows.len())
                .filter(|&i| m.x[rows[i - 1]][axis] < m.x[rows[i]][axis])
                .min_by_key(|&i| i.abs_diff(middle))
            else {
                continue;
            };
            let split = m.x[rows[cut - 1]][axis];
            let node = &tree.cells[index];
            if split <= node.lo[axis] || split >= node.hi[axis] {
                continue;
            }
            let mut left = Cell {
                lo: node.lo,
                hi: node.hi,
                split: None,
            };
            let mut right = left.clone();
            left.hi[axis] = split;
            right.lo[axis] = split;
            let child = tree.cells.len();
            tree.cells[index].split = Some((axis, split, child));
            for v in corners(left.lo, left.hi, m.d).filter(|v| v[axis] == split) {
                if seen.insert(v.map(f64::to_bits), ()).is_none() {
                    tree.vertices.push(v);
                }
            }
            tree.cells.extend([left, right]);
            let rest = rows.split_off(cut);
            groups.extend([(child, rows), (child + 1, rest)]);
        }
        tree
    }
}

fn hermite(s: f64, width: f64) -> (f64, f64) {
    (s * s * (3.0 - 2.0 * s), width * s * s * (1.0 - s))
}

fn blend_edges(
    m: &Model,
    tree: &Tree,
    cell: &Cell,
    q: Point,
    robust: &[f64],
    result: &mut [f64],
) -> bool {
    // Blend the four edges as in Cleveland and Grosse (1991), section 3.3.
    // Use the Hermite value basis to blend the normal derivatives.
    for normal in 0..2 {
        let tangent = 1 - normal;
        let width = cell.hi[normal] - cell.lo[normal];
        let t = (q[normal] - cell.lo[normal]) / width;
        for (edge, fraction, signed_width) in [
            (cell.lo[normal], 1.0 - t, width),
            (cell.hi[normal], t, -width),
        ] {
            let (normal_value, normal_deriv) = hermite(fraction, signed_width);
            if normal_value == 0.0 && normal_deriv == 0.0 {
                continue;
            }
            let mut lower = cell.lo[tangent];
            let mut upper = cell.hi[tangent];
            for vertex in &tree.vertices {
                if vertex[normal] != edge {
                    continue;
                }
                let v = vertex[tangent];
                if v <= q[tangent] {
                    lower = lower.max(v);
                } else {
                    upper = upper.min(v);
                }
            }
            let width = upper - lower;
            let t = if width > 0.0 {
                (q[tangent] - lower) / width
            } else {
                0.0
            };
            for (at, fraction, signed_width) in [(lower, 1.0 - t, width), (upper, t, -width)] {
                let (value, derivative) = hermite(fraction, signed_width);
                let mut vertex = [0.0; 4];
                vertex[normal] = edge;
                vertex[tangent] = at;
                let mut component = [0.0; 3];
                component[0] = normal_value * value;
                component[1 + normal] = normal_deriv * value;
                component[1 + tangent] = normal_value * derivative;
                if component.iter().any(|&v| v != 0.0)
                    && !m.local(vertex, robust, result, &component)
                {
                    return false;
                }
            }
        }
    }
    true
}

fn row(m: &Model, tree: Option<&Tree>, q: Point, robust: &[f64], weights: &mut [f64]) -> bool {
    weights.fill(0.0);
    if q[..m.d].iter().any(|x| !x.is_finite()) {
        return false;
    }
    let Some(tree) = tree else {
        return m.local(q, robust, weights, &[1.0]);
    };
    let root = &tree.cells[0];
    if (0..m.d).any(|j| q[j] < root.lo[j] || q[j] > root.hi[j]) {
        return false;
    }
    let mut cell = root;
    while let Some((axis, split, child)) = cell.split {
        cell = &tree.cells[child + usize::from(q[axis] > split)];
    }
    let mut t = [0.0; 4];
    for j in 0..m.d {
        t[j] = (q[j] - cell.lo[j]) / (cell.hi[j] - cell.lo[j]);
    }
    for (i, corner) in corners(cell.lo, cell.hi, m.d).enumerate() {
        let mut value = [1.0; 4];
        let mut derivative = [0.0; 4];
        for j in 0..m.d {
            let upper = i & (1 << j) != 0;
            let s = if upper { t[j] } else { 1.0 - t[j] };
            let width = cell.hi[j] - cell.lo[j];
            (value[j], derivative[j]) = hermite(s, if upper { -width } else { width });
        }
        let mut component = [0.0; 5];
        component[0] = value.iter().product();
        for j in 0..m.d {
            component[j + 1] = derivative[j]
                * (0..m.d)
                    .filter(|&k| k != j)
                    .map(|k| value[k])
                    .product::<f64>();
        }
        if m.d == 2 {
            component.iter_mut().for_each(|value| *value = -*value);
        }
        if component.iter().any(|&v| v != 0.0) && !m.local(corner, robust, weights, &component) {
            return false;
        }
    }
    m.d != 2 || blend_edges(m, tree, cell, q, robust, weights)
}

fn dot(x: &[f64], y: &[f64]) -> f64 {
    x.iter().zip(y).map(|(x, y)| x * y).sum()
}

fn kd(ctx: &mut Ctx, m: &Model, tree: Option<&Tree>, robust: &[f64]) -> EvalResult {
    let Some(tree) = tree else {
        return Ok(SEXP_NIL);
    };
    let vertices = 1 << m.d;
    let lv = 200.max(m.x.len());
    let mut buffer = vec![0.0; m.x.len()];
    let mut vval = Vec::with_capacity(tree.vertices.len() * (1 + m.d));
    for &q in &tree.vertices {
        for i in 0..=m.d {
            let mut component = [0.0; 5];
            component[i] = 1.0;
            buffer.fill(0.0);
            m.local(q, robust, &mut buffer, &component);
            vval.push(dot(&buffer, &m.y));
        }
    }
    let parameter = [
        m.d,
        m.x.len(),
        vertices,
        tree.cells.len(),
        tree.vertices.len(),
        (vertices + 3) * lv + 49,
        (2 * m.d + 2) * lv + 49,
    ]
    .map(|x| x as i32);
    let a: Vec<_> = tree
        .cells
        .iter()
        .map(|c| c.split.map_or(0, |x| x.0 as i32 + 1))
        .collect();
    let xi: Vec<_> = tree
        .cells
        .iter()
        .map(|c| c.split.map_or(0.0, |x| x.1))
        .collect();
    let root = &tree.cells[0];
    let vert: Vec<_> = root.lo[..m.d]
        .iter()
        .chain(&root.hi[..m.d])
        .copied()
        .collect();
    let parameter = ctx.session_mut().alloc_int_from(&parameter)?;
    ctx.protect(parameter);
    let a = ctx.session_mut().alloc_int_from(&a)?;
    ctx.protect(a);
    let xi = reals(ctx, &xi)?;
    let vert = reals(ctx, &vert)?;
    let vval = reals(ctx, &vval)?;
    list(ctx, &[parameter, a, xi, vert, vval])
}

fn residual_scale(residuals: &[f64], weights: &[f64]) -> f64 {
    let mut absolute: Vec<_> = residuals
        .iter()
        .zip(weights)
        .map(|(e, w)| e.abs() * w.sqrt())
        .collect();
    6.0 * median(&mut absolute)
}

fn weighted_squares(x: &[f64], weights: &[f64]) -> f64 {
    x.iter().zip(weights).map(|(v, w)| w * v * v).sum()
}

fn check_space(ctx: &mut Ctx, call: &Call, m: &Model) -> EvalResult<()> {
    limit::<f64>(ctx, call, m.x.len().saturating_mul(1 + m.d + m.terms.len()))?;
    if m.interpolate {
        limit::<f64>(ctx, call, m.x.len().saturating_mul((m.d + 1) * (1 << m.d)))?;
    }
    Ok(())
}

pub(super) fn fit(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let mut m = Model::read(ctx, call, args)?;
    check_space(ctx, call, &m)?;
    let divisor = m.divisor();
    let tree = m.interpolate.then(|| Tree::new(&m));
    let n = m.x.len();
    let mut fitted = vec![0.0; n];
    let mut residuals = vec![0.0; n];
    let mut robust = vec![1.0; n];
    let mut buffer = vec![0.0; n];
    let mut trace = 0.0;
    let mut delta1 = 0.0;
    let mut delta2 = 0.0;
    let mut matrix = Vec::new();
    if m.stats == 2 && n > 512 {
        return Err(ctx.stop(
            call.err_call(),
            "exact loess statistics exceed the computation limit",
        ));
    }
    if m.stats != 0 && n <= 512 {
        limit::<f64>(ctx, call, n * n)?;
        matrix.reserve_exact(n * n);
    }
    for iter in 0..m.iterations {
        for i in 0..n {
            if !row(&m, tree.as_ref(), m.x[i], &robust, &mut buffer) {
                return Err(ctx.stop(call.err_call(), "cannot fit local polynomial"));
            }
            fitted[i] = dot(&buffer, &m.y);
            residuals[i] = m.y[i] - fitted[i];
            if iter == 0 && m.stats != 0 {
                trace += buffer[i];
                buffer[i] -= 1.0;
                delta1 += dot(&buffer, &buffer);
                if n <= 512 {
                    matrix.extend_from_slice(&buffer);
                }
            }
        }
        if iter + 1 < m.iterations {
            let scale = residual_scale(&residuals, &m.weights);
            for ((w, r), a) in robust.iter_mut().zip(&residuals).zip(&m.weights) {
                *w = if scale <= f64::MIN_POSITIVE {
                    1.0
                } else {
                    (1.0 - a * (r / scale).powi(2)).max(0.0).powi(2)
                };
            }
        }
    }
    if !matrix.is_empty() {
        for i in 0..n {
            let a = &matrix[i * n..(i + 1) * n];
            for j in 0..=i {
                let b = &matrix[j * n..(j + 1) * n];
                delta2 += dot(a, b).powi(2) * if i == j { 1.0 } else { 2.0 };
            }
        }
    } else if m.stats != 0 {
        delta2 = delta1;
    }
    let enp = delta1 + 2.0 * trace - n as f64;
    let ss = if m.iterations > 1 {
        let scale = residual_scale(&residuals, &m.weights);
        let derivative = residuals
            .iter()
            .zip(&m.weights)
            .zip(&robust)
            .map(|((e, w), r)| r.sqrt() * (1.0 - 5.0 * w * (e / scale).powi(2)))
            .sum::<f64>()
            / n as f64;
        if scale > f64::MIN_POSITIVE && derivative.is_finite() && derivative != 0.0 {
            let pseudo: Vec<_> = fitted
                .iter()
                .zip(&residuals)
                .zip(&robust)
                .map(|((y, e), r)| y + r * e / derivative)
                .collect();
            let prior = vec![1.0; n];
            pseudo
                .iter()
                .enumerate()
                .map(|(i, y)| {
                    row(&m, tree.as_ref(), m.x[i], &prior, &mut buffer);
                    m.weights[i] * (y - dot(&buffer, &pseudo)).powi(2)
                })
                .sum()
        } else {
            weighted_squares(&residuals, &m.weights)
        }
    } else {
        weighted_squares(&residuals, &m.weights)
    };
    let s = (ss / delta1).sqrt();
    let fit = reals(ctx, &fitted)?;
    let residuals = reals(ctx, &residuals)?;
    let divisor = reals(ctx, &divisor)?;
    let robust_id = reals(ctx, &robust)?;
    let stats = reals(ctx, &[enp, s, delta1, delta2, trace])?;
    let kd = kd(ctx, &m, tree.as_ref(), &robust)?;
    list(ctx, &[fit, residuals, divisor, robust_id, stats, kd])
}

pub(super) fn predict(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let mut m = Model::read(ctx, call, args)?;
    check_space(ctx, call, &m)?;
    let query = numbers(ctx, call, args.get(4))?;
    let options = numbers(ctx, call, args.get(5))?;
    if options.len() != m.x.len() + 2 || query.len() % m.d != 0 {
        return Err(ctx.stop(call.err_call(), "invalid prediction data"));
    }
    let scale = options[0];
    let se = options[1] != 0.0;
    let robust = &options[2..];
    let divisor = m.divisor();
    let tree = m.interpolate.then(|| Tree::new(&m));
    let mut buffer = vec![0.0; m.x.len()];
    let prior = vec![1.0; robust.len()];
    let mut lower = [f64::INFINITY; 4];
    let mut upper = [f64::NEG_INFINITY; 4];
    if m.interpolate {
        for x in &m.x {
            for j in 0..m.d {
                lower[j] = lower[j].min(x[j]);
                upper[j] = upper[j].max(x[j]);
            }
        }
    }
    let size = query.len() / m.d;
    let mut fitted = vec![NA_REAL; size];
    let mut error = vec![NA_REAL; if se { size } else { 0 }];
    for i in 0..size {
        let mut point = [0.0; 4];
        for j in 0..m.d {
            point[j] = query[j * size + i] / divisor[j];
            if m.interpolate && (point[j] < lower[j] || point[j] > upper[j]) {
                point[j] = NA_REAL;
            }
        }
        if row(&m, tree.as_ref(), point, robust, &mut buffer) {
            fitted[i] = dot(&buffer, &m.y);
            if se && row(&m, tree.as_ref(), point, &prior, &mut buffer) {
                let variance = buffer
                    .iter()
                    .zip(&m.weights)
                    .filter(|(_, w)| **w > 0.0)
                    .map(|(b, w)| b * b / w)
                    .sum::<f64>();
                error[i] = scale * variance.sqrt();
            }
        }
    }
    let fitted = reals(ctx, &fitted)?;
    let error = reals(ctx, &error)?;
    list(ctx, &[fitted, error])
}
