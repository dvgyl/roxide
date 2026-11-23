//! Test generalized linear models and garbage collection during family callbacks.

#![forbid(unsafe_code)]

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::PureHost;
use roxide_value::{GLOBAL_ENV, ValueApi};

fn check(script: &str, policy: GcPolicy) {
    let mut ctx = Ctx::new(Box::new(PureHost::new()), policy).expect("Create session");
    ctx.set_stack_budget(1024 * 1024);
    roxide_base::register_all(&mut ctx).expect("Register base");
    roxide_stats::register(&mut ctx).expect("Register stats");
    let script = format!(
        "tryCatch({{\n{script}\n}}, error = function(e) .glm_test_error <<- conditionMessage(e))"
    );
    ctx.run_script("GLM parity", &script, GLOBAL_ENV, false)
        .expect("Run GLM checks");
    let error = ctx
        .session_mut()
        .intern(".glm_test_error")
        .expect("Intern test error name");
    if let Some(binding) = ctx.session().env_get(GLOBAL_ENV, error) {
        let messages = ctx
            .session()
            .str_slice(binding.value)
            .expect("Read test error");
        let message = ctx
            .session()
            .char_bytes(messages[0])
            .expect("Read test error text");
        panic!("{}", String::from_utf8_lossy(message));
    }
}

#[test]
fn family_callbacks_keep_model_values_live() {
    check(
        r#"
f <- poisson()
f$initialize <- quote({
    n <- rep(1, nobs)
    mustart <- as.double(c(y)) + 0.1
})
variance <- f$variance
deviance <- f$dev.resids
link <- f$linkfun
inverse <- f$linkinv
f$variance <- function(mu) variance(c(mu))
f$dev.resids <- function(y, mu, wt) deviance(c(y), c(mu), c(wt))
f$linkfun <- function(mu) link(c(mu))
f$linkinv <- function(eta) inverse(c(eta))
d <- data.frame(x=-2:3,n=c(1,0,2,3,2,5))
g <- glm(n~x,f,d)
stopifnot(g$converged, g$iter == 5L)
stopifnot(max(abs(coef(g) - c(.369106005772807,.387957314465342))) < 1e-11)
stopifnot(abs(g$deviance - 3.11618157212258) < 1e-11)
"#,
        GcPolicy::Torture,
    );
}

#[test]
fn irls_halves_a_step_that_gives_invalid_means() {
    check(
        r#"
x <- cbind(i=1,x=-2:3)
y <- c(1.5,.5,2.5,3.5,2.5,5.5)
seen <- character()
g <- withCallingHandlers(glm.fit(x,y,start=c(1,-.49/3),family=Gamma()),
    warning=function(w) {
        seen <<- c(seen, conditionMessage(w))
        invokeRestart("muffleWarning")
    })
stopifnot(any(seen == "step size truncated due to divergence"))
stopifnot(g$converged, !g$boundary, g$iter == 7L)
stopifnot(abs(g$deviance - 1.246982) < 1e-6, all(g$fitted.values > 0))
"#,
        GcPolicy::default(),
    );
}

#[test]
fn binomial_trials_and_rank_deficient_design() {
    check(
        r#"
d <- data.frame(x=-2:3,z=2*(-2:3),a=c(1,0,2,3,2,5),b=c(1,0,1,2,1,2),
                w=c(1,2,1,3,1,2))
g <- glm(cbind(a,b)~z+x,binomial(),d,weights=w)
stopifnot(identical(class(g), c("glm","lm")), g$converged, g$rank == 2L)
covariance <- vcov(g)
stopifnot(is.na(coef(g)[3L]), all(is.na(covariance[3L, ])))
stopifnot(all(g$prior.weights == c(2,0,3,15,3,14)), g$y[2L] == 0)
stopifnot(g$df.residual == 3L, nrow(g$qr$qr) == 5L)
prediction <- predict(g, type="response", se.fit=TRUE)
stopifnot(all(is.finite(prediction$se.fit)))
"#,
        GcPolicy::default(),
    );
}

#[test]
fn audit_cases_match_oracle() {
    let script = r#"
x <- matrix(1, 6, 1)
warnings <- character()
fit <- withCallingHandlers(
    glm.fit(x, 1:6, start=-1, family=gaussian("log"), control=list(maxit=1)),
    warning=function(w) {
        warnings <<- c(warnings, conditionMessage(w))
        invokeRestart("muffleWarning")
    })
stopifnot(abs(coef(fit) - (-1 + 3.5 * exp(1) - 1)) < 1e-12,
          !fit$converged, !fit$boundary,
          identical(warnings, "glm.fit: algorithm did not converge"))
stopifnot(glm.fit(x, 1:6, start=0, family=gaussian("log"))$iter == 6)
stopifnot(abs(poisson()$dev.resids(1e10, 1e10 + .01, 1) - 9.00198064306135e-7) < 1e-15,
          power(0)$linkinv(-1000) == 2.220446049250313e-16,
          power(0)$mu.eta(-1000) == 2.220446049250313e-16)

d <- data.frame(x=1:6, y=c(2,4,NA,6,7,8), z=c(1,4,2,6,7,8))
f <- poisson()
f$dispersion <- NA_real_
g <- glm(y ~ x, f, d, na.action=na.exclude)
near <- function(a, b) stopifnot(length(a) == length(b), max(abs(a - b)) < 1e-8)
keep <- c(1,2,4,5,6)
r <- rstandard(g)
cooks <- cooks.distance(g)
stopifnot(length(r) == 6L, length(cooks) == 6L, is.na(r[3]), is.na(cooks[3]))
near(r[keep], c(-.617159026,.390847066,.276702303,.077548545,-.334057060))
near(cooks[keep], c(.187927933,.045052191,.010107549,.001244138,.095321216))
stopifnot(summary(g)$dispersion == 1, colnames(coef(summary(g)))[3] == "z value")
single <- anova(g)
other <- glm(z ~ x, f, d)
seen <- character()
sequential <- withCallingHandlers(anova(g, other), warning=function(w) {
    seen <<- c(seen, conditionMessage(w))
    invokeRestart("muffleWarning")
})
stopifnot(identical(sequential, single), length(seen) == 1L, any(names(single) == "Pr(>F)"))
near(single[["Pr(>F)"]][2], .12671209104732115)
near(predict(g)[keep], predict(g, newdata=NULL))
stopifnot(is.na(predict(g)[3]))
for (type in c("link", "response", "terms")) {
    padded <- predict(g, type=type, se.fit=TRUE)
    explicit <- predict(g, newdata=NULL, type=type, se.fit=TRUE)
    stopifnot(length(padded$fit) == 6L, length(explicit$fit) == 5L)
    near(as.double(padded$fit)[keep], as.double(explicit$fit))
    near(as.double(padded$se.fit)[keep], as.double(explicit$se.fit))
}
g <- glm(y ~ x, f, data=d, model=FALSE)
d <- NULL
p <- predict(g, newdata=data.frame(x=c(2,4)), type="response", se.fit=TRUE)
near(p$fit, exp(c(1.2314601967277752, 1.6913265102577864)))
near(p$se.fit, exp(c(1.2314601967277752, 1.6913265102577864)) *
               c(.32906491297379270, .19619136203219301))
"#;
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
    check(script, GcPolicy::default());
}
