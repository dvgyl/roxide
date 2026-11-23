//! Check inspection and stream capture with garbage collection stress.

#![forbid(unsafe_code)]

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::PureHost;
use roxide_value::GLOBAL_ENV;

#[test]
fn capture_keeps_nested_streams_and_visibility() {
    let mut ctx =
        Ctx::new(Box::new(PureHost::new()), GcPolicy::Torture).expect("Create the R session");
    roxide_base::register_all(&mut ctx).expect("Register base");
    roxide_utils::register(&mut ctx).expect("Register utils");
    ctx.run_script(
        "<inspect-parity>",
        r#"
f <- function(...) capture.output(...)
stopifnot(identical(f({cat("prefix"); 7}), "prefix[1] 7"))
out <- capture.output({
    stopifnot(identical(capture.output(cat("private\n")), "private"))
    stopifnot(identical(capture.output(cat("shared\n"), split=TRUE), "shared"))
    stopifnot(identical(capture.output(message("message"),type="message"), "message"))
    str(list(x=1L))
})
stopifnot(identical(out, c("shared", "List of 1", " $ x: int 1")))
tryCatch(capture.output(stop("stop")), error=function(e) invisible(NULL))
stopifnot(identical(capture.output(cat("next")), "next"))
path <- tempfile()
error <- tryCatch(capture.output({cat("before error");stop("original error")}, file=path),
                  error=function(e) conditionMessage(e))
stopifnot(identical(error,"original error"),
          identical(readBin(path,"raw",n=12L),charToRaw("before error")))
unlink(path)
"#,
        GLOBAL_ENV,
        false,
    )
    .expect("Check nested output capture");
}

#[test]
fn capture_reports_limit_and_restores_the_destination() {
    use roxide_host::{Limits, NativeHost};

    let host = NativeHost::with_limits(Limits {
        max_vector_bytes: 64,
        ..Limits::default()
    });
    let mut ctx = Ctx::new(Box::new(host), GcPolicy::Torture).expect("Create the R session");
    let ((error, output), _) = ctx
        .capture_streams(false, true, false, |ctx| {
            let error = ctx.capture_streams(true, false, false, |ctx| {
                ctx.host()
                    .write_out(&[b'x'; 64])
                    .expect("Fill the output buffer");
                assert!(ctx.host().write_out(b"x").is_err());
                Ok(())
            });
            let (_, output) = ctx.capture_streams(true, false, false, |ctx| {
                ctx.host().write_out(b"next").expect("Write the next line");
                Ok(())
            })?;
            Ok((error, output))
        })
        .expect("Capture the error message");
    assert!(error.is_err());
    assert_eq!(output, b"next");
}

#[test]
fn inspection_uses_registered_s3_methods_and_validates_before_evaluation() {
    let mut ctx =
        Ctx::new(Box::new(PureHost::new()), GcPolicy::Torture).expect("Create the R session");
    roxide_base::register_all(&mut ctx).expect("Register base");
    roxide_utils::register(&mut ctx).expect("Register utils");
    ctx.run_script(
        "Inspection S3 dispatch",
        r#"
out <- local({
    capture <- function(x) UseMethod("capture")
    capture.default <- function(x) "default"
    capture(structure(1, class = "output"))
}, new.env(parent = baseenv()))
stopifnot(identical(out, "default"))
stopifnot(identical(capture.output(print(object.size(1))), "56 bytes"))
session <- sessionInfo()
stopifnot(identical(capture.output(print(session))[1L], session$R.version$version.string))

seen <- FALSE
run <- function() { seen <<- TRUE; invisible(NULL) }
err <- tryCatch(capture.output(run(), file = 1), error = conditionMessage)
stopifnot(identical(err, "'file' must be NULL, a character string or a connection"), !seen)
err <- tryCatch(capture.output(run(), type = c("out", "msg")), error = conditionMessage)
stopifnot(identical(err, "'arg' must be of length 1"), !seen)

length.audit <- function(x) 1L
str.audit <- function(object, ...) cat("custom\n")
a <- structure(1:5, class = "audit")
stopifnot(identical(head(a), 1L), identical(tail(a), 1L))
stopifnot(identical(capture.output(str(list(a))), c("List of 1", " $ :custom")))

x <- 1
for (i in 1:40) x <- list(x, x)
stopifnot(as.double(object.size(x)) == 120 * 2^40 - 64)

f <- function() { 1; 2; 3 }
stopifnot(identical(dim(head(f, 2L)), c(2L, 1L)),
          identical(rownames(tail(f, 2L)), c("5", "6")))
big <- eval(parse(text = paste0("function() {",
    paste(rep("123", 2000L), collapse = ";"), "}")))
stopifnot(identical(dim(head(big, 2L)), c(2L, 1L)),
          identical(rownames(tail(big, 2L)), c("2002", "2003")),
          identical(capture.output(str(big)), "function ()  "))
"#,
        GLOBAL_ENV,
        false,
    )
    .expect("Check inspection behavior");
}
