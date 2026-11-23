use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_value::{SEXP_NIL, SexpType, ValueApi};

use crate::distn::support::{attr, limit, numbers, set_attr};

use super::support::{
    area, choice, dimensions, ints, list, matrix, pair_count, real, size, strings,
};

#[derive(Clone, Copy)]
enum Linkage {
    Ward,
    WardSquared,
    Single,
    Complete,
    Average,
    McQuitty,
    Median,
    Centroid,
}

impl Linkage {
    fn update(self, a: f64, b: f64, c: f64, left: f64, right: f64, other: f64) -> f64 {
        let total = left + right;
        match self {
            Self::Ward | Self::WardSquared => {
                ((left + other) * a + (right + other) * b - other * c) / (total + other)
            }
            Self::Single => a.min(b),
            Self::Complete => a.max(b),
            Self::Average => (left * a + right * b) / total,
            Self::McQuitty => (a + b) * 0.5,
            Self::Median => (a + b) * 0.5 - c * 0.25,
            Self::Centroid => (left * a + right * b - left * right * c / total) / total,
        }
    }
}

struct Distances {
    values: Vec<f64>,
    n: usize,
}

impl Distances {
    fn index(&self, i: usize, j: usize) -> usize {
        let a = i.min(j);
        let b = i.max(j);
        a * (2 * self.n - a - 1) / 2 + b - a - 1
    }
    fn get(&self, i: usize, j: usize) -> f64 {
        self.values[self.index(i, j)]
    }
    fn set(&mut self, i: usize, j: usize, d: f64) {
        let index = self.index(i, j);
        self.values[index] = d;
    }
    fn nearest(&self, active: &[i32], i: usize) -> Option<usize> {
        let mut best = None;
        for (j, &node) in active.iter().enumerate().skip(i + 1) {
            if node != 0 && best.is_none_or(|k| self.get(i, j) < self.get(i, k)) {
                best = Some(j);
            }
        }
        best
    }
}

