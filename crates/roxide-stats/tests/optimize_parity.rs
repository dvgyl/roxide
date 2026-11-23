//! Exercise numerical callbacks through stats under garbage collection stress.

#![forbid(unsafe_code)]

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::PureHost;
use roxide_value::GLOBAL_ENV;

#[test]
fn closures_dots_rng_and_nonlocal_exit() {
    let mut ctx =
        Ctx::new(Box::new(PureHost::new()), GcPolicy::Torture).expect("Create the evaluator");
    roxide_base::register_all(&mut ctx).expect("Register base");
    roxide_stats::register(&mut ctx).expect("Register stats");
    ctx.run_script(
        "Optimization callbacks",
        r#"
f <- local({
    offset <- c(1, -2)
    function(x, power) sum((x - offset)^power)
})
g <- local({
    offset <- c(1, -2)
    function(x, power) power * (x - offset)^(power - 1)
})
fit <- stats::optim(c(a = 0, b = 0), f, g, power = 2, method = "BFGS", hessian = TRUE)
stopifnot(max(abs(fit$par - c(1, -2))) < 1e-7)
stopifnot(identical(dimnames(fit$hessian), list(c("a", "b"), c("a", "b"))))
set.seed(42)
expected <- runif(5L)
set.seed(42)
draws <- numeric()
fit <- optim(0, function(x) {
    draws <<- c(draws, runif(1L))
    (x - 1)^2
}, gr = function(x) if (x < 1) x + .5 else x, method = "SANN", control = list(maxit = 5L))
stopifnot(identical(draws, expected), identical(fit$par, 1))
leave <- function(expr) tryCatch(expr, error = function(e) conditionMessage(e))
stopifnot(identical(leave(optimize(function(x) stop("left callback"), c(0, 2))), "left callback"))
stopifnot(abs(integrate(function(x, k) k*x^2, 0, 1, k = 3)$value - 1) < 1e-12)
"#,
        GLOBAL_ENV,
        false,
    )
    .expect("Run the callback checks");
}

#[test]
fn root_warnings_and_optimization_callback_order() {
    let mut ctx = Ctx::new(Box::new(roxide_host::NativeHost::new()), GcPolicy::Torture).unwrap();
    roxide_base::register_all(&mut ctx).unwrap();
    roxide_stats::register(&mut ctx).unwrap();
    ctx.run_script(
        "Optimizer boundaries",
        r#"
error <- function(expr) tryCatch(expr, error = function(e) conditionMessage(e))
warnings <- character()
root <- withCallingHandlers(
    uniroot(function(x) if (x == .5) NaN else x-.5, c(0,1)),
    warning = function(w) {
        warnings <<- c(warnings, conditionMessage(w))
        invokeRestart("muffleWarning")
    })
stopifnot(abs(root$root - .4998779445886305) < 1e-7)
stopifnot(identical(warnings, "NA/NaN replaced by maximum positive value"))
stopifnot(identical(error(uniroot(function(x) x-2,c(0,3),check.conv=NA)),
                    "missing value where TRUE/FALSE needed"))
quiet <- function(expr) withCallingHandlers(expr, warning = function(w) invokeRestart("muffleWarning"))
stopifnot(identical(quiet(error(uniroot(function(x) x-2,c(0,3),maxiter=Inf))),
                    "'maxiter' must be positive"))
root <- quiet(uniroot(function(x) log(x)-2,c(0,1),extendInt="yes"))
stopifnot(abs(root$root-exp(2)) < 1e-6)
stopifnot(identical(error(optim(1,function(x) stop("objective ran"), hessian=NA)),
                    "objective ran"))
for (method in c("CG", "L-BFGS-B")) {
    fit <- optim(1,function(x) x^2,function(x) NaN,method=method)
    stopifnot(identical(fit$par,1), identical(fit$convergence,0L),
              identical(unname(fit$counts),c(1L,1L)))
}
"#,
        GLOBAL_ENV,
        false,
    )
    .unwrap();
}

#[test]
fn annealing_saves_only_after_success_and_observes_nested_draws() {
    let mut ctx = Ctx::new(Box::new(roxide_host::NativeHost::new()), GcPolicy::Torture).unwrap();
    roxide_base::register_all(&mut ctx).unwrap();
    roxide_stats::register(&mut ctx).unwrap();
    ctx.run_script("Annealing seed", r#"
error <- function(expr) tryCatch(expr, error = function(e) conditionMessage(e))
exists.seed <- function() exists(".Random.seed", envir=.GlobalEnv, inherits=FALSE)
stopifnot(!exists.seed())
control <- list(maxit=1L)
stopifnot(identical(error(optim(1,function(x) stop("callback"),method="SANN",control=control)),
                    "callback"), !exists.seed())
invisible(optim(1,function(x) {stopifnot(!exists.seed()); x^2},method="SANN",control=control))
stopifnot(exists.seed())
set.seed(11)
old <- .Random.seed
calls <- 0L
result <- error(optim(1,function(x) {
    calls <<- calls + 1L
    if (calls == 2L) stop("callback")
    x^2
}, method="SANN", control=list(maxit=4L)))
stopifnot(identical(result, "callback"), identical(old, .Random.seed))

set.seed(11)
expected <- runif(4)
after <- .Random.seed
set.seed(11)
draws <- numeric()
points <- numeric()
calls <- 0L
invisible(optim(1,function(x) {
    calls <<- calls + 1L
    draws <<- c(draws, runif(1))
    points <<- c(points,x)
    -calls
}, method="SANN", control=list(maxit=4L)))
stopifnot(identical(draws,expected), identical(after,.Random.seed))
stopifnot(max(abs(points - c(1,-2.2803941187835086,-2.2537997494889117,-4.4497457184630891))) < 1e-13)
"#, GLOBAL_ENV, false).unwrap();
}
