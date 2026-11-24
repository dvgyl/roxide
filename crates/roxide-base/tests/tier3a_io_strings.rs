#![forbid(unsafe_code)]

//! Compare base I/O and coercion with GNU R 4.6.2 without network access.

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::{Host, PureHost};
use roxide_value::GLOBAL_ENV;

fn run(host: impl Host + 'static, script: &str) {
    let mut ctx = Ctx::new(Box::new(host), GcPolicy::Torture).expect("Create R session");
    roxide_base::register_all(&mut ctx).expect("Register base functions");
    let (result, stderr) = ctx
        .capture_streams(false, true, false, |ctx| {
            Ok(ctx.run_script("<tier3a-io-strings>", script, GLOBAL_ENV, false))
        })
        .expect("Capture R diagnostics");
    result.unwrap_or_else(|error| {
        panic!("{error:?}\n{}\n{script}", String::from_utf8_lossy(&stderr))
    });
}

#[test]
fn read_lines_signals_a_mufflable_warning() {
    run(
        PureHost::new(),
        r#"
        path <- tempfile()
        writeBin(charToRaw("complete\npartial"), path)
        con <- file(path)
        warnings <- character()
        result <- withCallingHandlers(readLines(con), warning = function(w) {
            stopifnot(inherits(w, "simpleWarning"))
            warnings <<- c(warnings, conditionMessage(w))
            invokeRestart("muffleWarning")
        })
        stopifnot(identical(result, c("complete", "partial")),
                  identical(warnings, paste0("incomplete final line found on '", path, "'")),
                  !isOpen(con))
        close(con)
        "#,
    );
}

#[test]
fn open_updates_file_mode_and_warns_on_open_memory_connections() {
    run(
        PureHost::new(),
        r#"
        path <- tempfile()
        con <- file(path)
        result <- withVisible(open(con, open = "wb", blocking = FALSE))
        stopifnot(is.null(result$value), !result$visible,
                  isOpen(con, "write"), !isOpen(con, "read"))
        writeBin(charToRaw("abc"), con)
        close(con)
        con <- file(path)
        open.connection(con, "rb")
        stopifnot(isOpen(con, "read"), summary(con)$mode == "rb", summary(con)$text == "binary",
                  identical(readBin(con, raw(), 3L), charToRaw("abc")))
        close(con)
        for (con in list(rawConnection(raw()), textConnection("line", open = ""))) {
            before <- summary(con)
            warning <- NULL
            result <- withCallingHandlers(withVisible(open(con, "rb")), warning = function(w) {
                warning <<- conditionMessage(w)
                invokeRestart("muffleWarning")
            })
            stopifnot(is.null(result$value), !result$visible,
                      identical(warning, "connection is already open"),
                      identical(summary(con), before))
            close(con)
            stopifnot(identical(tryCatch(open(con), error = conditionMessage), "invalid connection"))
        }
        stopifnot(identical(tryCatch(open(stdin()), error = conditionMessage),
                            "cannot open standard connections"))
        "#,
    );
}

#[test]
fn url_can_describe_an_unopened_connection_without_network_io() {
    run(
        PureHost::new(),
        r#"
        address <- "https://example.com"
        con <- base::url(address, headers = c(Accept = "application/json"))
        stopifnot(identical(class(con), c("url", "connection")), !isOpen(con),
                  identical(summary(con),
                    list(description = address, class = "url-libcurl", mode = "r", text = "text",
                         opened = "closed", "can read" = "yes", "can write" = "no")))
        stopifnot(grepl("network URL connections", tryCatch(open(con, "rb"), error = conditionMessage)),
                  !isOpen(con),
                  grepl("network URL connections",
                        tryCatch(readLines(con), error = conditionMessage)),
                  grepl("network URL connections",
                        tryCatch(url(address, open = "rb"), error = conditionMessage)))
        close(con)
        stopifnot(identical(tryCatch(url(address, headers = "unnamed"), error = conditionMessage),
                            "'headers' must have names and must not be NA"))
        path <- tempfile()
        writeLines("local file", path)
        con <- url(paste0("file://", path), open = "rt")
        stopifnot(identical(class(con), c("file", "connection")),
                  identical(readLines(con), "local file"))
        close(con)
        "#,
    );
}

