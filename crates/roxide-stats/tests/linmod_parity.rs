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

#[test]
fn multivariate_model_comparisons_and_influence() {
    check(
        r#"
x <- 1:6
y <- c(2,4,5,4,9,8)
z <- c(1,4,2,8,2,11)
m <- lm(cbind(y,z) ~ x)
a <- anova(m)
b <- anova(lm(cbind(y,z) ~ 1), m)
stopifnot(identical(rownames(a), c("(Intercept)", "x", "Residuals")))
stopifnot(identical(names(b), c("Res.Df", "Df", "Gen.var.", "Pillai", "approx F", "num Df", "den Df", "Pr(>F)")))
stopifnot(abs(a$Pillai[1] - .990987975157466) < 1e-11, abs(b$Pillai[2] - .953703424347564) < 1e-11)
stopifnot(abs(b$Gen.var.[2] - 2.70317273904996) < 1e-11)
ci <- confint(m)
stopifnot(identical(rownames(ci), c("y:(Intercept)", "y:x", "z:(Intercept)", "z:x")))
stopifnot(max(abs(ci[,1] - c(-2.64728485594513, .33772498287657, -8.8687206188474, -.763113699316563))) < 1e-10)
fit <- lm(z ~ x)
for (f in list(rstandard, rstudent, cooks.distance)) {
    stopifnot(max(abs(f(m)[, 2] - f(fit))) < 1e-10)
}
stopifnot(all(lm(y ~ 0, offset=z)$fitted.values == z), df.residual(lm(cbind(y,z) ~ 0)) == 6L)
fit <- lm.wfit(matrix(numeric(),6,0), cbind(y,z), c(1,0,1,1,1,1), offset=z)
stopifnot(identical(dim(fit$residuals), c(5L,2L)), all(fit$weights == 1))
msg <- function(expr) tryCatch(expr, error=conditionMessage)
stopifnot(identical(msg(vcov(lm(cbind(y,z) ~ x, weights=x))), "'mlm' objects with weights are not supported"))
stopifnot(identical(msg(printCoefmat(matrix(1:6,3), P.values=TRUE)), "'P.values' is TRUE, but 'has.Pvalue' is not"))
print(summary(lm(y ~ x), correlation=TRUE, symbolic.cor=TRUE))
y[3] <- NA
m <- lm(y ~ x, weights=c(1,0,1,2,1,1), na.action=na.exclude)
d <- cooks.distance(m)
stopifnot(identical(names(d), c("1", NA, "4", "5", "6")))
stopifnot(is.na(d[2]), max(abs(d[-2] - c(1.63446995823876, 0, .2732292178974, .00254781843046891))) < 1e-10)
m <- lm(cbind(y,z) ~ x, weights=c(1,0,1,2,1,1), na.action=na.exclude)
stopifnot(max(abs(sigma(m) - c(2.14261903439006, 4.44409720865779))) < 1e-10)
stopifnot(max(abs(rstandard(m)[1,] - c(.790510306630321, -.283817474855534))) < 1e-10)
stopifnot(max(abs(cooks.distance(m)[1,] - c(1.63446995823876, .200777831793875))) < 1e-10)
"#,
    );
}
