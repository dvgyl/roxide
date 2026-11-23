//! Test smoothing and minimization through the evaluator with garbage collection enabled.

#![forbid(unsafe_code)]

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::{Host, Limits, NativeHost, PureHost};
use roxide_value::GLOBAL_ENV;

fn check(host: Box<dyn Host>, script: &str) {
    let mut ctx = Ctx::new(host, GcPolicy::Torture).expect("create the evaluator");
    roxide_base::register_all(&mut ctx).expect("register base");
    roxide_stats::register(&mut ctx).expect("register stats");
    ctx.run_script("smooth", script, GLOBAL_ENV, false)
        .expect("run smoothing checks");
}

#[test]
fn closures_retain_knots_and_callback_results() {
    check(
        Box::new(PureHost::new()),
        r#"
scratch <- function() invisible(as.list(as.double(seq_len(50L))))
build <- function() {
    tie <- function(z) { scratch(); max(z) }
    list(approxfun(c(1,2,2,3), c(2,3,4,6), ties = tie),
         splinefun(1:5, (1:5)^3), ecdf(c(3,1,2,2,NA)))
}
f <- build()
scratch()
stopifnot(identical(f[[1L]](c(1,2,3)), c(2,4,6)))
stopifnot(max(abs(f[[2L]](c(1.5,3.5), deriv = 1) - 3*c(1.5,3.5)^2)) < 1e-9)
stopifnot(identical(f[[3L]](c(0,2,3)), c(0,.75,1)))
"#,
    );
}

#[test]
fn optimization_roots_callback_values_and_propagates_nonlocal_exits() {
    check(
        Box::new(PureHost::new()),
        r#"
scratch <- function() invisible(as.list(as.double(seq_len(50L))))
f <- function(p, dest) { scratch(); sum((p - dest)^2) }
g <- function(p, dest) { scratch(); 2 * (p - dest) }
h <- function(p, dest) { scratch(); diag(2, 2) }
x <- nlminb(c(a = 0, b = 0), f, g, h, dest = c(2,-1))
stopifnot(max(abs(x$par - c(2,-1))) < 1e-7, identical(names(x$par), c("a","b")))
a <- function(p) {
    d <- p - c(2,-1)
    scratch()
    structure(sum(d^2), gradient = 2*d, hessian = diag(2,2))
}
x <- nlm(a, c(0,0), hessian = TRUE)
stopifnot(max(abs(x$estimate - c(2,-1))) < 1e-7, max(abs(x$hessian - diag(2,2))) < 1e-6)
err <- function(expr) tryCatch(expr, error = function(e) conditionMessage(e))
stopifnot(identical(err(nlminb(0, function(x) stop("objective exit"))), "objective exit"))
stopifnot(identical(err(nlminb(0, function(x) x^2,
                              function(x) stop("gradient exit"))), "gradient exit"))
stopifnot(identical(err(nlminb(1, function(x) x^2, function(x) 2*x,
                              function(x) stop("Hessian exit"))), "Hessian exit"))
stopifnot(identical(err(nlm(function(x) stop("nlm exit"), 0)), "nlm exit"))
exit <- function() {
    nlm(function(x) invokeRestart("done"), 0)
    FALSE
}
stopifnot(withRestarts(exit(), done = function() TRUE))
"#,
    );
}

#[test]
fn workspace_and_iteration_limits_apply_before_large_allocations() {
    check(
        Box::new(NativeHost::with_limits(Limits {
            max_vector_bytes: 1024,
            ..Limits::default()
        })),
        r#"
err <- function(expr) tryCatch(expr, error = function(e) conditionMessage(e))
stopifnot(identical(err(density(1:5)), "vector size limit exceeded"))
stopifnot(identical(err(nlm(function(x) sum(x^2), rep(0, 12))), "vector size limit exceeded"))
stopifnot(identical(err(lowess(1:5, 1:5, iter = 1001L)), "invalid lowess inputs"))
fit <- nlminb(c(0,0), function(x) sum((x - 2)^2), control = list(eval.max = 1L))
stopifnot(fit$convergence == 1L, fit$evaluations[[1L]] == 1L)
fit <- nlm(function(x) exp(-x), 0, stepmax = .1, gradtol = 0)
stopifnot(fit$code == 5L, fit$iterations == 5L)
"#,
    );
}
