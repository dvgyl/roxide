#![forbid(unsafe_code)]

//! Check public R behavior with expectations from GNU R 4.6.2.

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::{Host, PureHost};
use roxide_value::GLOBAL_ENV;

fn check(name: &str, script: &str) {
    let mut host = PureHost::new();
    host.set_env_var("TZ", Some(b"UTC"))
        .expect("Set the time zone");
    let mut ctx = Ctx::new(Box::new(host), GcPolicy::Torture).expect("Create an R session");
    roxide_base::register_all(&mut ctx).expect("Register base functions");
    let (result, stderr) = ctx
        .capture_streams(false, true, false, |ctx| {
            Ok(ctx.run_script(name, script, GLOBAL_ENV, false))
        })
        .expect("Capture R errors");
    result
        .unwrap_or_else(|error| panic!("{name}: {error:?}\n{}", String::from_utf8_lossy(&stderr)));
}

#[test]
fn date_and_posix_concatenation_and_arithmetic() {
    check(
        "date-time concatenation and arithmetic",
        r#"
d <- as.Date("2020-01-01")
ct <- as.POSIXct(1, tz = "UTC")
lt <- as.POSIXlt(ct)
stopifnot(identical(class(c(a = d)), "Date"), identical(names(c(a = d)), "a"),
          identical(as.double(c(a = d, NA)), c(18262, NA_real_)),
          identical(as.double(c(d, "2020-01-02", NULL)), c(18262, 18263)),
          identical(c(d, recursive = TRUE), d))
stopifnot(identical(class(c(a = ct, NA)), c("POSIXct", "POSIXt")),
          identical(names(c(a = ct, NA)), c("a", "")),
          is.null(attr(c(ct, NA), "tzone", exact = TRUE)),
          identical(attr(c(ct, lt, NULL), "tzone"), "UTC"),
          is.null(attr(c(ct, as.POSIXct(1, tz = "GMT")), "tzone", exact = TRUE)))
combined <- c(a = lt, NA)
stopifnot(identical(class(combined), c("POSIXlt", "POSIXt")),
          identical(attr(combined, "tzone"), "UTC"),
          identical(names(combined), c("a", "")),
          identical(as.double(as.POSIXct(combined)), c(1, NA_real_)),
          identical(attr(c(lt, as.POSIXlt(2, tz = "GMT")), "tzone"), "UTC"))

stopifnot(identical(class(d + 1:3), "Date"),
          identical(as.double(d + 1:3), c(18263, 18264, 18265)),
          identical(as.double(1:3 + d), as.double(d + 1:3)),
          identical(as.double(d - c(0, NA, 2)), c(18262, NA_real_, 18260)),
          identical(as.double(d + as.difftime(48, units = "hours")), 18264),
          identical(units(d - d), "days"), as.double(d - d) == 0)
for (time in list(ct, lt, Sys.time())) {
    result <- time + c(1, 2, NA, 3)
    stopifnot(identical(class(result), c("POSIXct", "POSIXt")),
              identical(attr(result, "tzone"), attr(as.POSIXct(time), "tzone")),
              identical(is.na(result), c(FALSE, FALSE, TRUE, FALSE)))
}
stopifnot(identical(as.double(ct - c(1, NA, 3)), c(0, NA_real_, -2)),
          identical(attr(ct - 2, "tzone"), "UTC"),
          identical(as.double(lt + as.difftime(1, units = "mins")), 61),
          identical(units(ct - lt), "secs"))
now <- Sys.time()
stopifnot(inherits(c(now, NA), "POSIXct"), is.na(c(now, NA)[2L]),
          identical(length(as.POSIXlt(now)), 1L),
          inherits(c(as.POSIXlt(now), NA), "POSIXlt"),
          is.na(c(as.POSIXlt(now), NA)[2L]))
"#,
    );
}