#[test]
fn sys_time_internal_returns_host_epoch_seconds() {
    let host = PureHost::new();
    let seconds = host.now().unix_us as f64 / 1_000_000.0;
    run(
        host,
        &format!(
            r#"
            value <- .Internal(Sys.time())
            stopifnot(identical(value, {seconds:.6}), is.null(attributes(value)),
                      identical(class(Sys.time()), c("POSIXct", "POSIXt")),
                      identical(unclass(Sys.time()), value))
            "#,
        ),
    );
}

#[cfg(unix)]
#[test]
fn system_captures_output_and_reports_exit_status() {
    run(
        roxide_host::NativeHost::new(),
        r#"
        stopifnot(identical(withVisible(base::system("exit 7")),
                            list(value = 7L, visible = FALSE)),
                  identical(withVisible(system(":", wait = FALSE)),
                            list(value = 0L, visible = FALSE)),
                  identical(withVisible(system("printf 'first\n\nlast'", intern = TRUE, wait = FALSE)),
                            list(value = c("first", "", "last"), visible = TRUE)),
                  identical(system("cat", intern = TRUE, input = c("one", NA, "three")),
                            c("one", "NA", "three")),
                  identical(system("printf ignored", intern = TRUE, ignore.stdout = TRUE),
                            character()),
                  identical(system("sh -c 'printf ignored >&2'", intern = TRUE, ignore.stderr = TRUE),
                            character()))
        warning <- NULL
        result <- withCallingHandlers(system("printf partial; exit 9", intern = TRUE), warning = function(w) {
            warning <<- conditionMessage(w)
            invokeRestart("muffleWarning")
        })
        stopifnot(identical(result, structure("partial", status = 9L)),
                  identical(warning, "running command 'printf partial; exit 9' had status 9"),
                  identical(tryCatch(system("cat", input = 1L), error = conditionMessage),
                            "'input' must be a character vector or 'NULL'"))
        "#,
    );
}

#[test]
fn pretty_num_and_hexmode_format_each_value() {
    run(
        PureHost::new(),
        r#"
        z <- complex(real = c(1, 2, NA), imaginary = 3:1)
        stopifnot(identical(prettyNum(z), c("1+3i", "2+2i", "NA")),
                  identical(prettyNum(c(a = 1.23, b = 1.2, c = NA, d = 1000), big.mark = ","),
                            c(a = "1.23", b = "1.2", c = "NA", d = "1,000")),
                  identical(as.character.hexmode(charToRaw("bla")), c("62", "6c", "61")),
                  identical(as.character.hexmode(as.raw(c(0L, 16L))), c("0", "10")),
                  identical(as.hexmode(c(a = "0xf", b = "10")), structure(c(15L, 16L), class = "hexmode")),
                  identical(as.hexmode(c(1, NA_real_)), structure(c(1L, NA_integer_), class = "hexmode")),
                  identical(format(as.hexmode(c(1L, 16L))), c("01", "10")),
                  identical(as.character(as.hexmode(c(a = 1L, b = 16L))), c("1", "10")),
                  identical(format.hexmode(c(a = 15L, b = -1L, c = NA_integer_), width = 2L,
                                           upper.case = TRUE),
                            c(a = "0F", b = "FFFFFFFF", c = NA_character_)),
                  identical(format.hexmode(c(NA_integer_, NA_integer_)), rep(NA_character_, 2L)),
                  identical(tryCatch(as.hexmode("-1"), error = conditionMessage),
                            "'x' cannot be coerced to class \"hexmode\""))
        "#,
    );
}

#[test]
fn closure_list_preserves_missing_formals_and_the_body() {
    run(
        PureHost::new(),
        r#"
        f <- function(x, y = 1) x + y
        values <- as.list(f)
        stopifnot(identical(names(values), c("x", "y", "")),
                  identical(as.list.function(f), values),
                  identical(values[1L], as.list(formals(f))[1L]),
                  identical(values[[2L]], 1),
                  identical(values[[3L]], quote(x + y)),
                  identical(as.list(function() NULL), list(NULL)),
                  identical(as.list(function(...) 1), c(as.list(formals(function(...) NULL)), list(1))),
                  isTRUE(tryCatch(as.vector(f, "list"), error = function(e) TRUE)))
        "#,
    );
}
