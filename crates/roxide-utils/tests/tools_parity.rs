//! Check tools I/O, resource limits, and licence classification.

#![forbid(unsafe_code)]

use std::sync::{Mutex, PoisonError};

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::{Host, Limits, NativeHost, PureHost};
use roxide_value::GLOBAL_ENV;

fn run(host: impl Host + 'static, script: &str) {
    let mut ctx = Ctx::new(Box::new(host), GcPolicy::Torture).expect("Create the R session");
    roxide_base::register_all(&mut ctx).expect("Register base");
    roxide_utils::register(&mut ctx).expect("Register utils and tools");
    ctx.run_script("<tools-parity>", script, GLOBAL_ENV, false)
        .expect("Run the tools test");
}

static NATIVE_HOST: Mutex<()> = Mutex::new(());

const ROUND_TRIP: &str = r#"
path <- tempfile()
writeLines(c("\\name{x}", "\\title{Test title}", "\\description{First.", "", "% comment", "Second.}"), path)
on_disk <- tools::parse_Rd(path, macros = FALSE)
stopifnot(identical(capture.output(print(on_disk)), readLines(path)))
file_con <- file(path)
from_file_con <- tools::parse_Rd(file_con, macros = FALSE)
stopifnot(!isOpen(file_con), identical(as.character(on_disk), as.character(from_file_con)))
close(file_con)
input <- textConnection(readLines(path))
from_connection <- tools::parse_Rd(input, macros = FALSE)
stopifnot(isOpen(input), identical(as.character(on_disk), as.character(from_connection)))
close(input)
output <- textConnection("result", "w")
returned <- withVisible(tools::Rd2txt(on_disk, output, options = list(underline_titles = FALSE)))
stopifnot(!returned$visible, identical(returned$value, "result"))
stopifnot(identical(result, c("Test title", "", "Description:", "", "     First.", "", "     Second.", "")))
close(output)
destination <- tempfile()
tools::Rd2txt(from_connection, destination, options = list(underline_titles = FALSE))
stopifnot(identical(readLines(destination), result))
stopifnot(file.exists(tools::file_path_as_absolute(path)))
unlink(c(path, destination))
"#;

#[test]
fn rd_file_and_connection_on_both_hosts() {
    run(PureHost::new(), ROUND_TRIP);
    let _guard = NATIVE_HOST.lock().unwrap_or_else(PoisonError::into_inner);
    run(NativeHost::new(), ROUND_TRIP);
}

#[test]
fn rd_input_limits_reject_large_and_deep_documents() {
    let _guard = NATIVE_HOST.lock().unwrap_or_else(PoisonError::into_inner);
    let limits = Limits {
        max_vector_bytes: 64 * 1024,
        ..Limits::default()
    };
    run(
        NativeHost::with_limits(limits),
        r#"
fails <- function(text, needle) {
    con <- textConnection(text)
    on.exit(close(con))
    message <- tryCatch({ tools::parse_Rd(con, fragment = TRUE); "" },
                        error = function(e) conditionMessage(e))
    if (!grepl(needle, message, fixed = TRUE)) stop(paste("Expected", needle, "but got", message))
}
fails(paste0(paste(rep("{", 150L), collapse = ""), "x", paste(rep("}", 150L), collapse = "")), "nesting depth limit")
fails(paste(rep("x\\R ", 1000L), collapse = ""), "size limit")
fails(c("\\newcommand{\\loop}{\\loop}", "\\loop"), "nesting depth limit")
con <- textConnection(c("\\name{x}", "\\title{T}", "\\usage{", rep("f()", 8), "}"))
rd <- tools::parse_Rd(con)
close(con)
message <- tryCatch({
    tools::Rd2txt(rd, options = list(sectionIndent = 10000L))
    ""
}, error = function(e) conditionMessage(e))
stopifnot(grepl("size limit", message, fixed = TRUE))
"#,
    );
}

#[test]
fn cran_licence_table_and_file_extensions() {
    run(
        PureHost::new(),
        r#"
x <- tools::analyze_license("MIT + file LICENCE | GPL (>= 2)")
stopifnot(x$is_verified, x$is_FOSS, !x$restricts_use, identical(x$pointers, "LICENCE"))
stopifnot(identical(x$extensions$extensible, TRUE))
stopifnot(identical(x$spdx, "MIT OR GPL-2.0-only OR GPL-3.0-only"))
stopifnot(identical(tools::analyze_license("GPL (>= 9)")$spdx, NA_character_))
stopifnot(identical(tools::analyze_license("CC BY-NC 4.0")$restricts_use, TRUE))
stopifnot(identical(tools::file_ext(c(NA, ".profile", "x.tar.gz")), c("", "", "gz")))
stopifnot(identical(tools::file_path_sans_ext("x.tar.gz", compression = TRUE), "x"))
"#,
    );
}

#[test]
fn rd_stages_and_path_extensions() {
    run(
        PureHost::new(),
        r#"
options <- list(underline_titles = FALSE)
rd <- paste0("\\name{x}\\title{X}\\description{",
             '\\Sexpr[stage=build]{2 + 2} \\Sexpr[stage=render]{3 + 2}}')
render <- function(stages) {
    input <- textConnection(rd)
    out <- textConnection("lines", "w", local=TRUE)
    on.exit({close(input); close(out)})
    result <- withVisible(tools::Rd2txt(input, out, stages=stages, options=options))
    stopifnot(!result$visible, identical(result$value, "lines"))
    lines
}
stopifnot(any(grepl("4 5", render(c("build", "render")), fixed=TRUE)))
stopifnot(any(grepl('\\Sexpr[stage=build]', render("render"), fixed=TRUE)))
stopifnot(any(grepl('\\Sexpr[stage=render]', render("build"), fixed=TRUE)))
stopifnot(identical(capture.output(tools::Rd2txt(textConnection(r"(\eqn{\alpha + x})"),fragment=TRUE)), "alpha + x"))
bad <- textConnection(r"(\tabular{l}{Left\tab Right\cr})")
message <- tryCatch(tools::Rd2txt(bad,fragment=TRUE), error=conditionMessage)
stopifnot(grepl("too many columns for format 'l'", message, fixed=TRUE))
close(bad)
long <- paste(rep("a", 65536), collapse="")
stopifnot(identical(tools::file_ext(c(paste0(long,".zip"), "a.²", "a.١", "a.é", "a.β", "a.四")), c("zip", "", "١", "é", "β", "四")))
license <- tools::analyze_license("Mozilla Public License 2.0 | GNU General Public License")
stopifnot(license$is_canonical, identical(license$spdx, "MPL-2.0 OR GPL-2.0-only OR GPL-3.0-only"))
"#,
    );
}