#[test]
fn date_and_posix_subsets_replacement_and_replication() {
    check(
        "date-time indexing and replication",
        r#"
d <- setNames(as.Date(0:1), c("a", "b"))
ct <- setNames(as.POSIXct(0:1, tz = "UTC"), c("a", "b"))
lt <- as.POSIXlt(ct)
for (x in list(d, ct, lt)) {
    y <- x[c(2L, NA_integer_, 1L, 4L)]
    stopifnot(identical(class(x), class(y)), identical(attr(x, "tzone"), attr(y, "tzone")),
              identical(names(y), c("b", NA_character_, "a", NA_character_)),
              identical(unname(is.na(y)), c(FALSE, TRUE, FALSE, TRUE)))
    stopifnot(identical(class(x[[1L]]), class(x)), is.null(names(x[[1L]])))
    y <- x
    y[2L] <- NA
    stopifnot(identical(class(x), class(y)), identical(attr(x, "tzone"), attr(y, "tzone")),
              identical(names(y), c("a", "b")), is.na(y[2L]), !is.na(x[2L]))
    y[1L] <- x[2L]
    stopifnot(identical(format(y[1L]), setNames(format(x[[2L]]), "a")))
    y <- rep(x, each = 2L, length.out = 5L)
    stopifnot(identical(class(y), class(x)), identical(attr(y, "tzone"), attr(x, "tzone")),
              identical(names(y), c("a", "a", "b", "b", "a")))
    y <- rep.int(times = c(2L, 1L), x = x)
    stopifnot(identical(class(y), class(x)), identical(names(y), c("a", "a", "b")))
    class(x) <- c("custom", class(x))
    stopifnot(identical(class(x[1L]), class(x)), identical(class(rep(x, 2L)), class(x)))
    y <- x
    y[2L] <- NA
    stopifnot(identical(class(y), class(x)))
}
stopifnot(identical(length(lt), 2L), identical(names(lt), c("a", "b")),
          identical(names(lt["b"]), "b"),
          identical(as.double(as.POSIXct(lt[["b"]])), 1),
          identical(lt[, "sec"], c(0, 1)), identical(length(lt[integer()]), 0L))
names(lt) <- "z"
stopifnot(identical(names(lt), c("z", NA_character_)),
          identical(is.na(lt), setNames(c(FALSE, FALSE), c("z", NA_character_))))
names(lt) <- NULL
stopifnot(is.null(names(lt)), identical(length(lt), 2L))
"#,
    );
}

#[test]
fn data_frame_construction_and_column_replacement() {
    check(
        "data-frame construction and replacement",
        r#"
d <- as.Date("2020-01-01")
ct <- as.POSIXct(0:1, tz = "UTC")
lt <- as.POSIXlt(ct)
frame <- data.frame(d = d, ct = ct, lt = lt, keep = I(1:4), n = 1:4)
stopifnot(identical(names(frame), c("d", "ct", "lt", "keep", "n")),
          identical(class(frame$d), "Date"), identical(as.double(frame$d), rep(18262, 4L)),
          identical(class(frame$ct), c("POSIXct", "POSIXt")),
          identical(attr(frame$ct, "tzone"), "UTC"),
          identical(as.double(frame$ct), c(0, 1, 0, 1)),
          identical(class(frame$lt), c("POSIXct", "POSIXt")),
          identical(attr(frame$lt, "tzone"), "UTC"),
          identical(as.double(frame$ct), as.double(frame$lt)), identical(class(frame$keep), "AsIs"))
stopifnot(identical(class(frame[1L, "d"]), "Date"),
          identical(attr(frame[1L, "ct"], "tzone"), "UTC"))
asis <- I(setNames(1:2, c("a", "b")))
frame <- data.frame(asis, check.names = FALSE)
stopifnot(identical(names(frame), "asis"),
          identical(frame$asis, asis), identical(row.names(frame), c("a", "b")),
          identical(row.names(data.frame(asis, row.names = NULL)), c("1", "2")))

for (replace in list(function(df, value) {df$x <- value; df},
                     function(df, value) {df[["x"]] <- value; df})) {
    frame <- data.frame(n = 1:2)
    m <- matrix(1:6, 2, 3)
    sub <- data.frame(a = 1:2, b = c("s", "t"), c = c(FALSE, TRUE))
    stopifnot(identical(replace(frame, m)$x, m),
              identical(replace(frame, sub)$x, sub),
              identical(replace(frame, list(a = 3L))$x, list(a = 3L, a = 3L)),
              identical(replace(frame, d)$x, rep(d, 2L)),
              identical(replace(frame, lt)$x, lt))
    stopifnot(identical(dim(replace(frame, m)[2L, , drop = FALSE]$x), c(1L, 3L)),
              identical(nrow(replace(frame, sub)[2L, , drop = FALSE]$x), 1L),
              identical(tryCatch(replace(data.frame(n = 1:4), m), error = conditionMessage),
                        "replacement has 2 rows, data has 4"),
              identical(tryCatch(replace(data.frame(n = 1:4), sub), error = conditionMessage),
                        "replacement has 2 rows, data has 4"))
}
"#,
    );
}

