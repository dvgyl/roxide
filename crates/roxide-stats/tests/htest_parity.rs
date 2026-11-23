//! Check test results and random state under garbage collection stress.

#![forbid(unsafe_code)]

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::PureHost;
use roxide_value::GLOBAL_ENV;

fn check(script: &str) {
    let mut ctx =
        Ctx::new(Box::new(PureHost::new()), GcPolicy::Torture).expect("Create the evaluator");
    roxide_base::register_all(&mut ctx).expect("Register base");
    roxide_stats::register(&mut ctx).expect("Register stats");
    ctx.run_script("Hypothesis tests", script, GLOBAL_ENV, false)
        .expect("Run the hypothesis tests");
}

#[test]
fn formula_and_distribution_callback() {
    check(
        r#"
d <- data.frame(z=c(1,4,6,9,12,2,3,7,8,10), g=rep(c("a","b"),each=5L))
x <- d$z[1:5]
y <- d$z[6:10]
v <- t.test(z~g,d)
stopifnot(identical(v$data.name,"z by g"), abs(v$p.value - .87412283451082) < 1e-8)
v <- t.test(Pair(x,y)~1)
stopifnot(identical(v$method,"Paired t-test"), identical(attr(v$conf.int,"conf.level"),.95))
cdf <- function(x, a) punif(x,0,a)
v <- ks.test(x,cdf,a=15)
stopifnot(inherits(v,"ks.test"),inherits(v,"htest"),abs(v$p.value-.9616) < 1e-8)
"#,
    );
}

#[test]
fn simulation_preserves_random_stream() {
    check(
        r#"
set.seed(42)
invisible(runif(14))
next.draw <- runif(1)
set.seed(42)
x <- matrix(c(3,1,2,2,4,1),2)
v <- fisher.test(x,simulate.p.value=TRUE,B=7)
stopifnot(identical(runif(1),next.draw))
stopifnot(v$p.value == 1)
set.seed(42)
v <- chisq.test(matrix(c(8,2,1,5),2),simulate.p.value=TRUE,B=7)
stopifnot(v$p.value == .125, .Random.seed[2] == 7L)
"#,
    );
}

#[test]
fn bounded_exact_and_simulation_paths() {
    check(
        r#"
err <- function(expr) tryCatch(expr, error = function(e) conditionMessage(e))
stopifnot(identical(err(shapiro.test(1:5001)), "sample size must be between 3 and 5000"))
stopifnot(identical(err(fisher.test(matrix(rep(1e6,4),2))), "Fisher exact test support size limit exceeded"))
stopifnot(identical(err(p.adjust(c(.1,.2),method="hommel",n=20000)),
                    "Hommel adjustment size limit exceeded"))
stopifnot(identical(err(chisq.test(c(1,2,3),simulate.p.value=TRUE,B=1e8)),
                    "simulation size limit exceeded"))
stopifnot(identical(err(chisq.test(matrix(c(1.8,.1,1.8,.1),2),simulate.p.value=TRUE,B=1)),
                    "simulation requires positive, equal totals for integer margins"))
"#,
    );
}
