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

#[test]
fn beta_and_noncentral_draws_match_oracle_bit_for_bit() {
    use roxide_value::ValueApi;
    use std::fmt::Write as _;
    use std::io::Write;
    use std::process::{Command, Stdio};

    let Some(oracle) = std::env::var_os("ROXIDE_ORACLE_RSCRIPT") else {
        return;
    };
    let cases = [
        "rbeta(100,2,3)",
        "rbeta(100,.5,.5)",
        "rbeta(100,1.01,1.02)",
        "rbeta(100,3,2)",
        "rbeta(100,.2,4)",
        "rbeta(100,4,.2)",
        "rbeta(100,1,1)",
        "rbeta(100,0,0)",
        "rbeta(100,0,1)",
        "rbeta(100,Inf,Inf)",
        "rbeta(100,3,Inf)",
        "rbeta(100,-1,3)",
        "rchisq(100,3,20)",
        "rchisq(100,3,0)",
        "rchisq(100,0,0)",
        "rchisq(100,0,2)",
        "rchisq(100,Inf,0)",
        "rchisq(100,Inf,20)",
        "rchisq(100,3,.1)",
        "rchisq(100,3,100)",
        "rchisq(100,3,Inf)",
        "rchisq(100,c(0,3,Inf),c(0,2))",
        "rbeta(100,2,3,2)",
        "rbeta(100,2,3,0)",
        "rbeta(100,.5,.5,2)",
        "rbeta(100,0,1,2)",
        "rbeta(100,2,Inf,0)",
        "rbeta(100,c(.5,2),c(1,3,4),2)",
        "rf(100,3,5,2)",
        "rf(100,3,5,0)",
        "rf(100,1,1,20)",
        "rf(100,3,Inf,2)",
        "rf(100,Inf,5,2)",
        "rf(100,c(3,5),c(1,3,5),2)",
        "rt(100,3,2)",
        "rt(100,3,0)",
        "rt(100,Inf,2)",
        "rt(100,3,-2)",
        "rt(100,3,Inf)",
        "rt(100,c(3,5,Inf),c(-2,2))",
    ];
    let mut script = String::from("options(warn=-1)\n");
    let mut print = String::from("stopifnot(getRversion() == '4.6.2')\n");
    for (i, case) in cases.iter().enumerate() {
        writeln!(
            script,
            "set.seed(42L); draw{i} <- {case}; seed{i} <- .Random.seed"
        )
        .unwrap();
        writeln!(print, "cat(sprintf('%.17g',draw{i}), seed{i}, '\\n')").unwrap();
    }
    let mut child = Command::new(oracle)
        .args(["--vanilla", "/dev/stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Start the oracle");
    let mut input = child.stdin.take().expect("Open oracle input");
    let output = std::thread::scope(|scope| {
        let writer = scope.spawn(|| {
            input.write_all(script.as_bytes()).unwrap();
            input.write_all(print.as_bytes()).unwrap();
            drop(input);
        });
        let output = child.wait_with_output().unwrap();
        writer.join().unwrap();
        output
    });
    assert!(output.status.success());
    let output = String::from_utf8(output.stdout).unwrap();
    let mut lines = output.lines();

    let mut ctx = Ctx::new(Box::new(NativeHost::new()), GcPolicy::default()).unwrap();
    roxide_base::register_all(&mut ctx).unwrap();
    roxide_stats::register(&mut ctx).unwrap();
    ctx.run_script("Streams", &script, GLOBAL_ENV, false)
        .unwrap();
    let mut mismatches = Vec::new();
    for (i, case) in cases.iter().enumerate() {
        let value = |ctx: &mut Ctx, name| {
            let sym = ctx.session_mut().intern(&format!("{name}{i}")).unwrap();
            ctx.session().env_get(GLOBAL_ENV, sym).unwrap().value
        };
        let line = lines.next().unwrap();
        let mut expected = line.split_whitespace();
        let draw = value(&mut ctx, "draw");
        let draw: Vec<_> = ctx.session().real(draw).unwrap().iter().collect();
        assert_eq!(draw.len(), 100, "{case}");
        for (j, actual) in draw.into_iter().enumerate() {
            let expected = expected.next().unwrap().parse::<f64>().unwrap();
            if actual.to_bits() != expected.to_bits() && !(actual.is_nan() && expected.is_nan()) {
                mismatches.push(format!(
                    "{case}, draw {j}: {actual:.17e} != {expected:.17e}"
                ));
                break;
            }
        }
        let seed = value(&mut ctx, "seed");
        let seed = ctx.session().int(seed).unwrap().to_vec();
        let expected: Vec<i32> = line
            .split_whitespace()
            .skip(100)
            .map(|word| word.parse().unwrap())
            .collect();
        assert_eq!(seed.len(), 626, "{case}");
        assert_eq!(expected.len(), 626, "{case}");
        if seed != expected {
            mismatches.push(format!(
                "{case}, seed cursor: {} != {}",
                seed[1], expected[1]
            ));
        }
    }
    assert!(lines.next().is_none());
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[test]
fn distribution_warnings_and_parameter_validation() {
    check(
        Box::new(PureHost::new()),
        "Boundary warnings",
        r#"
warnings <- character()
record <- function(expr) withCallingHandlers(expr, warning=function(w) {
    warnings <<- c(warnings, conditionMessage(w))
    invokeRestart("muffleWarning")
})
stopifnot(is.nan(record(pbinom(1,2.4,.5))))
stopifnot(identical(warnings,c("non-integer n = 2.400000","NaNs produced")))
warnings <- character()
stopifnot(is.finite(record(dgamma(1,2,rate=49,scale=1/49))))
stopifnot(identical(warnings,"specify 'rate' or 'scale' but not both"))
warnings <- character()
stopifnot(identical(record(dhyper(-1.5,10,10,10)),0), length(warnings)==0L)
message <- tryCatch(rmultinom(1,5,c(1e308,1e308)), error=function(e) conditionMessage(e))
stopifnot(identical(message, "rbinom: probability sum should be 1, but is 0"))
"#,
    );
}

#[test]
fn descriptive_order_statistics_preserve_date_class() {
    let mut ctx = roxide_eval::Ctx::new(
        Box::new(roxide_host::PureHost::new()),
        roxide_gc::GcPolicy::Torture,
    )
    .expect("Create the R session");
    roxide_base::register_all(&mut ctx).expect("Register base");
    roxide_stats::register(&mut ctx).expect("Register stats");
    ctx.run_script(
        "Classed order statistics",
        r#"
x <- as.Date("2020-01-01") + 0:3
stopifnot(inherits(median(x), "Date"), as.double(median(x)) == 18263.5)
err <- tryCatch(quantile(x, 0.5), error = conditionMessage)
stopifnot(identical(err, '* not defined for "Date" objects'))
stopifnot(inherits(quantile(x, .5, type = 1), "Date"))
stopifnot(mad(1:3, low = TRUE, high = TRUE) == 1.4826)
err <- tryCatch(mad(1:4, low = TRUE, high = TRUE), error = conditionMessage)
stopifnot(identical(err, "'low' and 'high' cannot be both TRUE"))
err <- tryCatch(fivenum(c("a", "b")), error = conditionMessage)
stopifnot(identical(err, "non-numeric argument to binary operator"))
err <- tryCatch(weighted.mean(c("1", "2"), 1:2), error = conditionMessage)
stopifnot(identical(err, "non-numeric argument to binary operator"),
          identical(weighted.mean(1:2, c(1, NaN)), NA_real_))
"#,
        roxide_value::GLOBAL_ENV,
        false,
    )
    .expect("Check descriptive order statistics");
}
