//! Check test results and random state under garbage collection stress.

#![forbid(unsafe_code)]

use std::process::Command;

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::{Host, Limits, NativeHost, PureHost};
use roxide_value::GLOBAL_ENV;

fn check(script: &str) {
    check_with_host(script, Box::new(PureHost::new()), GcPolicy::Torture);
}

fn check_with_host(script: &str, host: Box<dyn Host>, policy: GcPolicy) {
    let mut ctx = Ctx::new(host, policy).expect("Create the evaluator");
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
"#,
    );
}

fn check_oracle(script: &str) {
    if let Some(oracle) = std::env::var_os("ROXIDE_ORACLE_RSCRIPT") {
        let result = Command::new(oracle)
            .args([
                "--vanilla",
                "-e",
                &format!("setTimeLimit(elapsed=15);\n{script}"),
            ])
            .output()
            .expect("Run GNU R");
        assert!(
            result.status.success(),
            "GNU R: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    check_with_host(script, Box::new(PureHost::new()), GcPolicy::default());
}

#[test]
fn adjusted_p_values_and_confidence_level_validation() {
    check_oracle(
        r#"
err <- function(expr) tryCatch(expr, error=function(e) conditionMessage(e))
stopifnot(identical(p.adjust(c(a=.02,b=NA,c=.04)), c(a=.04,b=NA_real_,c=.04)))
stopifnot(identical(p.adjust(c(.01,NA,.02),n=2), c(.02,NA_real_,.02)))
stopifnot(identical(p.adjust(c(NA_real_,NaN)), c(NA_real_,NaN)))
message <- "'conf.level' must be a single number between 0 and 1"
stopifnot(identical(err(prop.test(2,10,conf.level=0)), message))
stopifnot(identical(err(var.test(1:3,2:6,conf.level=0)), message))
stopifnot(identical(err(fisher.test(matrix(1:4,2),conf.level=0)), message))
stopifnot(identical(err(binom.test(2,10,conf.level=1)), message))
stopifnot(identical(err(wilcox.test(1:8,conf.int=TRUE,conf.level=NaN)), message))
v <- prop.test(c(3,NA,4), c(10,15,14))
stopifnot(identical(names(v$estimate),c("prop 1","prop 3")))
"#,
    );
}

#[test]
fn rank_tails_and_confidence_intervals() {
    check_oracle(
        r#"
v <- wilcox.test(0:7, conf.int=TRUE)
stopifnot(identical(v$conf.int, structure(c(1.5,6), conf.level=.9609375)))
v <- wilcox.test(0:7, conf.int=TRUE, alternative="less")
stopifnot(identical(v$conf.int, structure(c(-Inf,5.5), conf.level=.95703125)))
v <- wilcox.test(0:7, conf.int=TRUE, conf.level=.9)
stopifnot(identical(v$conf.int, structure(c(2,5.5), conf.level=.91796875)))
v <- wilcox.test(c(1,1,3,4,8,8), conf.int=TRUE)
stopifnot(identical(v$conf.int, structure(c(1,8), conf.level=.984375)))
v <- wilcox.test(c(0,0,1,2,3,8), conf.int=TRUE)
stopifnot(identical(v$conf.int, structure(c(0,8), conf.level=1)))
v <- wilcox.test(c(-2,0,3,4,8,10), conf.int=TRUE)
stopifnot(identical(v$conf.int, structure(c(-1,10), conf.level=.984375)))
v <- wilcox.test(1:8, exact=FALSE, correct=3L)
stopifnot(identical(v$method,"Wilcoxon signed rank test with continuity correction"))
stopifnot(abs(v$p.value - .00789178807507795) < 1e-14)
v <- wilcox.test(1:8, 21:29, exact=FALSE, correct=3L)
stopifnot(abs(v$p.value - .0000469102664195618) < 1e-14)
v <- wilcox.test(1:3, exact=FALSE, conf.int=TRUE, conf.level=.999)
stopifnot(attr(v$conf.int,"conf.level") < .999)
v <- cor.test(1:5,c(1,2,3,5,4),method="spearman",exact=FALSE,continuity=TRUE)
stopifnot(abs(v$p.value - .0347733409403744) < 1e-14)
v <- cor.test(1:101,101:1,method="kendall",exact=TRUE)
stopifnot(identical(names(v$statistic),"T"), v$statistic==0)
stopifnot(abs(v$p.value / 2.1218025507425e-160 - 1) < 1e-12)
"#,
    );
}

#[test]
fn conditional_table_tails_and_random_stream() {
    check_oracle(
        r#"
near.mode <- matrix(c(10,9,10,9,9,10,10,9,10),3)
stopifnot(abs(fisher.test(near.mode)$p.value - 1) < 1e-14)
m <- matrix(c(5,4,3,2,4,4,1,1,2),3)
v <- fisher.test(m, hybrid=TRUE, hybridPars=c(expect=1,percent=0,Emin=0))
stopifnot(abs(v$p.value - .772741210917664) < 1e-12)
stopifnot(identical(v$method,
    "Fisher's Exact Test for Count Data hybrid using asym.chisq. iff (exp=1, perc=0, Emin=0)"))
stopifnot(abs(fisher.test(m,hybrid=TRUE)$p.value - .826416560545806) < 1e-12)
m <- matrix(c(6,10,5,6,8,11),2)
v <- fisher.test(m, hybrid=TRUE, hybridPars=c(1,0,0))
stopifnot(abs(v$p.value - .937652705349814) < 1e-12)
v <- fisher.test(matrix(c(2000,1,1,2000),2))
stopifnot(identical(unname(v$estimate),2^52), v$conf.int[[2L]] == 2^52)
err <- tryCatch(fisher.test(matrix(c(1,0,0,2,0,0,3,0,0),3),simulate.p.value=TRUE),
                error=function(e) conditionMessage(e))
stopifnot(identical(err, "need 2 or more non-zero row marginals"))
err <- tryCatch(fisher.test(matrix(0,3,3),simulate.p.value=TRUE),
                error=function(e) conditionMessage(e))
stopifnot(identical(err, "need 2 or more non-zero row marginals"))
for (x in list(c(1,4,5),c(0,5,5))) {
    set.seed(42)
    v <- chisq.test(x,p=c(0,.5,.5),simulate.p.value=TRUE,B=19)
    stopifnot(identical(v$p.value, NA_real_), .Random.seed[2L] == 190L)
    stopifnot(abs(runif(1) - .194676143815741) < 1e-15)
}
set.seed(42)
v <- chisq.test(matrix(c(1.8,.1,1.8,.1),2),simulate.p.value=TRUE,B=1)
stopifnot(v$p.value == 1, .Random.seed[2L] == 1L)
set.seed(42)
v <- chisq.test(matrix(1e-200,2,2),simulate.p.value=TRUE,B=1)
stopifnot(identical(v$p.value,NA_real_), .Random.seed[2L] == 624L)
"#,
    );
}

#[test]
fn callback_recycling_and_centered_shapiro_statistics() {
    check_oracle(
        r#"
v <- ks.test(c(.1,.3,.7,.9),function(q) c(.1,.8))
stopifnot(abs(v$statistic-.65) < 1e-14, abs(v$p.value-.0352125) < 1e-14)
err <- tryCatch(ks.test(c(1,1,2),function(q) stop("callback")),
                warning=function(w) conditionMessage(w),
                error=function(e) conditionMessage(e))
stopifnot(identical(err,"ties should not be present for the one-sample Kolmogorov-Smirnov test"))
v <- shapiro.test(1e16 + 2*(1:20))
stopifnot(abs(v$statistic-.958081191564426) < 1e-12)
stopifnot(abs(v$p.value-.506266041447420) < 1e-11)
"#,
    );
}

#[test]
fn internal_argument_and_print_limits() {
    check_with_host(
        r#"
err <- function(expr) tryCatch(expr, error=function(e) conditionMessage(e))
message <- "'conf.level' must be a single number between 0 and 1"
for (level in list(0,1,Inf,NaN,NA_real_,c(.9,.95))) {
    stopifnot(identical(err(.Internal(C_htest_wilcox(1:8,NULL,1,0,FALSE,FALSE,level,1e-4,Inf))), message))
}
stopifnot(identical(err(.Internal(C_htest_frame_call(list(a=1,b=2,c=3,d=4,e=5)))),
                    "invalid model frame arguments"))
stopifnot(identical(err(.Internal(C_htest_format(rep(1e-308,400),22,999))),
                    "vector size limit exceeded"))
stopifnot(identical(err(.Internal(C_htest_wrap(paste(rep("word",400),collapse=" "),
                                             paste(rep("x",200),collapse=""),10))),
                    "vector size limit exceeded"))
"#,
        Box::new(NativeHost::with_limits(Limits {
            max_vector_bytes: 32_768,
            ..Limits::default()
        })),
        GcPolicy::default(),
    );
}
