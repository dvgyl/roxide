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
