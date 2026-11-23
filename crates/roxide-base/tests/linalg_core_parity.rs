//! Check matrix bindings at the evaluator boundary.

#![forbid(unsafe_code)]

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::PureHost;
use roxide_value::GLOBAL_ENV;

#[test]
fn matrix_bindings_keep_shapes_names_and_values_under_collection() {
    let mut ctx = Ctx::new(Box::new(PureHost::new()), GcPolicy::Torture).expect("create evaluator");
    roxide_base::register_all(&mut ctx).expect("register the base package");
    ctx.run_script(
        "linalg_core_parity.R",
        r#"
A <- matrix(c(4,2,2,3),2,dimnames=list(c("r","s"),c("x","y")))
B <- A
diag(B) <- 7
stopifnot(identical(diag(A), c(4,3)),
          identical(diag(B), c(7,7)),
          identical(dimnames(t(A)), rev(dimnames(A))),
          identical(dimnames(solve(A)), rev(dimnames(A))),
          identical(solve(A,c(2,4)), c(x=-.25,y=1.5)),
          identical(crossprod(1i), matrix(-1+0i,1)),
          identical(NA_real_ %*% 0, matrix(NA_real_,1)),
          identical(outer(1:2,3:4,function(a,b,k) a+b+k,k=2L),
                    matrix(c(6L,7L,7L,8L),2)))
q <- qr(cbind(a=1:3,b=2*(1:3)))
stopifnot(is.qr(q), q$rank == 1L,
          identical(names(qr.coef(q,c(1,2,3))), c("a","b")),
          is.na(qr.coef(q,c(1,2,3))[2]))
        "#,
        GLOBAL_ENV,
        false,
    )
    .expect("check matrix bindings");
}

#[test]
fn matrix_output_checks_the_vector_limit() {
    use roxide_host::{Limits, NativeHost};

    let limits = Limits {
        max_vector_bytes: 64 * 1024,
        ..Limits::default()
    };
    let mut ctx = Ctx::new(Box::new(NativeHost::with_limits(limits)), GcPolicy::Torture)
        .expect("create evaluator");
    roxide_base::register_all(&mut ctx).expect("register the base package");
    ctx.run_script(
        "linalg_limit.R",
        r#"
check <- function(expr) {
    msg <- tryCatch({ force(expr); "" }, error = function(e) conditionMessage(e))
    stopifnot(identical(msg, "vector size limit exceeded"))
}
check(diag(500))
check(tcrossprod(1:500))
check(qr.Q(qr(1:500), complete=TRUE))
        "#,
        GLOBAL_ENV,
        false,
    )
    .expect("check matrix allocation limits");
}

#[test]
fn linear_algebra_uses_the_selected_triangle_and_rhs_dispatch() {
    let mut ctx = roxide_eval::Ctx::new(
        Box::new(roxide_host::PureHost::new()),
        roxide_gc::GcPolicy::Torture,
    )
    .expect("Create the R session");
    roxide_base::register_all(&mut ctx).expect("Register base");
    ctx.run_script(
        "Matrix dispatch and triangles",
        r#"
a <- matrix(c(1,2,3,4,5,6,7,8,10), 3L)
stopifnot(abs(rcond(a, triangular = TRUE) - 0.04) < 1e-12,
          abs(rcond(a, "O", TRUE, "L") - 0.062266500622665) < 1e-12)
err <- tryCatch(solve(matrix("2")), error = conditionMessage)
stopifnot(identical(err, "'a' must be a numeric matrix"), identical(solve(matrix(2), "4"), 2))
matrixOps.audit <- function(x, y) .Generic
f <- structure(diag(2), class = "audit")
stopifnot(identical(diag(2) %*% f, "%*%"), identical(crossprod(diag(2), f), "crossprod"))
"#,
        roxide_value::GLOBAL_ENV,
        false,
    )
    .expect("Check triangle selection and matrix dispatch");
}