pub(super) fn hclust(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let d = args.get(0);
    let method = args.get(1);
    let linkage = match choice(ctx, method) {
        Some("ward.D") => Linkage::Ward,
        Some("ward.D2") => Linkage::WardSquared,
        Some("single") => Linkage::Single,
        Some("complete") => Linkage::Complete,
        Some("average") => Linkage::Average,
        Some("mcquitty") => Linkage::McQuitty,
        Some("median") => Linkage::Median,
        Some("centroid") => Linkage::Centroid,
        _ => return Err(ctx.stop(call.err_call(), "invalid clustering method")),
    };
    let n = size(ctx, call, d)?;
    if n < 2 {
        return Err(ctx.stop(call.err_call(), "must have n >= 2 objects to cluster"));
    }
    let len = pair_count(ctx, call, n)?;
    let mut values = numbers(ctx, call, d)?;
    if values.len() != len {
        if values.len() < len {
            return Err(ctx.stop(call.err_call(), "dissimilarities of improper length"));
        }
        ctx.warn_now(call.err_call(), "dissimilarities of improper length")?;
        values.truncate(len);
    }
    if values.iter().any(|v| !v.is_finite()) {
        return Err(ctx.stop(
            call.err_call(),
            "NA/NaN/Inf in foreign function call (arg 10)",
        ));
    }
    if matches!(linkage, Linkage::WardSquared) {
        for value in &mut values {
            *value *= *value;
        }
    }
    let mut masses = if args.get(2) == SEXP_NIL {
        vec![1.0; n]
    } else {
        let masses = numbers(ctx, call, args.get(2))?;
        if masses.len() != n {
            return Err(ctx.stop(call.err_call(), "invalid length of members"));
        }
        if masses.iter().any(|v| !v.is_finite()) {
            return Err(ctx.stop(
                call.err_call(),
                "NA/NaN/Inf in foreign function call (arg 7)",
            ));
        }
        masses
    };
    let len = area::<i32>(ctx, call, n - 1, 2)?;
    limit::<Option<usize>>(ctx, call, n)?;
    let mut distances = Distances { values, n };
    let mut active: Vec<_> = (1..=n as i32).map(|i| -i).collect();
    let mut nearest: Vec<_> = (0..n).map(|i| distances.nearest(&active, i)).collect();
    let mut merge = vec![0; len];
    let mut heights = Vec::with_capacity(n - 1);

    for step in 0..n - 1 {
        let mut best = None;
        for (i, &candidate) in nearest.iter().enumerate() {
            if let Some(j) = candidate {
                let d = distances.get(i, j);
                if best.is_none_or(|(_, _, distance)| d < distance) {
                    best = Some((i, j, d));
                }
            }
        }
        let Some((i, j, height)) = best else {
            return Err(ctx.stop(call.err_call(), "invalid cluster distances"));
        };
        let (mut left, mut right) = (active[i], active[j]);
        if (left > 0 && right < 0) || (left > 0 && right > 0 && left > right) {
            std::mem::swap(&mut left, &mut right);
        }
        merge[step] = left;
        merge[step + n - 1] = right;
        heights.push(if matches!(linkage, Linkage::WardSquared) {
            height.sqrt()
        } else {
            height
        });
        active[i] = step as i32 + 1;
        active[j] = 0;
        for (k, &node) in active.iter().enumerate() {
            if node != 0 && k != i {
                let value = linkage.update(
                    distances.get(i, k),
                    distances.get(j, k),
                    height,
                    masses[i],
                    masses[j],
                    masses[k],
                );
                distances.set(i, k, value);
            }
        }
        masses[i] += masses[j];
        for (k, candidate) in nearest.iter_mut().enumerate() {
            if active[k] == 0 {
                *candidate = None;
            } else if k == i || *candidate == Some(i) || *candidate == Some(j) {
                *candidate = distances.nearest(&active, k);
            } else if k < i
                && candidate.is_none_or(|old| distances.get(k, i) < distances.get(k, old))
            {
                *candidate = Some(i);
            }
        }
    }
    let mut stack = vec![(n - 1) as i32];
    let mut order = Vec::with_capacity(n);
    while let Some(node) = stack.pop() {
        if node < 0 {
            order.push(-node);
        } else {
            stack.push(merge[node as usize + n - 2]);
            stack.push(merge[node as usize - 1]);
        }
    }

    let merge = ints(ctx, &merge)?;
    matrix(ctx, call, merge, (n - 1, 2), (SEXP_NIL, SEXP_NIL))?;
    let height = real(ctx, &heights)?;
    let order = ints(ctx, &order)?;
    let labels = attr(ctx, d, "Labels")?;
    let metric = attr(ctx, d, "method")?;
    list(
        ctx,
        &[
            ("merge", merge),
            ("height", height),
            ("order", order),
            ("labels", labels),
            ("method", method),
            ("call", args.get(3)),
            ("dist.method", metric),
        ],
        Some("hclust"),
    )
}

fn field(ctx: &mut Ctx, call: &Call, tree: roxide_gc::SexpId, name: &str) -> EvalResult {
    let names = attr(ctx, tree, "names")?;
    if let Ok(names) = ctx.session().str_slice(names) {
        for (i, &id) in names.iter().enumerate() {
            if ctx.session().char_bytes(id).ok() == Some(name.as_bytes()) {
                return Ok(ctx.session().list_get(tree, i)?);
            }
        }
    }
    if name == "merge" {
        return Err(ctx.stop(call.err_call(), "invalid 'tree' ('merge' component)"));
    }
    Ok(SEXP_NIL)
}

