//! Base matrix operations and linear algebra bindings.

#![deny(missing_docs)]

mod factor;
mod matrix;
mod product;
mod qr;
mod spectral;

use roxide_eval::{
    ArgEval, Arity, Call, Ctx, Dispatch, EvalResult, GroupGeneric, Module, PrimDef, PrimFn,
    PrimKind, Unwind, Visibility,
};
use roxide_gc::SexpId;
use roxide_math::{Cplx, MatRef, MathError};
use roxide_value::{CharId, RSession, SEXP_NIL, SexpType, ValueApi};

use crate::ops::common::{attr, chars, error, list, set, vector_limit};

/// Base matrix primitives and argument-matching wrappers.
pub const MODULE: Module = Module {
    prims: &[
        matrix_op("%*%", Arity::Exact(2), ArgEval::Eval, product::matprod),
        matrix_op(
            "crossprod",
            Arity::Any,
            ArgEval::EvalKeepMissing,
            product::crossprod,
        ),
        matrix_op(
            "tcrossprod",
            Arity::Any,
            ArgEval::EvalKeepMissing,
            product::tcrossprod,
        ),
        internal("qr", 3, qr::qr),
        internal("qr_extract", 4, qr::extract),
        internal("qr_apply", 3, qr::apply),
        internal("qr_solve", 4, qr::solve),
        internal("chol", 3, factor::chol),
        internal("chol2inv", 2, factor::chol2inv),
        internal("solve", 4, factor::solve),
        internal("rcond", 4, factor::rcond),
        internal("determinant", 2, factor::determinant),
        internal("backsolve", 5, factor::backsolve),
        internal("spectral_svd", 4, spectral::svd),
        internal("spectral_eigen", 4, spectral::eigen),
        internal("spectral_symmetric", 5, spectral::is_symmetric),
        internal("spectral_norm", 2, spectral::norm),
        internal("spectral_kappa", 6, spectral::kappa),
        internal("transpose", 1, matrix::transpose),
        internal("matrix_diag", 4, matrix::diag),
        internal("matrix_diag_set", 2, matrix::diag_set),
        internal("matrix_triangle", 3, matrix::triangle),
        internal("matrix_summary", 4, matrix::summary),
        internal("matrix_outer", 5, matrix::outer),
    ],
    prelude: r#"
qr <- function(x, ...) UseMethod("qr")
qr.default <- function(x, tol = 1e-7, LAPACK = FALSE, ...) .Internal(qr(x, tol, if (LAPACK) TRUE else FALSE))
is.qr <- function(x) inherits(x, "qr")
qr.Q <- function(qr, complete = FALSE, Dvec) .Internal(qr_extract(qr, complete, if (missing(Dvec)) NULL else Dvec, "Q"))
qr.R <- function(qr, complete = FALSE) .Internal(qr_extract(qr, complete, NULL, "R"))
qr.X <- function(qr, complete = FALSE, ncol)
    .Internal(qr_extract(qr, complete, if (missing(ncol)) NULL else ncol, "X"))
qr.coef <- function(qr, y) .Internal(qr_apply(qr, y, "coef"))
qr.resid <- function(qr, y) .Internal(qr_apply(qr, y, "resid"))
qr.fitted <- function(qr, y, k = qr$rank) .Internal(qr_apply(qr, y, k))
qr.qty <- function(qr, y) .Internal(qr_apply(qr, y, "qty"))
qr.qy <- function(qr, y) .Internal(qr_apply(qr, y, "qy"))
qr.solve <- function(a, b, tol = 1e-7) .Internal(qr_solve(a, if (missing(b)) NULL else b, tol, missing(b)))
solve <- function(a, b, ...) UseMethod("solve")
solve.default <- function(a, b, tol = 2^-52, LINPACK = FALSE, ...) {
    if (LINPACK) stop("the LINPACK argument has been defunct since R 3.1.0")
    .Internal(solve(a, if (missing(b)) NULL else b, tol, missing(b)))
}
solve.qr <- function(a, b, ...) .Internal(qr_solve(a, if (missing(b)) NULL else b, 1e-7, missing(b)))
chol <- function(x, ...) UseMethod("chol")
chol.default <- function(x, pivot = FALSE, LINPACK = FALSE, tol = -1, ...) {
    if (LINPACK) stop("the LINPACK argument has been defunct since R 3.1.0")
    .Internal(chol(x, pivot, tol))
}
chol2inv <- function(x, size = if (is.matrix(x)) ncol(x) else 1L, LINPACK = FALSE) {
    if (LINPACK) stop("the LINPACK argument has been defunct since R 3.1.0")
    .Internal(chol2inv(x, size))
}
determinant <- function(x, logarithm = TRUE, ...) UseMethod("determinant")
determinant.matrix <- function(x, logarithm = TRUE, ...) .Internal(determinant(x, logarithm))
det <- function(x, ...) {
    d <- determinant(x, logarithm = TRUE, ...)
    d$sign * exp(as.double(d$modulus))
}
rcond <- function(x, norm = "O", triangular = FALSE, uplo = "U", ...)
    .Internal(rcond(x, norm, triangular, uplo))
backsolve <- function(r, x, k = ncol(r), upper.tri = TRUE, transpose = FALSE)
    .Internal(backsolve(r, x, if (missing(k)) if (is.matrix(r)) ncol(r) else 1L else k, upper.tri, transpose))
forwardsolve <- function(l, x, k = ncol(l), upper.tri = FALSE, transpose = FALSE)
    .Internal(backsolve(l, x, if (missing(k)) if (is.matrix(l)) ncol(l) else 1L else k, upper.tri, transpose))
svd <- function(x, nu = min(n, p), nv = min(n, p), LINPACK = FALSE) {
    if (!missing(LINPACK)) stop("the LINPACK argument has been defunct since R 3.1.0")
    n <- if (is.matrix(x)) nrow(x) else length(x)
    p <- if (is.matrix(x)) ncol(x) else 1L
    .Internal(spectral_svd(x, nu, nv, FALSE))
}
La.svd <- function(x, nu = min(n, p), nv = min(n, p)) {
    n <- if (is.matrix(x)) nrow(x) else length(x)
    p <- if (is.matrix(x)) ncol(x) else 1L
    .Internal(spectral_svd(x, nu, nv, TRUE))
}
eigen <- function(x, symmetric, only.values = FALSE, EISPACK = FALSE)
    .Internal(spectral_eigen(x, if (missing(symmetric)) NULL else if (symmetric) TRUE else FALSE,
                             only.values, missing(symmetric)))
isSymmetric <- function(object, ...) UseMethod("isSymmetric")
isSymmetric.matrix <- function(object, tol = 100 * 2^-52, tol1 = 8 * tol,
                               trans = "C", ..., check.attributes = TRUE)
    .Internal(spectral_symmetric(object, tol, tol1, trans, check.attributes))
norm <- function(x, type = c("O", "I", "F", "M", "2"))
    .Internal(spectral_norm(x, type))
kappa <- function(z, ...) UseMethod("kappa")
kappa.default <- function(z, exact = FALSE, norm = NULL, method = c("qr", "direct"),
                          inv_z = solve(z), triangular = FALSE, uplo = "U", ...) {
    if (exact && !is.null(norm) && norm != "2") {
        if (!is.matrix(z)) z <- matrix(z, ncol = 1L)
        return(.Internal(spectral_norm(z, norm)) * .Internal(spectral_norm(inv_z, norm)))
    }
    .Internal(spectral_kappa(z, exact, norm, if (exact) NULL else method, triangular, uplo))
}
kappa.matrix <- kappa.default
kappa.qr <- function(z, ...) {
    if (!is.qr(z)) stop("argument is not a QR decomposition")
    kappa.default(qr.R(z), triangular = TRUE, ...)
}
t <- function(x) UseMethod("t")
t.default <- function(x) .Internal(transpose(x))
diag <- function(x = 1, nrow, ncol, names = TRUE) {
    if (missing(x) && missing(nrow) && !missing(ncol))
        stop('argument "nrow" is missing, with no default')
    .Internal(matrix_diag(x, if (missing(nrow)) NULL else nrow,
                          if (missing(ncol)) NULL else ncol, names))
}
`diag<-` <- function(x, value) .Internal(matrix_diag_set(x, value))
lower.tri <- function(x, diag = FALSE) .Internal(matrix_triangle(x, diag, FALSE))
upper.tri <- function(x, diag = FALSE) .Internal(matrix_triangle(x, diag, TRUE))
rowSums <- function(x, na.rm = FALSE, dims = 1) .Internal(matrix_summary(x, na.rm, dims, 0L))
colSums <- function(x, na.rm = FALSE, dims = 1) .Internal(matrix_summary(x, na.rm, dims, 1L))
rowMeans <- function(x, na.rm = FALSE, dims = 1) .Internal(matrix_summary(x, na.rm, dims, 2L))
colMeans <- function(x, na.rm = FALSE, dims = 1) .Internal(matrix_summary(x, na.rm, dims, 3L))
outer <- function(X, Y, FUN = "*", ...) .Internal(matrix_outer(X, Y, FUN, list(...), FALSE))
`%o%` <- function(X, Y) outer(X, Y)
kronecker <- function(X, Y, FUN = "*", make.dimnames = FALSE, ...)
    UseMethod("kronecker")
kronecker.default <- function(X, Y, FUN = "*", make.dimnames = FALSE, ...)
    .Internal(matrix_outer(X, Y, FUN, list(...), if (make.dimnames) 2L else 1L))
`%x%` <- function(X, Y) kronecker(X, Y)
"#,
};

