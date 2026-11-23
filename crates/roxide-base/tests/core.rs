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
