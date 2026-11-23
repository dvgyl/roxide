//! Check DCF I/O with an in-memory host and garbage collection stress.

#![forbid(unsafe_code)]

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::PureHost;
use roxide_value::GLOBAL_ENV;

#[test]
fn dcf_round_trips_repeated_fields_and_missing_values() {
    let mut ctx =
        Ctx::new(Box::new(PureHost::new()), GcPolicy::Torture).expect("Create the R session");
    roxide_base::register_all(&mut ctx).expect("Register base");
    ctx.run_script(
        "<dcf-parity>",
        r#"
src <- textConnection(c("A: café", "A: deux", "", "B: y"))
d <- read.dcf(src, all=TRUE)
stopifnot(identical(d$A, list(c("café","deux"),NA_character_)))
close(src)
path <- tempfile()
write.dcf(d, path)
src <- file(path)
stopifnot(!isOpen(src), identical(read.dcf(src, all=TRUE), d), !isOpen(src))
close(src)
unlink(path)
"#,
        GLOBAL_ENV,
        false,
    )
    .expect("Check the DCF round trip");
}

#[path = "support/failing_host.rs"]
mod failing_host;

#[test]
fn write_dcf_preserves_the_write_error_across_close() {
    let mut ctx = Ctx::new(
        Box::<failing_host::FailingHost>::default(),
        GcPolicy::Torture,
    )
    .expect("Create the R session");
    roxide_base::register_all(&mut ctx).expect("Register base");
    ctx.run_script(
        "DCF write failure",
        r#"
error <- tryCatch(write.dcf(list(A = "value"), "failing.txt", append = TRUE),
                  error = function(e) e)
stopifnot(inherits(error, "error"),
          identical(conditionMessage(error), "injected file write failure"))
"#,
        GLOBAL_ENV,
        false,
    )
    .expect("Check the condition after close");
}

#[test]
fn read_dcf_removes_nuls_in_all_mode() {
    let mut ctx =
        Ctx::new(Box::new(PureHost::new()), GcPolicy::Torture).expect("Create the R session");
    roxide_base::register_all(&mut ctx).expect("Register base");
    ctx.run_script(
        "DCF embedded NUL",
        r#"
p <- tempfile()
writeBin(as.raw(c(65, 58, 32, 120, 0, 121, 10, 66, 58, 32, 122, 10)), p)
stopifnot(identical(read.dcf(p)[[1L]], "x"))
out <- read.dcf(p, fields = c("A", "A", "B"), all = TRUE)
stopifnot(identical(out[[1L]], "xy"), identical(out[[2L]], "xy"),
          identical(out[[3L]], "z"))
"#,
        GLOBAL_ENV,
        false,
    )
    .expect("Check NUL handling and repeated field selection");
}

#[test]
fn dcf_honors_native_encoding_and_checks_output_size() {
    let host = failing_host::FailingHost::with_limit(4096, false);
    let mut ctx = Ctx::new(Box::new(host), GcPolicy::Torture).expect("Create the R session");
    roxide_base::register_all(&mut ctx).expect("Register base");
    ctx.run_script(
        "DCF locale and size limits",
        r#"
x <- rawToChar(as.raw(c(99, 97, 102, 195, 169)))
Encoding(x) <- "UTF-8"
p <- tempfile()
write.dcf(list(A = x), p)
stopifnot(identical(readLines(p), "A: caf<U+00E9>"))
write.dcf(list(A = x), p, useBytes = TRUE)
stopifnot(identical(readBin(p, "raw", 9L), as.raw(c(65,58,32,99,97,102,195,169,10))))
writeLines(rep("A: value", 40L), p)
err <- tryCatch(read.dcf(p, fields = rep("A", 25L), all = TRUE), error = conditionMessage)
stopifnot(identical(err, "vector size limit exceeded"))
x <- matrix(1:200, 10L, dimnames = list(NULL, paste0("x", 1:20)))
err <- tryCatch(write.dcf(x), error = conditionMessage)
stopifnot(identical(err, "vector size limit exceeded"))
"#,
        GLOBAL_ENV,
        false,
    )
    .expect("Check DCF locale and size limits");
}