const fn matrix_op(name: &'static str, arity: Arity, args: ArgEval, fun: PrimFn) -> PrimDef {
    PrimDef {
        name,
        kind: PrimKind::Primitive,
        args,
        arity,
        dispatch: Dispatch::Group(GroupGeneric::MatrixOps),
        visibility: Visibility::On,
        fun,
    }
}

const fn internal(name: &'static str, count: usize, fun: PrimFn) -> PrimDef {
    PrimDef {
        name,
        kind: PrimKind::Internal,
        args: ArgEval::Eval,
        arity: Arity::Exact(count),
        dispatch: Dispatch::None,
        visibility: Visibility::On,
        fun,
    }
}

fn numeric(ty: SexpType) -> bool {
    matches!(
        ty,
        SexpType::Lgl | SexpType::Int | SexpType::Real | SexpType::Cplx
    )
}

fn dimensions(s: &RSession, x: SexpId) -> Option<(usize, usize)> {
    let dims = s.int(attr(s, x, "dim")).ok()?;
    if dims.len() != 2 {
        return None;
    }
    Some((
        usize::try_from(dims.get(0)).ok()?,
        usize::try_from(dims.get(1)).ok()?,
    ))
}

fn axes(s: &RSession, x: SexpId) -> [SexpId; 2] {
    let names = attr(s, x, "dimnames");
    [0, 1].map(|i| s.list_get(names, i).unwrap_or(SEXP_NIL))
}

