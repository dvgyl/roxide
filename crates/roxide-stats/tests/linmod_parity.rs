//! Test linear models with garbage collection at each safepoint.

#![forbid(unsafe_code)]

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::PureHost;
use roxide_value::GLOBAL_ENV;

fn check(script: &str) {
    let mut ctx = Ctx::new(Box::new(PureHost::new()), GcPolicy::Torture).expect("Create session");
    roxide_base::register_all(&mut ctx).expect("Register base");
    roxide_stats::register(&mut ctx).expect("Register stats");
    ctx.run_script("Linear model parity", script, GLOBAL_ENV, false)
        .expect("Run linear model checks");
}

#[test]
fn pivoted_fit_and_summary() {
    check(
        r#"
d <- data.frame(y = c(3,1,6,2,8,4,9,5), a = 0:7, b = 2*(0:7),
                f = factor(rep(c("lo","hi"), 4), levels = c("lo","hi")))
m <- lm(y ~ a + b + f, d)
stopifnot(identical(m$qr$pivot, c(1L,2L,4L,3L)), is.na(coef(m)[[3L]]))
stopifnot(identical(colnames(m$qr$qr), c("(Intercept)", "a", "fhi", "b")))
s <- summary(m)
stopifnot(identical(s$df, c(3L, 5L, 4L)), abs(s$sigma - sqrt(0.42)) < 1e-12)
stopifnot(identical(rownames(anova(m)), c("a", "f", "Residuals")))
stopifnot(nrow(vcov(m)) == 4L, all(is.na(vcov(m)[3L, ])))
"#,
    );
}

#[test]
fn zero_weights_and_multiple_responses() {
    check(
        r#"
X <- cbind(i = 1, a = 1:4, b = 2*(1:4))
Y <- cbind(m = c(4,1,3,2), n = c(8,2,6,4))
fit <- lm.wfit(X, Y, c(1,0,2,1), offset = 1)
stopifnot(identical(dim(fit$residuals), c(4L, 2L)), is.na(fit$coefficients[3,1]))
stopifnot(nrow(fit$effects) == 3L, fit$df.residual == 1L)
stopifnot(max(abs(Y - fit$fitted.values - fit$residuals)) < 1e-12)
m <- lm(Y ~ X[,2L])
stopifnot(identical(class(m), c("mlm", "lm")), all(dim(predict(m)) == dim(Y)))
"#,
    );
}

#[test]
fn prediction_reuses_the_polynomial_basis() {
    check(
        r#"
d <- data.frame(y = c(5,1,4,8,2,9), x = c(1,3,4,8,9,20),
                f = factor(c("a","b","a","b","a","b")))
m <- lm(y ~ poly(x, 2) + f, d, model = FALSE)
p <- predict(m, d[c(2,5), ], se.fit = TRUE, interval = "confidence")
stopifnot(max(abs(p$fit[,1] - fitted(m)[c(2,5)])) < 1e-11)
stopifnot(all(p$se.fit > 0), all(p$fit[,2] < p$fit[,3]))
"#,
    );
}

#[test]
fn zero_columns_and_na_exclusion() {
    check(
        r#"
d <- data.frame(y = c(4,NA,3,2,7), x = 1:5)
m <- lm(y ~ 0, d, na.action = na.exclude)
stopifnot(length(coef(m)) == 0, is.na(predict(m)[2]), predict(m)[3] == 0)
stopifnot(identical(dim(summary(m)$coefficients), c(0L,4L)))
stopifnot(identical(rownames(anova(m)), "Residuals"))
m <- lm(y ~ x, d, na.action = na.exclude)
stopifnot(length(residuals(m)) == 5L, length(hatvalues(m)) == 5L)
stopifnot(hatvalues(m)[2L] == 0, is.na(cooks.distance(m)[2L]))
"#,
    );
}
