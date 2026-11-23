#![forbid(unsafe_code)]

//! Check I/O behavior against GNU R 4.6.2 on both host implementations.

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::{Host, Limits, NativeHost, PureHost};
use roxide_value::GLOBAL_ENV;

fn run(ctx: &mut Ctx, script: &str) {
    ctx.run_script("<io-os-parity>", script, GLOBAL_ENV, false)
        .unwrap_or_else(|error| panic!("{error:?}\n{script}"));
}

fn context(host: Box<dyn Host>) -> Ctx {
    let mut ctx = Ctx::new(host, GcPolicy::Torture).expect("Create R session");
    roxide_base::register_all(&mut ctx).expect("Register base functions");
    ctx
}

fn roundtrips(ctx: &mut Ctx) {
    run(
        ctx,
        r#"
(function() {
    path <- tempfile()
    on.exit(unlink(path))
    con <- file(path)
    stopifnot(!isOpen(con))
    writeLines(c("one", "", "three"), con)
    stopifnot(!isOpen(con))
    stopifnot(identical(readLines(con, 2L), c("one", "")))
    stopifnot(identical(readLines(con, 1L), "one"))
    close(con)

    con <- file(path, "r")
    stopifnot(isOpen(con, "read"), !isOpen(con, "write"))
    stopifnot(identical(readLines(con, 2L), c("one", "")))
    stopifnot(identical(readLines(con, 1L), "three"))
    stopifnot(identical(readLines(con, 1L), character()))
    close(con)

    writeBin(as.raw(c(116L, 111L, 112L, 10L, 10L, 98L, 111L, 116L, 116L, 111L, 109L)), path)
    stopifnot(identical(readLines(path, warn = FALSE), c("top", "", "bottom")))

    for (open_file in list(gzfile, bzfile, xzfile)) {
        con <- open_file(path, "wb")
        writeBin(c(258L, -2L, NA_integer_), con, endian = "big")
        close(con)
        con <- open_file(path, "rb")
        stopifnot(identical(readBin(con, "integer", 1L, endian = "big"), 258L))
        stopifnot(identical(readBin(con, integer(), 9L, endian = "big"), c(-2L, NA_integer_)))
        close(con)
    }

    writeChar(c("café!", "界x", "end"), path, c(4L, 1L, 3L), eos = NULL)
    stopifnot(identical(readChar(path, c(4L, 1L, 3L)), c("café", "界", "end")))
    stopifnot(identical(readBin(path, "raw", 5L), as.raw(c(99L, 97L, 102L, 195L, 169L))))
    stopifnot(identical(writeChar("a", raw()), as.raw(c(97L, 0L))))

    bytes <- writeBin(c(-1L, -32768L, 258L), raw(), size = 2L, endian = "big")
    stopifnot(identical(readBin(bytes, integer(), 3L, 2L, FALSE, "big"), c(65535L, 32768L, 258L)))
    bytes <- writeBin(c(-0.0, Inf, NA_real_, NaN), raw(), endian = "big")
    value <- readBin(bytes, numeric(), 4L, endian = "big")
    stopifnot(identical(1 / value[1L], -Inf), identical(value[2:4], c(Inf, NA_real_, NaN)))
    stopifnot(identical(writeBin(value, raw(), endian = "big"), bytes))
    stopifnot(identical(readBin(writeBin(c("", "abc"), raw()), character(), 3L), c("", "abc", "")))
    stopifnot(identical(readBin(writeBin(c(1+2i, 3-4i), raw()), "complex", 2L), c(1+2i, 3-4i)))
})()
"#,
    );
}