fn axis_keys<const N: usize>(s: &RSession, x: SexpId) -> [CharId; N] {
    let labels = crate::ops::common::names(s, attr(s, x, "dimnames"));
    std::array::from_fn(|i| labels.get(i).copied().unwrap_or(CharId::BLANK))
}

fn set_axis_keys(s: &mut RSession, x: SexpId, keys: &[CharId]) -> EvalResult {
    let labels = attr(s, x, "dimnames");
    if labels != SEXP_NIL && keys.iter().any(|&key| key != CharId::BLANK) {
        crate::ops::common::set_names(s, labels, keys)?;
    }
    Ok(x)
}

fn size(ctx: &mut Ctx, call: &Call, rows: usize, cols: usize, bytes: usize) -> EvalResult<usize> {
    if rows > i32::MAX as usize || cols > i32::MAX as usize {
        return Err(error(ctx, call, "invalid matrix extent"));
    }
    let n = rows
        .checked_mul(cols)
        .ok_or_else(|| error(ctx, call, "vector size limit exceeded"))?;
    vector_limit(ctx, call, n, bytes)?;
    Ok(n)
}

fn shape(s: &mut RSession, x: SexpId, rows: usize, cols: usize, labels: [SexpId; 2]) -> EvalResult {
    let dim = s.alloc_int_from(&[rows as i32, cols as i32])?;
    set(s, x, "dim", dim)?;
    if labels != [SEXP_NIL; 2] {
        let names = list(s, &labels)?;
        set(s, x, "dimnames", names)?;
    }
    Ok(x)
}

