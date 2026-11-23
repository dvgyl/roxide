//! Exercise spectral bindings during collection and at allocation limits.

#![forbid(unsafe_code)]

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::{Limits, NativeHost, PureHost};
use roxide_value::GLOBAL_ENV;

#[test]
fn matrix_spectral_bindings_under_collection() {
    let mut ctx = Ctx::new(Box::new(PureHost::new()), GcPolicy::Torture).expect("create evaluator");
    roxide_base::register_all(&mut ctx).expect("register base");
    ctx.run_script(
        "linalg_spectral.R",
        r#"
A <- matrix(c(1,2,3,4,5,6),2,dimnames=list(c("a","b"),c("x","y","z")))
s <- svd(A,nu=10,nv=10)
l <- La.svd(A,nu=10,nv=10)
stopifnot(identical(names(s),c("d","u","v")), identical(t(l$vt),s$v),
          identical(dim(s$u),c(2L,2L)), identical(dim(s$v),c(3L,3L)),
          is.null(dimnames(s$u)), is.null(dimnames(l$vt)),
          max(abs(s$u %*% diag(s$d) %*% t(s$v[,1:2]) - A)) < 1e-13)
B <- matrix(c(0,1,-1,0),2)
e <- eigen(B)
stopifnot(identical(class(e),"eigen"), identical(class(eigen(B,only.values=TRUE)),"list"),
          is.complex(e$values), is.complex(e$vectors),
          max(Mod(B %*% e$vectors - e$vectors %*% diag(e$values))) < 1e-13)
stopifnot(isSymmetric(matrix(c(1,2i,-2i,1),2)))
msg <- function(expr) tryCatch({force(expr); ""},error=function(e) conditionMessage(e))
cs <- svd(B+1i)
stopifnot(max(Mod(cs$u %*% diag(cs$d) %*% Conj(t(cs$v)) - (B+1i))) < 1e-12)
ce <- eigen(B+1i)
stopifnot(is.complex(ce$values), is.complex(ce$vectors),
          max(Mod((B+1i) %*% ce$vectors - ce$vectors %*% diag(ce$values))) < 1e-12)
stopifnot(identical(msg(svd(matrix("3",1))),"infinite or missing values in 'x'"))
w <- withCallingHandlers(kappa(A,norm="2"),warning=function(w) invokeRestart("muffleWarning"))
stopifnot(abs(w - kappa(A)) < 1e-13)
        "#,
        GLOBAL_ENV,
        false,
    )
    .expect("check spectral bindings");
}

#[test]
fn spectral_output_checks_vector_limit() {
    let limits = Limits {
        max_vector_bytes: 64 * 1024,
        ..Limits::default()
    };
    let mut ctx = Ctx::new(Box::new(NativeHost::with_limits(limits)), GcPolicy::Torture)
        .expect("create evaluator");
    roxide_base::register_all(&mut ctx).expect("register base");
    ctx.run_script(
        "linalg_spectral_limits.R",
        r#"
check <- function(expr) {
    msg <- tryCatch({force(expr); ""},error=function(e) conditionMessage(e))
    stopifnot(identical(msg,"vector size limit exceeded"))
}
A <- matrix(1,300,1)
check(svd(A,nu=300))
check(La.svd(t(A),nv=300))
stopifnot(length(svd(A,nu=0,nv=0)$d)==1)
A <- numeric(6400)
dim(A) <- c(80L,80L)
check(eigen(A,symmetric=FALSE))
stopifnot(length(eigen(A,symmetric=FALSE,only.values=TRUE)$values)==80)
        "#,
        GLOBAL_ENV,
        false,
    )
    .expect("check spectral vector limits");
}
