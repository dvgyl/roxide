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
