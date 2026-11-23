//! Check Fourier and time-series allocation through the public R functions.

#![forbid(unsafe_code)]

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::{Host, Limits, NativeHost, PureHost};
use roxide_value::GLOBAL_ENV;

fn run(host: Box<dyn Host>, script: &str) {
    let mut ctx = Ctx::new(host, GcPolicy::Torture).expect("Create the evaluator");
    roxide_base::register_all(&mut ctx).expect("Register base");
    roxide_stats::register(&mut ctx).expect("Register stats");
    ctx.run_script("Time series", script, GLOBAL_ENV, false)
        .expect("Run the checks");
}

#[test]
fn time_series_and_complex_outputs_survive_collection() {
    run(
        Box::new(PureHost::new()),
        r#"
x <- ts(matrix(1:12,6), start=c(2,2), frequency=3, names=c("a","b"))
stopifnot(identical(dimnames(fft(x)), dimnames(x)))
stopifnot(identical(tsp(fft(x)), tsp(x)))
stopifnot(identical(dimnames(mvfft(x)), dimnames(x)))
stopifnot(is.mts(window(x,start=3)), is.mts(filter(x,1:2)))
y <- x
tsp(y) <- NULL
stopifnot(is.ts(x), !is.ts(y))
z <- acf(x,2,plot=FALSE)
stopifnot(identical(dim(z$acf),c(3L,2L,2L)))
shown <- withVisible(acf(x,2,plot=TRUE))
stopifnot(!shown$visible, identical(shown$value,z))
stopifnot(inherits(Box.test(1:8),"htest"))
a <- matrix(c(3,1+2i,1-2i,5),2)
stopifnot(is.double(eigen(a)$values), is.complex(svd(a)$u))
"#,
    );
}

#[test]
fn scratch_space_and_extended_windows_use_host_limits() {
    run(
        Box::new(NativeHost::with_limits(Limits {
            max_vector_bytes: 512,
            ..Limits::default()
        })),
        r#"
message <- function(expr) tryCatch(expr, error=function(e) conditionMessage(e))
limit <- "vector size limit exceeded"
stopifnot(identical(message(fft(1:17)),limit))
stopifnot(identical(message(mvfft(matrix(1:17,17))),limit))
stopifnot(identical(message(window(ts(1:8),end=2000,extend=TRUE)),limit))
stopifnot(identical(message(ts(1:8,start=1,end=2000)),limit))
stopifnot(identical(message(acf(matrix(1:40,5),4,plot=FALSE)),limit))
"#,
    );
}

#[test]
fn fft_nonfinite_components_and_time_series_tolerances() {
    run(
        Box::new(PureHost::new()),
        r#"
z <- fft(c(1e308, rep(0,6)))
stopifnot(all(Re(z) == 1e308), all(Im(z) == 0))
for (a in c(NA_real_, Inf)) {
    z <- fft(c(a, rep(0,6)))
    stopifnot(all(Im(z) == 0), identical(Re(z), rep(a, 7)))
}
stopifnot(identical(tsp(ts(1:3, start=1, end=3.005, ts.eps=.01)), c(1,3.005,1)))
msg <- tryCatch(ccf(ts(1:5), ts(2:6, start=1.5), plot=FALSE), error=conditionMessage)
stopifnot(identical(msg, "not all series have the same phase"))
warning <- ""
n <- withCallingHandlers(nextn(2^54), warning=function(w) {
    warning <<- conditionMessage(w)
    invokeRestart("muffleWarning")
})
stopifnot(n == 2^54, grepl("> 2^53", warning, fixed=TRUE))
"#,
    );
}