fn labels(s: &mut RSession, names: SexpId, index: impl IntoIterator<Item = usize>) -> EvalResult {
    if names == SEXP_NIL {
        return Ok(names);
    }
    let values = chars(s, names);
    let out: Vec<_> = index
        .into_iter()
        .filter_map(|i| values.get(i).copied())
        .collect();
    Ok(if out.is_empty() {
        SEXP_NIL
    } else {
        s.alloc_str_from(&out)?
    })
}

fn math_error(ctx: &mut Ctx, call: &Call, e: MathError) -> Unwind {
    error(ctx, call, e.to_string())
}

struct Dense {
    values: Vec<f64>,
    rows: usize,
    cols: usize,
    labels: [SexpId; 2],
    keys: [CharId; 2],
    matrix: bool,
}

impl Dense {
    fn read(ctx: &mut Ctx, call: &Call, x: SexpId) -> EvalResult<Self> {
        let s = ctx.session();
        let len = s.xlength(x)?;
        let dims = dimensions(s, x);
        let (rows, cols) = dims.unwrap_or((len, 1));
        let labels = dims.map_or([attr(s, x, "names"), SEXP_NIL], |_| axes(s, x));
        if rows.checked_mul(cols) != Some(len) {
            return Err(error(ctx, call, "invalid matrix dimensions"));
        }
        let keys = axis_keys(s, x);
        let values = real(ctx, call, x)?;
        Ok(Self {
            values,
            rows,
            cols,
            labels,
            keys,
            matrix: dims.is_some(),
        })
    }

    fn view(&self) -> MatRef<'_> {
        MatRef {
            data: &self.values,
            nrow: self.rows,
            ncol: self.cols,
        }
    }

    fn result(&self, s: &mut RSession, v: &[f64], keep_matrix: bool) -> EvalResult {
        let out = s.alloc_real_from(v)?;
        if keep_matrix {
            shape(s, out, self.rows, self.cols, self.labels)?;
            set_axis_keys(s, out, &self.keys)
        } else {
            set(s, out, "names", self.labels[0])?;
            Ok(out)
        }
    }
}

fn real(ctx: &mut Ctx, call: &Call, x: SexpId) -> EvalResult<Vec<f64>> {
    let len = ctx.session().xlength(x)?;
    vector_limit(ctx, call, len, size_of::<f64>())?;
    let s = ctx.session_mut();
    let (x, warn) = s.coerce(x, SexpType::Real)?;
    let values = s.real(x)?.iter().collect();
    if warn.imaginary_discarded {
        ctx.warn_now(call.err_call(), "imaginary parts discarded in coercion")?;
    }
    if warn.na_introduced {
        ctx.warn_now(call.err_call(), "NAs introduced by coercion")?;
    }
    Ok(values)
}

fn named_list(s: &mut RSession, items: &[(&str, SexpId)], class: &str) -> EvalResult {
    let values: Vec<_> = items.iter().map(|&(_, value)| value).collect();
    let out = list(s, &values)?;
    let names = items
        .iter()
        .map(|&(name, _)| crate::ops::common::ch(s, name))
        .collect::<Result<Vec<_>, _>>()?;
    crate::ops::common::set_names(s, out, &names)?;
    let class = s.scalar_str(class)?;
    set(s, out, "class", class)?;
    Ok(out)
}

fn complex(ctx: &mut Ctx, call: &Call, value: SexpId) -> EvalResult<Vec<Cplx>> {
    let n = ctx.session().xlength(value)?;
    vector_limit(ctx, call, n, size_of::<Cplx>())?;
    let s = ctx.session_mut();
    let (value, _) = s.coerce(value, SexpType::Cplx)?;
    Ok(s.cplx(value)?
        .iter()
        .map(|v| Cplx::new(v.re, v.im))
        .collect())
}
