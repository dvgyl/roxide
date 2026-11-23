//! Test generalized linear models and garbage collection during family callbacks.

#![forbid(unsafe_code)]

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::PureHost;
use roxide_value::{GLOBAL_ENV, ValueApi};

fn check(script: &str, policy: GcPolicy) {
    let mut ctx = Ctx::new(Box::new(PureHost::new()), policy).expect("Create session");
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
