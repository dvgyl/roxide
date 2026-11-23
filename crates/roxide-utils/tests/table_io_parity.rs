//! Check table I/O with an in-memory host under garbage collection stress.

#![forbid(unsafe_code)]

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::PureHost;
use roxide_value::GLOBAL_ENV;

#[test]
fn table_io_preserves_types_and_rows_through_file_connections() {
    let mut ctx =
        Ctx::new(Box::new(PureHost::new()), GcPolicy::Torture).expect("Create the R session");
    roxide_base::register_all(&mut ctx).expect("Register base");
    roxide_utils::register(&mut ctx).expect("Register utils");
    ctx.run_script(
        "<table-io-parity>",
        r#"
p <- tempfile()
d <- read.csv2(text = 'id;name;value\n007;"a;z";1,25\n008;"b""q";NA',
               colClasses = c(id = "character"))
stopifnot(identical(d$id, c("007", "008")),
          identical(d$name, c("a;z", 'b"q')),
          identical(d$value, c(1.25, NA_real_)))
out <- file(p)
write.csv2(d, out, row.names = FALSE)
stopifnot(!isOpen(out))
close(out)
src <- file(p, "r")
x <- read.csv2(src, nrows = 1L, colClasses = c(id = "character"))
stopifnot(isOpen(src), identical(x$id, "007"),
          identical(readLines(src), '"008";"b""q";NA'))
close(src)
unlink(p)
"#,
        GLOBAL_ENV,
        false,
    )
    .expect("Check the table round trip");
}

#[test]
fn bounded_scans_keep_later_standard_input_lines() {
    let mut host = PureHost::new();
    host.set_stdin(b"1 2 3\n4 5\n".to_vec());
    let mut ctx = Ctx::new(Box::new(host), GcPolicy::Torture).expect("Create the R session");
    roxide_base::register_all(&mut ctx).expect("Register base");
    ctx.run_script(
        "<table-io-stdin>",
        r#"
stopifnot(identical(scan(nmax = 1L, quiet = TRUE), 1),
          identical(scan(nmax = 2L, quiet = TRUE), c(2, 3)),
          identical(readLines(stdin()), "4 5"))
"#,
        GLOBAL_ENV,
        false,
    )
    .expect("Check the scan position");
}
