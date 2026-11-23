//! Exercise distribution and descriptive bindings through the evaluator.

#![forbid(unsafe_code)]

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::{Host, Limits, NativeHost, PureHost};
use roxide_value::GLOBAL_ENV;

fn check(host: Box<dyn Host>, name: &str, script: &str) {
    let mut ctx = Ctx::new(host, GcPolicy::Torture).expect("Create the evaluator");
    roxide_base::register_all(&mut ctx).expect("Register base");
    roxide_stats::register(&mut ctx).expect("Register stats");
    ctx.run_script(name, script, GLOBAL_ENV, false)
        .expect("Run the parity checks");
}

#[test]
fn distribution_attributes_missing_values_and_seed() {
    check(
        Box::new(PureHost::new()),
        "Distributions",
        r#"
stopifnot(identical(environment(runif), environment(stats::dnorm)))
x <- matrix(c(NA_real_, NaN, 0, 1), nrow = 2L,
            dimnames = list(c("a", "b"), c("c", "d")))
p <- dnorm(x, c(NaN, NA_real_, 0, 0), sd = c(-1, -1, 1, 1))
stopifnot(identical(dimnames(p), dimnames(x)), identical(p[1:2], c(NA_real_, NA_real_)))
set.seed(42L)
seed <- .Random.seed
stopifnot(identical(rbinom(0L, 1L, 0.5), integer(0)), identical(seed, .Random.seed))
x <- rmultinom(4L, 10L, c(a = 1, b = 2))
stopifnot(identical(dim(x), c(2L, 4L)), identical(dimnames(x)[[1L]], c("a", "b")))
"#,
    );
}

#[test]
fn descriptive_dispatch_and_complete_rows() {
    check(
        Box::new(PureHost::new()),
        "Descriptive statistics",
        r#"
quantile.example <- function(x, ...) "custom"
stopifnot(identical(quantile(structure(1:3, class = "example")), "custom"))
q <- quantile(1:5, c(1/3, 0.999999, NA_real_, NaN))
stopifnot(identical(names(q), c("33.33333%", "99.9999%", "", "")))
stopifnot(identical(unname(q[3:4]), c(NA_real_, NaN)))
x <- data.frame(a = c(1, 2, NA, 4), b = c(1, 3, 4, 2))
stopifnot(identical(dimnames(cor(x)), list(c("a", "b"), c("a", "b"))))
stopifnot(identical(cor(x, use = "pairwise.complete.obs", method = "kendall")[1, 1], 1))
stopifnot(identical(cov(1:4, 4:1, method = "kendall"), -12))
stopifnot(identical(weighted.mean(c(Inf, 7), c(0, 1)), 7))
"#,
    );
}

#[test]
fn vector_limits_apply_before_coercion_and_rng() {
    let host = NativeHost::with_limits(Limits {
        max_vector_bytes: 128,
        ..Limits::default()
    });
    check(
        Box::new(host),
        "Vector size limits",
        r#"
error <- function(expr) tryCatch(expr, error = function(e) conditionMessage(e))
message <- "vector size limit exceeded"
stopifnot(identical(error(dnorm(1:17)), message))
stopifnot(identical(error(runif(17L)), message))
stopifnot(identical(error(quantile(1:17)), message))
stopifnot(identical(error(cor(1:17, 1:17)), message))
stopifnot(identical(error(rmultinom(17L, 2L, c(1, 1))), message))
stopifnot(!exists(".Random.seed", envir = .GlobalEnv, inherits = FALSE))
"#,
    );
}
