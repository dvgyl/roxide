//! Test smoothing and minimization through the evaluator with garbage collection enabled.

#![forbid(unsafe_code)]

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::{Host, Limits, NativeHost, PureHost};
use roxide_value::GLOBAL_ENV;

fn check(host: Box<dyn Host>, script: &str, policy: GcPolicy) {
    let mut ctx = Ctx::new(host, policy).expect("create the evaluator");
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
        GcPolicy::Torture,
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
        GcPolicy::Torture,
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
        GcPolicy::Torture,
    );
}

#[test]
fn smoothing_audit_cases_match_oracle() {
    let script = r#"
near <- function(x, y, tol=1e-8) {
    stopifnot(length(x) == length(y), max(abs(x - y)) < tol)
}
den <- density(c(0,1,Inf), weights=c(.2,.3,.5), bw=1, n=3)
near(den$y, c(.000463999139399597,.0880021367683284,.000679221449227931), 1e-10)
out <- lowess(1:4, c(1,NA,9,16))
stopifnot(identical(out$x, as.double(1:4)), identical(out$y, c(1,NA,9,16)))
err <- function(expr) tryCatch(expr, error=conditionMessage)
stopifnot(identical(err(spline(1:3, n=0)), "'spline' requires n >= 1"))

d <- data.frame(x=1:20, y=sin(1:20), w=1+(1:20)%%3)
fit <- function(statistics, surface="direct", trace.hat="exact") {
    loess(y~x, d, weights=w, family="symmetric", control=loess.control(
        surface=surface, statistics=statistics, trace.hat=trace.hat))
}
g <- fit("approximate")
h <- fit("exact")
near(g$fitted, h$fitted)
near(c(g$fitted[1], g$robust[1]), c(.760769853610006,.998196251502614))
near(c(g$one.delta,g$two.delta,g$s), c(14.6540669350501,14.3684072213361,1.3003339860518))
near(c(h$one.delta,h$two.delta,h$s), c(15.3155460390612,15.7862732526577,1.27194331312369))
p <- predict(g, data.frame(x=c(2,4)), se=TRUE)
named <- predict(g, data.frame(x=c(2,4),row.names=c("1","2")), se=TRUE)
three <- predict(g, data.frame(x=c(2,4,6)), se=TRUE)
omitted <- predict(g, data.frame(x=c(2,NA,6)), se=TRUE, na.action=na.omit)
stopifnot(is.null(names(p$fit)), is.null(names(p$se.fit)),
          identical(names(named$fit), c("1","2")),
          identical(names(named$se.fit), c("1","2")),
          identical(names(three$fit), c("1","2","3")),
          identical(names(omitted$fit), c("1","3")))
near(p$df, g$one.delta^2/g$two.delta)
a <- fit("approximate","interpolate","approximate")
near(c(a$trace.hat,a$one.delta,a$two.delta), c(4.395,15.229973441011,14.9575116529421))
big <- data.frame(x=1:513, y=sin(1:513))
large <- loess(y~x, big, control=loess.control(surface="direct",statistics="exact"))
stopifnot(length(large$fitted) == 513L, is.finite(large$two.delta))
"#;
    oracle_check(script);
    check(Box::new(PureHost::new()), script, GcPolicy::default());
}

#[test]
fn nonlinear_audit_cases_match_oracle() {
    let script = r#"
f <- function(x) (x-2)^2 + 1e10
g <- function(x) 2*(x-2)
fit <- nlminb(0, f, g)
stopifnot(abs(fit$par - 2) < 1e-10, fit$objective == 1e10, fit$convergence == 0)
err <- function(expr) tryCatch(expr,error=conditionMessage)
stopifnot(identical(err(nlminb(0, f, g, function(x) 2)),
                    "Hessian function must return a square numeric matrix of order 1"),
          identical(err(nlm(function(x) x^2, 1, iterlim=0)), "invalid iteration limit in nlm"))
seen <- character()
fit <- withCallingHandlers(nlm(function(x) structure(x*x, gradient="bad"), 1),
    warning=function(w) {
        seen <<- c(seen, conditionMessage(w))
        invokeRestart("muffleWarning")
    })
stopifnot(abs(fit$estimate) < 1e-5,
          identical(seen, "gradient supplied is of the wrong length or mode, so ignored"))
"#;
    oracle_check(script);
    check(Box::new(PureHost::new()), script, GcPolicy::default());
}

#[test]
fn loess_limits_tree_cells_during_allocation() {
    check(
        Box::new(NativeHost::with_limits(Limits {
            max_vector_bytes: 8 * 1024,
            ..Limits::default()
        })),
        r#"
x <- 1:100
y <- sin(x)
ok <- loess(y~x, control=loess.control(statistics="none"))
stopifnot(length(ok$fitted) == 100L)
error <- tryCatch(loess(y~x, control=loess.control(statistics="none",cell=0)),
                  error=conditionMessage)
stopifnot(error == "vector size limit exceeded")
"#,
        GcPolicy::default(),
    );
}

#[test]
fn smoothing_package_closures_roundtrip() {
    let script = r#"
functions <- list(approxfun(1:3), splinefun(1:3), ecdf(c(1,1,2)), sd)
for (version in c(2L,3L)) {
    copies <- unserialize(serialize(functions, NULL, version=version))
    for (i in seq_along(copies)) {
        x <- c(1,1.5,2)
        stopifnot(max(abs(copies[[i]](x) - functions[[i]](x))) < 1e-12)
    }
    stopifnot(identical(knots(copies[[3L]]), knots(functions[[3L]])),
              identical(environment(copies[[4L]]), environment(sd)))
}
"#;
    oracle_check(script);
    check(Box::new(PureHost::new()), script, GcPolicy::default());
}

fn oracle_check(script: &str) {
    if let Some(rscript) = std::env::var_os("ROXIDE_ORACLE_RSCRIPT") {
        let output = std::process::Command::new(rscript)
            .args(["--vanilla", "-e", script])
            .output()
            .expect("Run GNU R");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
