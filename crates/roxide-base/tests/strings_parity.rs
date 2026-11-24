#![forbid(unsafe_code)]

//! Regression tests for base string primitives and their R return values.

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::PureHost;
use roxide_value::{GLOBAL_ENV, RLogical, ValueApi};

fn check(script: &str) {
    let mut ctx = Ctx::new(Box::new(PureHost::new()), GcPolicy::Torture).expect("create evaluator");
    roxide_base::register_all(&mut ctx).expect("register base primitives");
    ctx.run_script("string parity", script, GLOBAL_ENV, false)
        .expect("run string checks");
    let key = ctx.session_mut().intern("checks").expect("intern checks");
    let result = ctx
        .get_var(key, GLOBAL_ENV)
        .expect("read variable")
        .expect("checks");
    let values = ctx.session().lgl(result).expect("logical vector");
    assert!(!values.is_empty());
    assert_eq!(values.to_vec(), vec![RLogical::TRUE; values.len()]);
}

#[test]
fn paste_and_sprintf_follow_r_recycling_and_missing_value_rules() {
    check(
        r#"
        checks <- c(
            identical(paste("a", character(), 1:2, sep = ":"),
                      c("a::1", "a::2")),
            identical(paste0(character(), 1:2, recycle0 = TRUE), character()),
            identical(paste0(character(), NA_character_, collapse = ",", recycle0 = TRUE), ""),
            identical(paste0("x=", c(NA_character_, "a")), c("x=NA", "x=a")),
            identical(sprintf("%2$s/%1$05d", c(-42L, NA_integer_), c("x", NA_character_)),
                      c("x/-0042", "NA/000NA")),
            identical(sprintf("%#g %a", c(1e6, 1), c(1.5, 0.5)),
                      c("1.00000e+06 0x1.8p+0", "1.00000 0x1p-1")),
            identical(tryCatch(sprintf("%d", 1.25), error = conditionMessage),
                      "invalid format '%d'; use format %f, %e, %g or %a for numeric objects")
        )
        "#,
    );
}

#[test]
fn text_operations_count_code_points_and_preserve_encoding_marks() {
    check(
        r#"
        latin <- rawToChar(as.raw(c(0x63, 0x61, 0x66, 0xe9)))
        Encoding(latin) <- "latin1"
        y <- enc2utf8(c(latin, "plain", NA_character_))
        checks <- c(
            identical(Encoding(latin), "latin1"),
            identical(Encoding(y), c("UTF-8", "unknown", "unknown")),
            identical(charToRaw(y[1L]), as.raw(c(0x63, 0x61, 0x66, 0xc3, 0xa9))),
            identical(charToRaw(enc2native(latin)), as.raw(c(0x63, 0x61, 0x66, 0xc3, 0xa9))),
            identical(rawToChar(as.raw(c(0, 97, 0)), multiple = TRUE), c("", "a", "")),
            identical(nchar(c(latin, "界", "é", NA_character_), "width"), c(4L, 2L, 1L, 2L)),
            identical(nchar(c(latin, y[1L]), "bytes"), c(4L, 5L)),
            identical(nzchar(c("", NA_character_), keepNA = TRUE), c(FALSE, NA)),
            identical(substr(c(latin, "界艮"), c(4L, 2L), 8L), c("é", "艮")),
            identical(substring("界艮", c(1L, 2L), c(1L, 3L)), c("界", "艮"))
        )
        "#,
    );
}

#[test]
fn regex_modes_preserve_values_and_handle_zero_width_matches() {
    check(
        r#"
        x <- c(a = "Lot:37", b = NA_character_, c = "row:8")
        checks <- c(
            identical(grepv("(?<=lot:)\\d+$", x, perl = TRUE, ignore.case = TRUE), c(a = "Lot:37")),
            identical(grepv(":", x, fixed = TRUE, value = FALSE, invert = TRUE), 2L),
            identical(grepl(NA_character_, x), rep(NA, 3L)),
            identical(gsub("(?<=Lot:)(\\d)(\\d)", "\\2\\1", x, perl = TRUE),
                      c(a = "Lot:73", b = NA_character_, c = "row:8")),
            identical(strsplit(c("bay..sea.", "", NA_character_), ".", fixed = TRUE),
                      list(c("bay", "", "sea"), character(), NA_character_)),
            identical(strsplit("bay sea", "[ ]*"), list(c("b", "a", "y", "", "s", "e", "a"))),
            identical(gsub("a*", "X", c("aa", "ab", "ba", "")), c("X", "XbX", "XbX", "X")),
            identical(gsub("(a|aa)", "<\\1>", "aaa"), "<aa><a>"),
            identical(gsub("(?<=.)a", "b", "éa", perl = TRUE, useBytes = TRUE), "éb")
        )
        "#,
    );
}

#[test]
fn match_objects_use_the_correct_offsets_and_inverse_ranges() {
    check(
        r#"
        x <- c(a = "é:27", b = "none", c = NA_character_)
        first <- regexpr("[[:digit:]]+", x)
        bytes <- regexpr("[[:digit:]]+", x, useBytes = TRUE)
        all <- gregexpr("[[:digit:]]+", x)
        captures <- regexpr("(?<first>a)(b)?", c("é:a", "z"), perl = TRUE)
        checks <- c(
            identical(attr(captures, "capture.names"), c("first", "")),
            identical(attr(captures, "capture.start"),
                      matrix(c(3L, -1L, 0L, -1L), 2L, dimnames = list(NULL, c("first", "")))),
            identical(attr(captures, "capture.length"),
                      matrix(c(1L, -1L, 0L, -1L), 2L, dimnames = list(NULL, c("first", "")))),
            identical(first, structure(c(3L, -1L, NA_integer_),
                      match.length = c(2L, -1L, NA_integer_))),
            identical(bytes, structure(c(4L, -1L, NA_integer_),
                      match.length = c(2L, -1L, NA_integer_), index.type = "bytes", useBytes = TRUE)),
            identical(regmatches(x, first), c(a = "27")),
            identical(regmatches(x, all), list(a = "27", b = character(), c = character())),
            identical(regmatches(x, all, invert = TRUE),
                      list(a = c("é:", ""), b = "none", c = NA_character_)),
            identical(regmatches(x, all, invert = NA),
                      list(a = c("é:", "27", ""), b = "none", c = NA_character_))
        )
        "#,
    );
}
