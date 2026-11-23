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
