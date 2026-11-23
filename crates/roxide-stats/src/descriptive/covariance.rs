use std::cmp::Ordering;

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{CharId, NA_REAL, SEXP_NIL, SexpType, ValueApi};

use crate::distn::support::{
    attr, copy_attrs, has_class, limit, numbers, numeric, set_dim, set_dimnames,
};

use super::{average, boolean};

const USES: &[&str] = &[
    "everything",
    "all.obs",
    "complete.obs",
    "na.or.complete",
    "pairwise.complete.obs",
];
const METHODS: &[&str] = &["pearson", "kendall", "spearman"];
const ZERO_SD: &str = "the standard deviation is zero";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Use {
    Everything,
    All,
    Complete,
    NaOrComplete,
    Pairwise,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Method {
    Pearson,
    Kendall,
    Spearman,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Measure {
    Variance,
    Covariance,
    Correlation,
}

fn match_choice(ctx: &Ctx, value: SexpId, choices: &[&str]) -> Option<usize> {
    let &id = ctx.session().str_slice(value).ok()?.first()?;
    if id == CharId::NA {
        return None;
    }
    let text = ctx.session().char_bytes(id).ok()?;
    let mut prefixes = choices
        .iter()
        .enumerate()
        .filter(|(_, item)| item.as_bytes().starts_with(text));
    let (i, _) = prefixes.next()?;
    if text.is_empty() || prefixes.next().is_some() {
        return None;
    }
    Some(i)
}

fn use_mode(ctx: &mut Ctx, call: &Call, value: SexpId) -> EvalResult<Use> {
    match ctx.session().xlength(value)? {
        0 => return Err(ctx.stop(call.err_call(), "argument is of length zero")),
        1 => {}
        _ => return Err(ctx.stop(call.err_call(), "the condition has length > 1")),
    }
    match match_choice(ctx, value, USES) {
        Some(0) => Ok(Use::Everything),
        Some(1) => Ok(Use::All),
        Some(2) => Ok(Use::Complete),
        Some(3) => Ok(Use::NaOrComplete),
        Some(4) => Ok(Use::Pairwise),
        _ => Err(ctx.stop(call.err_call(), "invalid 'use' argument")),
    }
}

fn method(ctx: &mut Ctx, call: &Call, value: SexpId) -> EvalResult<Method> {
    if value == SEXP_NIL {
        return Ok(Method::Pearson);
    }
    if ctx.session().type_of(value)? != SexpType::Str {
        return Err(ctx.stop(call.err_call(), "'arg' must be NULL or a character vector"));
    }
    let strings = ctx.session().str_slice(value)?;
    if strings.len() == METHODS.len()
        && strings
            .iter()
            .zip(METHODS)
            .all(|(&id, &choice)| ctx.session().char_bytes(id).ok() == Some(choice.as_bytes()))
    {
        return Ok(Method::Pearson);
    }
    if strings.len() != 1 {
        return Err(ctx.stop(call.err_call(), "'arg' must be of length 1"));
    }
    match match_choice(ctx, value, METHODS) {
        Some(0) => Ok(Method::Pearson),
        Some(1) => Ok(Method::Kendall),
        Some(2) => Ok(Method::Spearman),
        _ => Err(ctx.stop(
            call.err_call(),
            "'arg' should be one of “pearson”, “kendall”, “spearman”",
        )),
    }
}

struct Columns {
    data: Vec<f64>,
    rows: usize,
    cols: usize,
    matrix: bool,
    names: SexpId,
}

impl Columns {
    fn column(&self, i: usize) -> &[f64] {
        &self.data[i * self.rows..][..self.rows]
    }
}

fn dimensions(ctx: &mut Ctx, x: SexpId) -> EvalResult<Option<(usize, usize)>> {
    let dim = attr(ctx, x, "dim")?;
    if dim == SEXP_NIL {
        return Ok(None);
    }
    let dim = ctx.session().int(dim)?;
    Ok((dim.len() == 2).then(|| (dim.get(0) as usize, dim.get(1) as usize)))
}

fn columns(ctx: &mut Ctx, call: &Call, x: SexpId, name: &str, coerce: bool) -> EvalResult<Columns> {
    if has_class(ctx, x, "data.frame")? {
        let cols = ctx.session().xlength(x)?;
        let names = attr(ctx, x, "names")?;
        let rows = if cols == 0 {
            let row_names = attr(ctx, x, "row.names")?;
            ctx.session().xlength(row_names)?
        } else {
            ctx.session().xlength(ctx.session().list_get(x, 0)?)?
        };
        let len = rows.saturating_mul(cols);
        limit::<f64>(ctx, call, len)?;
        let mut data = Vec::with_capacity(len);
        for i in 0..cols {
            let column = ctx.session().list_get(x, i)?;
            if !coerce && (!numeric(ctx, column)? || has_class(ctx, column, "factor")?) {
                return Err(ctx.stop(call.err_call(), format!("'{name}' must be numeric")));
            }
            if ctx.session().xlength(column)? != rows {
                return Err(ctx.stop(call.err_call(), "incompatible dimensions"));
            }
            data.extend(numbers(ctx, call, column)?);
        }
        return Ok(Columns {
            data,
            rows,
            cols,
            matrix: true,
            names,
        });
    }
    if has_class(ctx, x, "factor")? {
        let error = if coerce {
            format!(
                "Calling var({name}) on a factor {name} is defunct.\n  Use something like 'all(duplicated({name})[-1L])' to test for a constant vector."
            )
        } else {
            format!("is.numeric({name}) || is.logical({name}) is not TRUE")
        };
        return Err(ctx.stop(call.err_call(), error));
    }
    if !coerce && !numeric(ctx, x)? {
        return Err(ctx.stop(call.err_call(), format!("'{name}' must be numeric")));
    }
    let data = numbers(ctx, call, x)?;
    if let Some((rows, cols)) = dimensions(ctx, x)? {
        if rows.checked_mul(cols) != Some(data.len()) {
            return Err(ctx.stop(call.err_call(), "incompatible dimensions"));
        }
        let dimnames = attr(ctx, x, "dimnames")?;
        let names = if dimnames == SEXP_NIL {
            SEXP_NIL
        } else {
            ctx.session().list_get(dimnames, 1)?
        };
        Ok(Columns {
            data,
            rows,
            cols,
            matrix: true,
            names,
        })
    } else {
        Ok(Columns {
            rows: data.len(),
            data,
            cols: 1,
            matrix: false,
            names: SEXP_NIL,
        })
    }
}

fn rank(x: &[f64]) -> Vec<f64> {
    let mut order: Vec<_> = (0..x.len()).collect();
    order.sort_unstable_by(|&a, &b| x[a].partial_cmp(&x[b]).unwrap_or(Ordering::Equal));
    let mut out = vec![0.0; x.len()];
    let mut left = 0;
    while left < order.len() {
        let mut right = left + 1;
        while right < order.len() && x[order[left]] == x[order[right]] {
            right += 1;
        }
        let average = (left + 1 + right) as f64 * 0.5;
        for &index in &order[left..right] {
            out[index] = average;
        }
        left = right;
    }
    out
}

// A prefix sum tree counts earlier ranks in logarithmic time.
fn prefix(tree: &[usize], mut index: usize) -> usize {
    let mut count = 0;
    while index > 0 {
        count += tree[index - 1];
        index &= index - 1;
    }
    count
}

fn kendall(x: &[f64], y: &[f64], correlate: bool) -> (f64, bool) {
    let ranks = rank(y);
    let mut order: Vec<_> = (0..x.len()).collect();
    order.sort_unstable_by(|&a, &b| x[a].partial_cmp(&x[b]).unwrap_or(Ordering::Equal));
    let mut tree = vec![0; y.len()];
    let (mut score, mut distinct_x, mut distinct_y) = (0.0, 0.0, 0.0);
    let mut left = 0;
    while left < order.len() {
        let mut right = left + 1;
        while right < order.len() && x[order[left]] == x[order[right]] {
            right += 1;
        }
        for &i in &order[left..right] {
            let r = ranks[i] as usize;
            let less = prefix(&tree, r - 1);
            let greater = left - prefix(&tree, r);
            score += less as f64 - greater as f64;
        }
        distinct_x += left as f64 * (right - left) as f64;
        for (seen, &i) in (left..right).zip(&order[left..right]) {
            let mut r = ranks[i] as usize;
            distinct_y += (seen - (prefix(&tree, r) - prefix(&tree, r - 1))) as f64;
            while r <= tree.len() {
                tree[r - 1] += 1;
                r += 1 << r.trailing_zeros();
            }
        }
        left = right;
    }
    if !correlate {
        return (2.0 * score, false);
    }
    if distinct_x == 0.0 || distinct_y == 0.0 {
        return (NA_REAL, true);
    }
    (
        (score / distinct_x.sqrt() / distinct_y.sqrt()).clamp(-1.0, 1.0),
        false,
    )
}

fn pearson(x: &[f64], y: &[f64], correlate: bool) -> (f64, bool) {
    let (mx, my) = (average(x), average(y));
    let (mut xy, mut xx, mut yy) = (0.0, 0.0, 0.0);
    for (&x, &y) in x.iter().zip(y) {
        let (dx, dy) = (x - mx, y - my);
        xy += dx * dy;
        if correlate {
            xx += dx * dx;
            yy += dy * dy;
        }
    }
    let df = (x.len() - 1) as f64;
    if !correlate {
        return (xy / df, false);
    }
    if xx == 0.0 || yy == 0.0 {
        return (NA_REAL, true);
    }
    let corr = (xy / df) / (xx / df).sqrt() / (yy / df).sqrt();
    (corr.clamp(-1.0, 1.0), false)
}

fn measure_pair(x: &[f64], y: &[f64], method: Method, correlate: bool) -> (f64, bool) {
    if x.len() < 2 || x.iter().chain(y).any(|v| v.is_nan()) {
        return (NA_REAL, false);
    }
    match method {
        Method::Pearson => pearson(x, y, correlate),
        Method::Spearman => pearson(&rank(x), &rank(y), correlate),
        Method::Kendall => kendall(x, y, correlate),
    }
}

pub(super) fn variance(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    calculate(ctx, call, args, Measure::Variance)
}

pub(super) fn cov(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    calculate(ctx, call, args, Measure::Covariance)
}

pub(super) fn cor(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    calculate(ctx, call, args, Measure::Correlation)
}

fn calculate(ctx: &mut Ctx, call: &Call, args: &Args, measure: Measure) -> EvalResult {
    let (x, y) = (args.get(0), args.get(1));
    let symmetric = y == SEXP_NIL;
    let mode = use_mode(ctx, call, args.get(2))?;
    let method = method(ctx, call, args.get(3))?;
    let correlate = measure == Measure::Correlation;
    let coerce = measure == Measure::Variance;
    if symmetric && !coerce && dimensions(ctx, x)?.is_none() && !has_class(ctx, x, "data.frame")? {
        return Err(ctx.stop(
            call.err_call(),
            "supply both 'x' and 'y' or a matrix-like 'x'",
        ));
    }
    if x == SEXP_NIL && coerce {
        return Err(ctx.stop(call.err_call(), "'x' is NULL"));
    }
    let x = columns(ctx, call, x, "x", coerce)?;
    let other = if symmetric {
        None
    } else {
        Some(columns(ctx, call, y, "y", coerce)?)
    };
    let y = other.as_ref().unwrap_or(&x);
    if x.rows != y.rows {
        return Err(ctx.stop(call.err_call(), "incompatible dimensions"));
    }
    if mode == Use::Pairwise && !correlate && method != Method::Pearson {
        return Err(ctx.stop(call.err_call(), "cannot handle 'pairwise.complete.obs'"));
    }
    if x.data.is_empty() && matches!(mode, Use::All | Use::Pairwise) {
        return Err(ctx.stop(call.err_call(), "'x' is empty"));
    }
    if mode == Use::All && x.data.iter().chain(&y.data).any(|v| v.is_nan()) {
        return Err(ctx.stop(call.err_call(), "missing observations in cov/cor"));
    }
    let complete = matches!(mode, Use::Complete | Use::NaOrComplete);
    let rows = if complete {
        limit::<usize>(ctx, call, x.rows)?;
        (0..x.rows)
            .filter(|&row| {
                (0..x.cols).all(|i| !x.column(i)[row].is_nan())
                    && (0..y.cols).all(|j| !y.column(j)[row].is_nan())
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if rows.is_empty() && mode == Use::Complete {
        return Err(ctx.stop(call.err_call(), "no complete element pairs"));
    }
    let len = x.cols.saturating_mul(y.cols);
    limit::<f64>(ctx, call, len)?;
    let mut out = vec![NA_REAL; len];
    let mut zero = false;
    for j in 0..y.cols {
        for i in if symmetric { j } else { 0 }..x.cols {
            let a = x.column(i);
            let b = y.column(j);
            let (value, warn) = if complete {
                let a: Vec<_> = rows.iter().map(|&r| a[r]).collect();
                let b: Vec<_> = rows.iter().map(|&r| b[r]).collect();
                measure_pair(&a, &b, method, correlate)
            } else if mode == Use::Pairwise {
                let (a, b): (Vec<_>, Vec<_>) = a
                    .iter()
                    .copied()
                    .zip(b.iter().copied())
                    .filter(|(a, b)| !a.is_nan() && !b.is_nan())
                    .unzip();
                measure_pair(&a, &b, method, correlate)
            } else {
                measure_pair(a, b, method, correlate)
            };
            let diagonal = symmetric
                && i == j
                && correlate
                && mode != Use::Pairwise
                && if complete {
                    rows.len() >= 2
                } else {
                    x.rows >= 2
                };
            if warn && !diagonal {
                if mode == Use::Pairwise && method != Method::Pearson {
                    ctx.warn_now(call.err_call(), ZERO_SD)?;
                } else {
                    zero = true;
                }
            }
            let value = if diagonal { 1.0 } else { value };
            out[j * x.cols + i] = value;
            if symmetric {
                out[i * x.cols + j] = value;
            }
        }
    }
    if zero {
        ctx.warn_now(call.err_call(), ZERO_SD)?;
    }
    ctx.scoped(|ctx| {
        let value = ctx.session_mut().alloc_real_from(&out)?;
        ctx.protect(value);
        if x.matrix || y.matrix {
            set_dim(ctx, call, value, x.cols, y.cols)?;
            set_dimnames(ctx, value, x.names, y.names)?;
        }
        Ok(value)
    })
}

pub(super) fn sd(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let na_rm = boolean(ctx, call, args.get(1))?;
    let mut x = numbers(ctx, call, args.get(0))?;
    if na_rm {
        x.retain(|x| !x.is_nan());
    }
    let (v, _) = measure_pair(&x, &x, Method::Pearson, false);
    Ok(ctx.session_mut().scalar_real(v.sqrt())?)
}

pub(super) fn cov2cor(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let v = args.get(0);
    let Some((n, m)) = dimensions(ctx, v)? else {
        return Err(ctx.stop(call.err_call(), "'V' is not a square numeric matrix"));
    };
    if n != m || !matches!(ctx.session().type_of(v)?, SexpType::Int | SexpType::Real) {
        return Err(ctx.stop(call.err_call(), "'V' is not a square numeric matrix"));
    }
    let mut out = numbers(ctx, call, v)?;
    if n.checked_mul(n) != Some(out.len()) {
        return Err(ctx.stop(call.err_call(), "'V' is not a square numeric matrix"));
    }
    limit::<f64>(ctx, call, n)?;
    let scale: Vec<_> = (0..n).map(|i| out[i * n + i].recip().sqrt()).collect();
    if scale.iter().any(|x| !x.is_finite()) {
        ctx.warn_now(
            call.err_call(),
            "diag(V) had non-positive or NA entries; the non-finite result may be dubious",
        )?;
    }
    for j in 0..n {
        for i in 0..n {
            let cell = &mut out[j * n + i];
            *cell = if i == j {
                1.0
            } else {
                (*cell * scale[i]) * scale[j]
            };
        }
    }
    ctx.scoped(|ctx| {
        let result = ctx.session_mut().alloc_real_from(&out)?;
        ctx.protect(result);
        copy_attrs(ctx, v, result)?;
        Ok(result)
    })
}
