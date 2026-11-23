use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_math::{
    Cplx, MatRef,
    linalg::blas::{self, ComplexMatRef},
};
use roxide_value::{CharId, RComplex, SEXP_MISSING, SEXP_NIL, SexpType, ValueApi};

use super::{
    axes, axis_keys, complex, dimensions, math_error, numeric, real, set_axis_keys, shape, size,
};
use crate::ops::common::error;

#[derive(Clone, Copy)]
enum Product {
    Matrix,
    Cross,
    TransposeCross,
}

pub(super) fn matprod(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    product(
        ctx,
        call,
        args.values()[0],
        args.values()[1],
        Product::Matrix,
    )
}

pub(super) fn crossprod(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    cross(ctx, call, args, Product::Cross, "crossprod")
}

pub(super) fn tcrossprod(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    cross(ctx, call, args, Product::TransposeCross, "tcrossprod")
}

fn cross(ctx: &mut Ctx, call: &Call, args: &Args, op: Product, name: &str) -> EvalResult {
    if args.is_empty() {
        return Err(error(
            ctx,
            call,
            format!("0 arguments passed to '{name}' which requires at least 1"),
        ));
    }
    if let Some(i) = args.values().iter().position(|&x| x == SEXP_MISSING) {
        return Err(error(ctx, call, format!("argument {} is empty", i + 1)));
    }
    if args.len() > 2 {
        ctx.warn_now(
            call.err_call(),
            format!("more than 2 arguments passed to default method of '{name}'"),
        )?;
    }
    product(ctx, call, args.get(0), args.get(1), op)
}

fn promote(
    op: Product,
    a: Option<(usize, usize)>,
    b: Option<(usize, usize)>,
    x: usize,
    y: usize,
) -> ((usize, usize), (usize, usize)) {
    let row = |n| (1, n);
    let col = |n| (n, 1);
    match (a, b) {
        (Some(a), Some(b)) => (a, b),
        (None, None) => match op {
            Product::Matrix => (row(x), col(y)),
            Product::Cross | Product::TransposeCross => (col(x), col(y)),
        },
        (None, Some(b)) => (
            match op {
                Product::Matrix if x == b.0 => row(x),
                Product::Matrix if b.0 == 1 => col(x),
                Product::Matrix => row(x),
                Product::Cross => col(x),
                Product::TransposeCross if b.1 == 1 => col(x),
                Product::TransposeCross => row(x),
            },
            b,
        ),
        (Some(a), None) => (
            a,
            match op {
                Product::Matrix if y == a.1 => col(y),
                Product::Matrix if a.1 == 1 => row(y),
                Product::Matrix => col(y),
                Product::Cross if y == a.0 => col(y),
                Product::Cross if a.0 == 1 => row(y),
                Product::Cross => col(y),
                Product::TransposeCross if a.1 == 1 => col(y),
                Product::TransposeCross if a.0 == 1 => row(y),
                Product::TransposeCross => col(y),
            },
        ),
    }
}

fn product(ctx: &mut Ctx, call: &Call, x: SexpId, y: SexpId, op: Product) -> EvalResult {
    let same = !matches!(op, Product::Matrix) && y == SEXP_NIL;
    let y = if same { x } else { y };
    let s = ctx.session();
    let (xt, yt) = (s.type_of(x)?, s.type_of(y)?);
    if !numeric(xt) || !numeric(yt) {
        return Err(error(
            ctx,
            call,
            "requires numeric/complex matrix/vector arguments",
        ));
    }
    let (xd, yd) = (dimensions(s, x), dimensions(s, y));
    let (a, b) = promote(op, xd, yd, s.xlength(x)?, s.xlength(y)?);

    let (nr, k, l, nc, i, j) = match op {
        Product::Matrix => (a.0, a.1, b.0, b.1, 0, 1),
        Product::Cross => (a.1, a.0, b.0, b.1, 1, 1),
        Product::TransposeCross => (a.0, a.1, b.1, b.0, 0, 0),
    };
    if k != l {
        return Err(error(ctx, call, "non-conformable arguments"));
    }
    let labels = [
        xd.map_or(SEXP_NIL, |_| axes(s, x)[i]),
        yd.map_or(SEXP_NIL, |_| axes(s, y)[j]),
    ];
    let keys = [
        xd.map_or(CharId::BLANK, |_| axis_keys::<2>(s, x)[i]),
        yd.map_or(CharId::BLANK, |_| axis_keys::<2>(s, y)[j]),
    ];
    let out = if xt == SexpType::Cplx || yt == SexpType::Cplx {
        let n = size(ctx, call, nr, nc, size_of::<Cplx>())?;
        let (x, y) = (complex(ctx, call, x)?, complex(ctx, call, y)?);
        let run = || {
            let a = ComplexMatRef::new(&x, a.0, a.1)?;
            let b = ComplexMatRef::new(&y, b.0, b.1)?;
            let mut out = vec![Cplx::default(); n];
            match op {
                Product::Matrix => blas::matprod_cplx(a, b, &mut out)?,
                Product::Cross => blas::crossprod_cplx(a, b, &mut out)?,
                Product::TransposeCross => blas::tcrossprod_cplx(a, b, &mut out)?,
            }
            Ok(out)
        };
        let data: Vec<_> = run()
            .map_err(|e| math_error(ctx, call, e))?
            .into_iter()
            .map(|v| RComplex { re: v.re, im: v.im })
            .collect();
        ctx.session_mut().alloc_cplx_from(&data)?
    } else {
        let n = size(ctx, call, nr, nc, size_of::<f64>())?;
        let x = real(ctx, call, x)?;
        let y = if same {
            None
        } else {
            Some(real(ctx, call, y)?)
        };
        let run = || {
            let a = MatRef::new(&x, a.0, a.1)?;
            let b = MatRef::new(y.as_deref().unwrap_or(&x), b.0, b.1)?;
            let mut out = vec![0.0; n];
            match (op, same) {
                (Product::Matrix, _) => blas::matprod(a, b, &mut out)?,
                (Product::Cross, true) => blas::syrk_crossprod(a, &mut out)?,
                (Product::Cross, false) => blas::crossprod(a, b, &mut out)?,
                (Product::TransposeCross, true) => blas::syrk_tcrossprod(a, &mut out)?,
                (Product::TransposeCross, false) => blas::tcrossprod(a, b, &mut out)?,
            }
            Ok(out)
        };
        let data = run().map_err(|e| math_error(ctx, call, e))?;
        ctx.session_mut().alloc_real_from(&data)?
    };
    let s = ctx.session_mut();
    shape(s, out, nr, nc, labels)?;
    set_axis_keys(s, out, &keys)
}
