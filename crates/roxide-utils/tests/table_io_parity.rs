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
f <- textConnection(c("   ", "1", "2"))
stopifnot(identical(scan(f, sep = ",", nmax = 1L, quiet = TRUE), 1),
          identical(readLines(f), "2"))
close(f)
"#,
        GLOBAL_ENV,
        false,
    )
    .expect("Check the scan position");
}

#[path = "../../roxide-base/tests/support/failing_host.rs"]
mod failing_host;

#[test]
fn write_table_preserves_the_write_error_across_close() {
    let mut ctx = Ctx::new(
        Box::<failing_host::FailingHost>::default(),
        GcPolicy::Torture,
    )
    .expect("Create the R session");
    roxide_base::register_all(&mut ctx).expect("Register base");
    roxide_utils::register(&mut ctx).expect("Register utils");
    ctx.run_script(
        "Table write failure",
        r#"
error <- tryCatch(write.table(matrix(1:4, 2L), "failing.txt", append = TRUE),
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
fn read_table_uses_original_column_positions_for_row_names() {
    let mut ctx =
        Ctx::new(Box::new(PureHost::new()), GcPolicy::Torture).expect("Create the R session");
    roxide_base::register_all(&mut ctx).expect("Register base");
    roxide_utils::register(&mut ctx).expect("Register utils");
    ctx.run_script(
        "Table column positions",
        r#"
s <- "a b c\n1 2 3\n4 5 6"
x <- read.table(text = s, header = TRUE, row.names = 2L,
                colClasses = c("NULL", "integer", "integer"))
stopifnot(identical(names(x), "c"), identical(rownames(x), c("2", "5")))
x <- read.table(text = s, header = TRUE, nrows = 0L)
stopifnot(identical(dim(x), c(2L, 3L)))
err <- tryCatch(read.table(text = s, nrows = 0L), error = conditionMessage)
stopifnot(identical(err, "invalid 'nlines' argument"))
"#,
        GLOBAL_ENV,
        false,
    )
    .expect("Check original column positions and zero row limit");
}

#[test]
fn table_io_rejects_oversized_output_and_recycled_classes() {
    let host = failing_host::FailingHost::with_limit(4096, true);
    let mut ctx = Ctx::new(Box::new(host), GcPolicy::Torture).expect("Create the R session");
    roxide_base::register_all(&mut ctx).expect("Register base");
    roxide_utils::register(&mut ctx).expect("Register utils");
    ctx.run_script(
        "Table size limits",
        r#"
header <- paste(paste0("x", 1:30), collapse = " ")
err <- tryCatch(read.table(text = header, header = TRUE,
                           colClasses = paste(rep("a", 200), collapse = "")),
                error = conditionMessage)
stopifnot(identical(err, "vector size limit exceeded"))
x <- matrix(1:200, 10L)
err <- tryCatch(write.table(x), error = conditionMessage)
stopifnot(identical(err, "vector size limit exceeded"))
"#,
        GLOBAL_ENV,
        false,
    )
    .expect("Check table size limits");
}