fn filesystem(ctx: &mut Ctx) {
    run(
        ctx,
        r#"
(function() {
    root <- tempfile()
    stopifnot(dir.create(root))
    on.exit(unlink(root, recursive = TRUE))
    nested <- file.path(root, "nested")
    stopifnot(dir.create(nested))
    stopifnot(file.create(file.path(root, c("a.1", "ab1", ".hidden", "nested/data"))))
    stopifnot(identical(list.files(root, ".1$"), c("a.1", "ab1")))
    stopifnot(identical(list.files(root, ".1", fixed = TRUE), "a.1"))
    stopifnot(identical(dir(root, all.files = TRUE), c(".", "..", ".hidden", "a.1", "ab1", "nested")))
    stopifnot(identical(dir(root, all.files = TRUE, no.. = TRUE), c(".hidden", "a.1", "ab1", "nested")))
    stopifnot(identical(dir(root, recursive = TRUE, include.dirs = TRUE),
                        c("a.1", "ab1", "nested", "nested/data")))
    old <- file.path(nested, "data")
    new <- file.path(nested, "renamed")
    writeLines("abc", old)
    stopifnot(file.rename(old, new), !file.exists(old), file.exists(new), dir.exists(nested))
    stopifnot(identical(file.info(new)$size, 4), identical(file.info(new)$isdir, FALSE))
    stopifnot(identical(basename(dirname(new)), "nested"))
    stopifnot(identical(normalizePath(file.path(nested, "..", "nested", "renamed"), mustWork = TRUE),
                        normalizePath(new, mustWork = TRUE)))
    original <- setwd(nested)
    stopifnot(identical(normalizePath(getwd()), normalizePath(nested)))
    setwd(original)
    stopifnot(Sys.setenv(ROXIDE_IO_TEST = "test value"))
    stopifnot(identical(Sys.getenv("ROXIDE_IO_TEST"), "test value"))
    stopifnot(Sys.unsetenv("ROXIDE_IO_TEST"))
    stopifnot(identical(Sys.getenv("ROXIDE_IO_TEST", unset = "missing"), "missing"))
    stopifnot(identical(extSoftVersion()[["zstd"]], "1.5.7"))
})()
"#,
    );
}

#[test]
fn pure_host_preserves_text_streams_and_local_bindings() {
    let mut host = PureHost::new();
    host.set_stdin(b"one\ntwo\n");
    let mut ctx = context(Box::new(host));
    run(
        &mut ctx,
        r#"
stopifnot(identical(readLines(n = 1L), "one"), identical(readLines(), "two"))
(function() {
    input <- textConnection(c("red", "", "green"))
    stopifnot(identical(readLines(input, 2L), c("red", "")))
    stopifnot(identical(readLines(input), "green"))
    close(input)
    lines <- "old"
    output <- textConnection("lines", "a", local = TRUE)
    writeLines("new", output, sep = "")
    stopifnot(identical(lines, "old"), identical(textConnectionValue(output), "old"))
    writeLines("", output)
    stopifnot(identical(lines, c("old", "new")), identical(textConnectionValue(output), lines))
    writeLines("tail", output, sep = "")
    close(output)
    stopifnot(identical(lines, c("old", "new", "tail")))
})()
# The connection keeps its original environment alive after the function returns.
con <- (function() textConnection("saved", "w", local = TRUE))()
writeLines("saved", con)
stopifnot(identical(textConnectionValue(con), "saved"))
close(con)
"#,
    );
    roundtrips(&mut ctx);
    filesystem(&mut ctx);
}

#[test]
fn native_host_roundtrips_and_limits_decompression() {
    let mut ctx = context(Box::new(NativeHost::with_limits(Limits {
        max_decompress_bytes: 64,
        ..Limits::default()
    })));
    roundtrips(&mut ctx);
    filesystem(&mut ctx);
    run(
        &mut ctx,
        r#"
(function() {
    path <- tempfile()
    on.exit(unlink(path))
    con <- gzfile(path, "wb")
    writeBin(raw(65L), con)
    close(con)
    message <- tryCatch({
        readLines(path)
        ""
    }, error = function(e) conditionMessage(e))
    stopifnot(identical(message, "input exceeds the byte limit"))
})()
stopifnot(identical(system2("printf", c("'%s\n'", "'one two'"), stdout = TRUE), "one two"))
"#,
    );
}
