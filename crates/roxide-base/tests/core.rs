#![forbid(unsafe_code)]

//! Tests for the base primitives with the default evaluator and garbage collection.

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::PureHost;
use roxide_value::{GLOBAL_ENV, RLogical, ValueApi};

fn context() -> Ctx {
    let mut ctx = Ctx::new(Box::new(PureHost::new()), GcPolicy::Torture).expect("create evaluator");
    roxide_base::register_all(&mut ctx).expect("register base primitives");
    ctx
}

fn check(ctx: &mut Ctx, script: &str) {
    ctx.run_script("base primitives", script, GLOBAL_ENV, false)
        .expect("run base primitives");
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
fn replace_uses_subset_assignment() {
    check(
        &mut context(),
        r#"
        x <- setNames(1:3, c("a", "b", "c"))
        checks <- c(
            identical(base::replace(x, c("b", "d"), 9L), c(a=1L, b=9L, c=3L, d=9L)),
            identical(x, c(a=1L, b=2L, c=3L)),
            identical(base::replace(list(a=1, b=2), "a", NULL), list(b=2)),
            identical(base::replace(x, FALSE, 0L), x)
        )
        "#,
    );
}

#[test]
fn as_data_frame_dispatches_and_converts_vectors() {
    check(
        &mut context(),
        r#"
        x <- setNames(1:2, c("first", "second"))
        f <- base::as.data.frame(x)
        optional <- base::as.data.frame(x, optional=TRUE)
        as.data.frame.packet <- function(x, row.names=NULL, optional=FALSE, ...)
            list(row.names, optional)
        checks <- c(
            identical(class(f), "data.frame"),
            identical(names(f), "x"),
            identical(attr(f, "row.names"), c("first", "second")),
            identical(attr(base::as.data.frame(integer()), "row.names"), character()),
            identical(f[[1L]], 1:2),
            is.null(names(optional)),
            identical(attr(base::as.data.frame(x, row.names=c("a", "a")), "row.names"), c("a", "a")),
            identical(base::as.data.frame(structure(1L, class="packet"), c("r"), TRUE),
                      list("r", TRUE)),
            is.character(base::as.data.frame(c("b", "a"))[[1L]]),
            identical(names(base::as.data.frame("b", nm="column")), "column"),
            is.factor(base::as.data.frame(c("b", "a"), stringsAsFactors=TRUE)[[1L]]),
            identical(base::as.data.frame(f), f),
            identical(class(base::as.data.frame(structure(f, class=c("a", "data.frame", "b")))),
                      c("data.frame", "b")),
            inherits(base::as.data.frame(as.Date("2020-01-01"))[[1L]], "Date"),
            inherits(base::as.data.frame(as.POSIXlt("2020-01-01", tz="UTC"))[[1L]], "POSIXct"),
            inherits(base::as.data.frame(as.POSIXct("2020-01-01", tz="UTC"))[[1L]], "POSIXct"),
            identical(base::as.data.frame(as.raw(1:2))[[1L]], as.raw(1:2)),
            identical(base::as.data.frame(c(TRUE, FALSE))[[1L]], c(TRUE, FALSE)),
            identical(base::as.data.frame(c(1.5, 2.5))[[1L]], c(1.5, 2.5)),
            identical(base::as.data.frame(1+2i)[[1L]], 1+2i)
        )
        "#,
    );
}

#[test]
fn as_data_frame_builds_list_matrix_and_table_columns() {
    check(
        &mut context(),
        r#"
        empty <- base::as.data.frame(list(), row.names=c("i", "j"))
        x <- list("a b"=1:2, label=c("b", "a"))
        m <- matrix(c("a", "b", "a", "c"), 2,
                    dimnames=list(c("r", "r"), c("x y", "x y")))
        t <- structure(c(1L, 1L, 0L, 1L), dim=c(2L, 2L),
                       dimnames=list(x=c("a", "b"), y=c("u", "v")), class="table")
        f <- base::as.data.frame(m)
        tf <- base::as.data.frame(t)
        a <- array(1:8, c(2, 2, 2), dimnames=list(c("r", "s"), c("A", "B"), c("x", "y")))
        compact <- structure(list(x=1:3), class="data.frame", row.names=c(NA_integer_, 3L))
        checks <- c(
            identical(names(empty), character()),
            identical(dim(empty), c(2L, 0L)),
            identical(dim(base::as.data.frame(NULL)), c(0L, 0L)),
            identical(names(base::as.data.frame(x)), c("a.b", "label")),
            identical(names(base::as.data.frame(x, optional=TRUE)), c("a b", "label")),
            identical(names(base::as.data.frame(x, col.names=c("d e", "f g"))), c("d.e", "f.g")),
            identical(names(base::as.data.frame(list("Long label text"=1:2), cut.names=10)),
                      "Long...."),
            identical(names(base::as.data.frame(list(1:2, 3:4), optional=TRUE)), c("1:2", "3:4")),
            identical(names(base::as.data.frame(list(1:2, 3:4), fix.empty.names=FALSE)), c("", "")),
            identical(names(base::as.data.frame(list(a=data.frame(b=1:2)))), "b"),
            identical(names(base::as.data.frame(list(a=data.frame(b=1:2, c=3:4)))), c("a.b", "a.c")),
            identical(attr(base::as.data.frame(list(compact)), "row.names"), 1:3),
            identical(base::as.data.frame(x)$label, c("b", "a")),
            is.factor(base::as.data.frame(x, stringsAsFactors=TRUE)$label),
            identical(attr(base::as.data.frame(x, row.names="label"), "row.names"), c("b", "a")),
            identical(base::as.data.frame(list(a=1:2, b=3L))$b, c(3L, 3L)),
            inherits(base::as.data.frame(list(a=1:2, b=as.Date("2020-01-01")))$b, "Date"),
            identical(names(f), c("x y", "x y")),
            identical(attr(f, "row.names"), c("r", "r.1")),
            identical(names(base::as.data.frame(a)), c("A.x", "B.x", "A.y", "B.y")),
            identical(attr(base::as.data.frame(a), "row.names"), c("r", "s")),
            identical(attr(base::as.data.frame(m, make.names=NA), "row.names"), c(1L, 2L)),
            identical(f[[2L]], c("a", "c")),
            is.factor(base::as.data.frame(m, stringsAsFactors=TRUE)[[1L]]),
            is.null(names(base::as.data.frame(matrix(1:4, 2), optional=TRUE))),
            identical(names(base::as.data.frame(matrix(1:4, 2, dimnames=list(NULL, c("", ""))),
                                                optional=TRUE)), c("V1", "V2")),
            identical(names(tf), c("x", "y", "Freq")),
            identical(as.character(tf$x), c("a", "b", "a", "b")),
            identical(as.character(tf$y), c("u", "u", "v", "v")),
            identical(tf$Freq, c(1L, 1L, 0L, 1L)),
            identical(base::as.data.frame(t, stringsAsFactors=FALSE, responseName="n")$x,
                      c("a", "b", "a", "b"))
        )
        "#,
    );
}

#[test]
fn character_comparison_and_concatenation_preserve_encoding() {
    check(
        &mut context(),
        r#"
        latin <- rawToChar(as.raw(233))
        Encoding(latin) <- "latin1"
        utf <- rawToChar(as.raw(c(195, 169)))
        Encoding(utf) <- "UTF-8"
        next_utf <- rawToChar(as.raw(c(195, 170)))
        Encoding(next_utf) <- "UTF-8"
        bytes <- latin
        Encoding(bytes) <- "bytes"
        labels <- names(c(setNames(1, latin), setNames(2, utf), setNames(3, bytes)))
        qualified <- names(c(prefix=setNames(1, latin)))
        numbered <- names(c(setNames(list(1:2), latin), recursive=TRUE))
        checks <- c(
            latin == utf, !(latin != utf), !(latin < utf), latin <= utf, !(latin > utf), latin >= utf,
            latin < next_utf, next_utf > latin,
            identical(c(latin, NA_character_) == utf, c(TRUE, NA)),
            latin != bytes, bytes == bytes, bytes <= bytes,
            identical(tryCatch(bytes < latin, error=conditionMessage),
                      'translating strings with "bytes" encoding is not allowed'),
            identical(tryCatch(c(prefix=setNames(1, bytes)), error=conditionMessage),
                      'translating strings with "bytes" encoding is not allowed'),
            identical(Encoding(labels), c("latin1", "UTF-8", "bytes")),
            identical(charToRaw(labels[[1L]]), as.raw(233)),
            identical(charToRaw(labels[[2L]]), as.raw(c(195, 169))),
            identical(Encoding(qualified), "UTF-8"),
            identical(charToRaw(qualified), c(charToRaw("prefix."), as.raw(c(195, 169)))),
            identical(Encoding(numbered), c("UTF-8", "UTF-8")),
            identical(charToRaw(numbered[[2L]]), as.raw(c(195, 169, 50))),
            identical(names(c(setNames(1, NA_character_), q=setNames(2, NA_character_))),
                      c(NA_character_, "q.NA")),
            is.null(names(c(setNames(1, bytes), use.names=FALSE)))
        )
        "#,
    );
}

#[test]
fn arithmetic_and_logic() {
    check(
        &mut context(),
        r#"
        checks <- c(
            identical(1 + 2i + (3 - 4i), 4 - 2i),
            identical((1 + 2i) ^ 2L, -3 + 4i),
            identical(NA_real_ ^ 0, 1),
            identical(-5L %/% 2L, -3L),
            identical(-5L %% 2L, 1L),
            identical(2147483647L + 1L, NA_integer_),
            identical(sum(2147483647L, 1L), 2147483648),
            identical(cumprod(c(a = 2L, b = 3L)), c(a = 2, b = 6)),
            identical(cumsum(c(NaN, NA_real_)), c(NaN, NA_real_)),
            identical(round(c(0.5, 1.5, 2.5, -1.5)), c(0, 2, 2, -2)),
            identical(NA & FALSE, FALSE),
            identical(NA | TRUE, TRUE),
            identical(NA && FALSE, FALSE),
            identical(NA || TRUE, TRUE),
            identical(FALSE && undefined, FALSE),
            identical(TRUE || undefined, TRUE),
            identical(NA %||% undefined, NA),
            identical(NULL %||% 42L, 42L))
    "#,
    );
}

#[test]
fn indices_and_copy_on_modify() {
    let mut ctx = context();
    ctx.opts_mut().warn_partial_match_dollar = true;
    check(
        &mut ctx,
        r#"
        x <- list(items = c(a = 4L, b = 8L), score = 3L)
        before <- x
        updated <- `[[<-`(x, "items", `[<-`(x$items, 2L, 99))
        m <- matrix(1:6, nrow = 2L, dimnames = list(c("a","b"), c("x","y","z")))
        coordinates <- rbind(c(2L,3L), c(0L,1L), c(NA_integer_,2L), c(1L,2L))
        checks <- c(
            identical(x, before),
            identical(x$items, c(a = 4L, b = 8L)),
            identical(updated$items, c(a = 4, b = 99)),
            identical(`$<-`(x, score, NULL), list(items = x$items)),
            identical(x$sco, 3L),
            identical(x[[c("items", "b")]], 8L),
            identical(x[[NA_integer_]], NULL),
            identical(x$items[c(2L, 0L, NA_integer_)],
                structure(c(8L, NA_integer_), names = c("b", NA_character_))),
            identical(m[coordinates], c(6L, NA_integer_, 3L)),
            identical(m["b", ], c(x = 2L, y = 4L, z = 6L)),
            identical(dim(m["b", , drop = FALSE]), c(1L, 3L)),
            identical(`[<-`(c(a = 1L), 3L, 4),
                structure(c(1, NA_real_, 4), names = c("a", "", ""))))
    "#,
    );
}

#[test]
fn containers_and_callbacks() {
    check(
        &mut context(),
        r#"
        x <- c(a = -3L, b = 2L)
        run <- rle(c(4L, 4L, 8L, NA_integer_, NA_integer_))
        group <- factor(c("low", "high", NA_character_), levels = c("high", "low"))
        frame <- data.frame(group = group, score = c(4L, 8L, 12L))
        checks <- c(
            identical(c(list(pair = c(left = 4L, right = 8L)), end = 12L, recursive = TRUE),
                c(pair.left = 4L, pair.right = 8L, end = 12L)),
            identical(rep(x, times = c(1L, 3L)), c(a = -3L, b = 2L, b = 2L, b = 2L)),
            identical(seq.int(from = 8L, to = 2L, by = -3L), c(8L, 5L, 2L)),
            identical(order(c(2L, NA_integer_, 1L)), c(3L, 1L, 2L)),
            identical(as.character(frame[2:1, "group"]), c("high", "low")),
            identical(dim(frame[, "score", drop = FALSE]), c(3L, 1L)),
            identical(inverse.rle(run), c(4L, 4L, 8L, NA_integer_, NA_integer_)),
            identical(lapply(x, abs), list(a = 3L, b = 2L)),
            identical(sapply(x, abs), c(a = 3L, b = 2L)),
            identical(vapply(x, abs, numeric(1)), c(a = 3, b = 2)),
            identical(Reduce(sum, x, init = 10L, accumulate = TRUE), c(10L, 7L, 9L)),
            identical(Filter(abs, c(0L, -1L, 3L)), c(-1L, 3L)),
            identical(do.call("paste", list("north", "east", sep = "-")), "north-east"))
    "#,
    );
}

#[test]
fn rbind_recycles_a_vector_next_to_a_named_matrix() {
    check(
        &mut context(),
        r#"
        m <- matrix(1:6, nrow = 2L, dimnames = list(c("a", "b"), c("x", "y", "z")))
        warnings <- character()
        result <- withCallingHandlers(rbind(m, row = c(7L, 8L)), warning = function(w) {
            warnings <<- c(warnings, conditionMessage(w))
            invokeRestart("muffleWarning")
        })
        checks <- c(
            identical(result, matrix(c(1L, 2L, 7L, 3L, 4L, 8L, 5L, 6L, 7L), nrow = 3L,
                dimnames = list(c("a", "b", "row"), c("x", "y", "z")))),
            identical(warnings, "number of columns of result is not a multiple of vector length (arg 2)"))
        "#,
    );
}

#[test]
fn replacement_and_language_values() {
    let mut ctx = context();
    let s = ctx.session_mut();
    let star = s.intern("*").expect("operator").as_sexp();
    let weight = s.intern("weight").expect("symbol").as_sexp();
    let count = s.intern("count").expect("symbol").as_sexp();
    let nil = roxide_value::SEXP_NIL;
    let rhs = s.cons(count, nil, nil).expect("right operand");
    let lhs = s.cons(weight, rhs, nil).expect("left operand");
    let call = s.lcons(star, lhs, nil).expect("language object");
    let key = s.intern("quoted_call").expect("name");
    s.env_define(GLOBAL_ENV, key, call).expect("bind call");

    check(
        &mut ctx,
        r#"
        x <- factor(c("a", "b", "a"))
        y <- `[<-`(x, 1:2, c("b", "c"))
        df <- data.frame(a = 1:2, b = 3:4)
        df1 <- `[<-`(df, 2L, 2L, 9L)
        df2 <- `$<-`(df1, score, 7L)
        checks <- c(
            identical(seq.int(from = 4L, along.with = quoted_call), 4:6),
            identical(pairlist(x = 4L)[["x"]], 4L),
            identical(as.character(x), c("a", "b", "a")),
            identical(as.character(y), c("b", NA_character_, "a")),
            identical(as.character(`levels<-`(x, c("z", "z"))), c("z", "z", "z")),
            identical(df1$b, c(3L, 9L)),
            identical(df2$score, c(7L, 7L)),
            identical(dimnames(df2), list(c("1", "2"), c("a", "b", "score"))),
            identical(vapply(list(a = -3L), abs, integer(1), USE.NAMES = FALSE), 3L),
            identical(sapply(list(x = c(a = -2L)), abs), c(x.a = 2L)),
            identical(sort(c(2+2i, 2+1i, 1+3i)), c(1+3i, 2+1i, 2+2i)),
            identical(round(c(0.15, 2.675), 2), c(0.15, 2.67)),
            identical(paste(character()), character()))
    "#,
    );
}

#[test]
fn factor_comparisons_and_base_helpers() {
    let mut ctx = roxide_eval::Ctx::new(
        Box::new(roxide_host::PureHost::new()),
        roxide_gc::GcPolicy::Torture,
    )
    .expect("Create the R session");
    roxide_base::register_all(&mut ctx).expect("Register base");
    ctx.run_script(
        "Factor comparisons and helpers",
        r#"
f <- factor(c("a", "c", NA))
stopifnot(identical(f != "c", c(TRUE,FALSE,NA)),
          identical("a" == f, c(TRUE,FALSE,NA)),
          identical(f == factor(c("a", "c", NA), levels = c("c", "a")), c(TRUE,TRUE,NA)))
err <- tryCatch(f == factor(c("a","b","c")), error = conditionMessage)
stopifnot(identical(err, "level sets of factors are different"))
warn <- tryCatch(f < "c", warning = conditionMessage)
stopifnot(identical(warn, paste0("‘", "<", "’ not meaningful for factors")))
x <- structure(1:3, class = "AsIs")
stopifnot(inherits(x[2L], "AsIs"))
y <- 1:3; x <- 4:6
stopifnot(identical(colnames(cbind(y, x)), c("y", "x")),
          identical(colnames(cbind(y = y, x = x)), c("y", "x")))
a <- matrix(1:4, 2); colnames(a) <- c("y","x")
stopifnot(identical(colnames(a), c("y","x")), identical(as.vector(a), 1:4),
          identical(as.vector(f), c("a", "c", NA_character_)),
          identical(signif(c(123.456, 0.012345), 3), c(123, 0.0123)),
          identical(all.vars(quote(y ~ f(x) + x + z)), c("y","x","z")),
          identical(environmentName(baseenv()), "base"))
"#,
        roxide_value::GLOBAL_ENV,
        false,
    )
    .expect("Check factor comparisons and helpers");
}