#[test]
fn row_names_get_set_and_reset() {
    check(
        "row names",
        r#"
frame <- data.frame(x = 1:3)
stopifnot(identical(row.names(frame), c("1", "2", "3")),
          identical(row.names.data.frame(frame), row.names(frame)),
          identical(.row_names_info(frame, 0L), c(NA_integer_, -3L)),
          identical(.row_names_info(frame), -3L),
          identical(.row_names_info(frame, 2L), 3L),
          identical(.set_row_names(3L), c(NA_integer_, -3L)),
          identical(.set_row_names(0L), integer()))
row.names(frame) <- 4:6
stopifnot(identical(attr(frame, "row.names"), 4:6),
          identical(row.names(frame), c("4", "5", "6")))
frame <- `row.names<-.data.frame`(frame, c(4, 5, 6))
stopifnot(identical(attr(frame, "row.names"), c("4", "5", "6")))
set_rows <- function(v)
    tryCatch(suppressWarnings(`row.names<-`(frame, v)), error = conditionMessage)
stopifnot(identical(set_rows(c("a", "a", "c")), "duplicate 'row.names' are not allowed"),
          identical(set_rows(c(NA, "b", "c")), "missing values in 'row.names' are not allowed"),
          identical(set_rows("a"), "invalid 'row.names' length"))
row.names(frame) <- NULL
stopifnot(identical(.row_names_info(frame, 0L), .set_row_names(3L)))
empty <- data.frame()
row.names(empty) <- NULL
stopifnot(identical(row.names(empty), character()))
m <- matrix(1:4, 2)
stopifnot(is.null(row.names(m)), is.null(row.names.default(1:2)))
m <- `row.names<-.default`(m, c(7, 8))
stopifnot(identical(row.names(m), c("7", "8")))
row.names(m) <- NULL
stopifnot(is.null(row.names(m)))
"#,
    );
}

#[test]
fn do_call_passes_the_inner_call_to_binding_functions() {
    check(
        "do.call with rbind and cbind",
        r#"
for (f in list(rbind, "rbind")) {
    m <- do.call(f, list(c(1L, 2L)))
    stopifnot(identical(dim(m), c(1L, 2L)), is.null(dimnames(m)))
    m <- do.call(f, list(row = c(a = 1L, b = 2L)))
    stopifnot(identical(dimnames(m), list("row", c("a", "b"))))
}
for (f in list(cbind, "cbind")) {
    m <- do.call(f, list(c(1L, 2L)))
    stopifnot(identical(dim(m), c(2L, 1L)), is.null(dimnames(m)))
    m <- do.call(f, list(col = c(a = 1L, b = 2L)))
    stopifnot(identical(dimnames(m), list(c("a", "b"), "col")))
}
x <- 1:2
stopifnot(identical(colnames(do.call(cbind, list(quote(x)))), "x"),
          identical(do.call(identity, list(quote(x)), quote = TRUE), quote(x)))
"#,
    );
}

#[test]
fn rep_int_repeats_vectors_and_checks_times() {
    check(
        "rep.int",
        r#"
stopifnot(identical(rep.int(c(a = 1L, b = 2L), 2), c(1L, 2L, 1L, 2L)),
          identical(rep.int(times = c(2.9, 0, 1), x = 1:3), c(1L, 1L, 3L)),
          identical(rep.int(list(a = "s", b = 1:2), c(1L, 2L)), list("s", 1:2, 1:2)),
          identical(rep.int(raw(2L), 0L), raw()),
          identical(rep.int(vector("expression", 2L), c(1L, 2L)), vector("expression", 3L)),
          identical(rep.int(integer(), integer()), integer()),
          identical(rep.int(factor(c("a", "b")), c(1L, 2L)), factor(c("a", "b", "b"))))
for (times in list(NA, Inf, -1L, 1:3)) {
    stopifnot(identical(tryCatch(rep.int(1:2, times), error = conditionMessage),
                        "invalid 'times' value"))
}
"#,
    );
}