fn root(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

pub(super) fn cutree(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let tree = args.get(0);
    let merge = field(ctx, call, tree, "merge")?;
    let (rows, cols) = dimensions(ctx, call, merge)?;
    let n = rows + 1;
    let len = area::<i32>(ctx, call, rows, 2)?;
    if cols != 2 || ctx.session().xlength(merge)? != len {
        return Err(ctx.stop(call.err_call(), "invalid 'tree' ('merge' component)"));
    }
    let k = args.get(1);
    let h = args.get(2);
    let (cuts, names) = if k != SEXP_NIL {
        let cuts = numbers(ctx, call, k)?;
        if cuts.is_empty() {
            ctx.warn_now(
                call.err_call(),
                "no non-missing arguments to min; returning Inf",
            )?;
            ctx.warn_now(
                call.err_call(),
                "no non-missing arguments to max; returning -Inf",
            )?;
        }
        let names = if cuts.is_empty() {
            SEXP_NIL
        } else {
            strings(ctx, cuts.iter().map(|x| (*x as i32).to_string()))?
        };
        (cuts, names)
    } else {
        if h == SEXP_NIL {
            return Err(ctx.stop(call.err_call(), "either 'k' or 'h' must be specified"));
        }
        let heights = field(ctx, call, tree, "height")?;
        let heights = numbers(ctx, call, heights)?;
        if heights.len() != rows
            || heights.iter().any(|x| x.is_nan())
            || heights.windows(2).any(|w| w[0] > w[1])
        {
            return Err(ctx.stop(
                call.err_call(),
                "the 'height' component of 'tree' is not sorted (increasingly)",
            ));
        }
        let (names, _) = ctx.session_mut().coerce(h, SexpType::Str)?;
        ctx.protect(names);
        let h = numbers(ctx, call, h)?;
        if h.iter().any(|x| x.is_nan()) {
            return Err(ctx.stop(call.err_call(), "non-numeric argument to binary operator"));
        }
        let names = if h.is_empty() { SEXP_NIL } else { names };
        let cuts = h
            .iter()
            .map(|&x| {
                if x == f64::INFINITY {
                    n as f64
                } else {
                    (n - heights.partition_point(|&v| v <= x)) as f64
                }
            })
            .collect();
        (cuts, names)
    };
    if cuts.iter().any(|x| x.is_nan()) {
        return Err(ctx.stop(call.err_call(), "missing value where TRUE/FALSE needed"));
    }
    if cuts
        .iter()
        .any(|&x| !x.is_finite() || x < 1.0 || x > n as f64)
    {
        return Err(ctx.stop(
            call.err_call(),
            format!("elements of 'k' must be between 1 and {n}"),
        ));
    }
    let len = area::<i32>(ctx, call, n, cuts.len())?;
    limit::<usize>(ctx, call, n)?;
    let merges = numbers(ctx, call, merge)?;
    let mut out = vec![0; len];
    let mut parent: Vec<_> = (0..n).collect();
    let mut nodes = Vec::with_capacity(rows);
    let mut levels: Vec<_> = cuts
        .iter()
        .enumerate()
        .map(|(col, &k)| (n - k as usize, col))
        .collect();
    levels.sort_unstable();
    let mut step = 0;
    for (cut, col) in levels {
        while step < cut {
            let decode = |code: f64| {
                if code <= -1.0 && -code <= n as f64 {
                    Some((-code) as usize - 1)
                } else if code >= 1.0 {
                    nodes.get(code as usize - 1).copied()
                } else {
                    None
                }
            };
            let (Some(a), Some(b)) = (decode(merges[step]), decode(merges[rows + step])) else {
                return Err(ctx.stop(call.err_call(), "invalid 'tree' ('merge' component)"));
            };
            let a = root(&mut parent, a);
            let b = root(&mut parent, b);
            parent[b] = a;
            nodes.push(a);
            step += 1;
        }
        let target = &mut out[col * n..(col + 1) * n];
        let mut classes = vec![0; n];
        let mut next = 0;
        for (i, slot) in target.iter_mut().enumerate() {
            let root = root(&mut parent, i);
            if classes[root] == 0 {
                next += 1;
                classes[root] = next;
            }
            *slot = classes[root];
        }
    }
    let labels = field(ctx, call, tree, "labels")?;
    let result = ints(ctx, &out)?;
    if cuts.len() == 1 {
        set_attr(ctx, result, "names", labels)?;
    } else {
        matrix(ctx, call, result, (n, cuts.len()), (labels, names))?;
        if labels == SEXP_NIL && names == SEXP_NIL {
            let names = ctx.session_mut().alloc_list(2)?;
            ctx.protect(names);
            set_attr(ctx, result, "dimnames", names)?;
        }
    }
    Ok(result)
}
