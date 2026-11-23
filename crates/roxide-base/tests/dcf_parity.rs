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
