//! Test model registration and evaluation with garbage collection at each safepoint.

#![forbid(unsafe_code)]

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::PureHost;
use roxide_value::GLOBAL_ENV;

fn check(script: &str) {
    check_with_policy(script, GcPolicy::Torture);
}

fn check_with_policy(script: &str, policy: GcPolicy) {
    let mut ctx = Ctx::new(Box::new(PureHost::new()), policy).expect("Create session");
    roxide_base::register_all(&mut ctx).expect("Register base");
    roxide_stats::register(&mut ctx).expect("Register stats");
    ctx.run_script("Model parity", script, GLOBAL_ENV, false)
        .expect("Run model checks");
}

#[test]
fn formula_environment_and_prediction_calls() {
    check(
        r#"
f <- local({
    k <- 10
    y ~ poly(x + k, 2)
})
d <- data.frame(y = 1:5, x = 1:5)
m <- model.frame(f, d, subset = x < 4)
tt <- terms(m)
stopifnot(identical(environment(tt), environment(f)))
stopifnot(identical(nrow(m), 3L))
new <- model.frame(tt, data.frame(y = 1:2, x = c(2L, 3L)))
stopifnot(max(abs(new[[2L]] - m[[2L]][2:3, ])) < 1e-12)
f <- update(f, log(.) ~ . - 1)
stopifnot(identical(attr(terms(f), "intercept"), 0L))
p <- poly(1:5, 2)
stopifnot(max(abs(predict(p, 2:4) - p[2:4, ])) < 1e-12)
mm <- model.matrix(~poly(x, 1), d)
stopifnot(identical(colnames(mm), c("(Intercept)", "poly(x, 1)")))
"#,
    );
}

#[test]
fn custom_methods_and_matrix_coding() {
    check(
        r#"
makepredictcall.probe <- function(var, call) quote(x)
seen <- new.env()
d <- data.frame(f = factor(c("c", "a", "b", "c")), y = c(2, NA, 6, 8), x = 11:14)
omit <- function(object) {
    seen$terms <- terms(object)
    na.exclude(object)
}
m <- model.frame(y ~ f + structure(x, class = "probe"), d, na.action = omit)
stopifnot(inherits(seen$terms, "terms"))
stopifnot(identical(as.list(attr(terms(m), "predvars"))[[4L]], quote(x)))
stopifnot(identical(as.integer(na.action(m)), 2L))
mm <- model.matrix(y ~ 0+f, d, na.action = na.exclude)
stopifnot(identical(colnames(mm), c("fa", "fb", "fc")))
stopifnot(identical(as.double(mm[1L, ]), c(0, 0, 1)))
"#,
    );
}

#[test]
fn terms_handles_long_chains_and_integer_powers() {
    let script = r#"
t <- terms(reformulate(paste0("x", 1:200), "y"))
stopifnot(length(labels(t)) == 200L, identical(labels(t), labels(terms(formula(t)))))
t <- terms(reformulate(paste0("x", 1:5000), "y"))
stopifnot(length(labels(t)) == 5000L, identical(labels(t), labels(terms(formula(t)))))
t <- terms(formula(paste0("~(", paste(paste0("x", 1:13), collapse = "+"), ")^13")))
stopifnot(length(labels(t)) == 8191L)
t <- terms(y ~ (a + b)^2L)
stopifnot(identical(labels(t), c("a", "b", "a:b")),
          identical(labels(t[c(2L,1L)]), c("b", "a")),
          !is.empty.model(terms(y ~ 1)), is.empty.model(terms(y ~ 0)))
f <- factor(c("a", "b", "c"))
stopifnot(identical(attr(C(f, sum), "contrasts"), "contr.sum"),
          ncol(attr(C(f, treatment, 1), "contrasts")) == 1L)
d <- data.frame(y = 1:3, f = factor(c("a", "c", "a")))
stopifnot(nrow(model.frame(y ~ f, d, subset = f != "c")) == 2L)
"#;
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || check_with_policy(script, GcPolicy::default()))
        .expect("Start a thread with the command-line interpreter stack size")
        .join()
        .expect("Run the long model checks");
}
