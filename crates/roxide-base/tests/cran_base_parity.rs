#![forbid(unsafe_code)]

//! Exercise CRAN package helpers with garbage collection on each allocation.

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::PureHost;
use roxide_value::GLOBAL_ENV;

fn run(source: &str) {
    let mut ctx = Ctx::new(Box::new(PureHost::new()), GcPolicy::Torture).expect("create evaluator");
    roxide_base::register_all(&mut ctx).expect("register base");
    ctx.run_script("CRAN base parity", source, GLOBAL_ENV, false)
        .expect("run base checks");
}

#[test]
fn call_replacement_keeps_language_and_argument_tags() {
    run(r#"
        x <- quote(f(x=1L, y=2L))
        original <- x
        x[[1L]] <- as.name("g")
        x["x"] <- list(7L)
        x$next.arg <- 8L
        x[["y"]] <- NULL
        stopifnot(identical(x, quote(g(x=7L, next.arg=8L))),
                  identical(original, quote(f(x=1L, y=2L))),
                  identical(typeof(x[-1L]), "language"),
                  identical(x[["next.arg"]], 8L),
                  is.null(x[integer()]))
        "#);
}

#[test]
fn sets_format_and_nested_callbacks_keep_intermediate_values() {
    run(r#"
        x <- list(a=1:2, b=list(c=3L, 4L), 5L)
        stopifnot(identical(unlist(x), structure(1:5, names=c("a1","a2","b.c","b",""))),
                  identical(match(c(NA, NaN, 0), c(NaN, NA, -0)), c(2L,1L,3L)),
                  identical(Map(paste0, c("a","b"), 1:2), list(a="a1", b="b2")),
                  identical(format(c(123456,1), big.mark=","), c("123,456", "      1")))
        "#);
}

#[test]
fn pure_host_environment_keeps_missing_unset_values() {
    run(r#"
        Sys.unsetenv("ROXIDE_CRAN_BASE_UNSET")
        stopifnot(identical(Sys.getenv("ROXIDE_CRAN_BASE_UNSET", unset=NA), NA_character_),
                  identical(class(Sys.getenv()), "Dlist"),
                  identical(class(Sys.getenv(names=FALSE)), "character"))
        "#);
}

#[test]
fn primitive_subsets_and_formal_defaults_survive_collection() {
    run(r#"
        `[.R6` <- function(x, ...) stop("unexpected dispatch")
        `[[.R6` <- function(x, ...) stop("unexpected dispatch")
        env <- structure(new.env(), class = "R6")
        env$value <- 42L
        v <- structure(1:3, class = "R6")
        stopifnot(identical(.subset2(env, "value"), 42L),
                  is.null(.subset2(env, "val", exact = FALSE)),
                  identical(.subset(v, 2), 2L),
                  identical(.subset2(v, 2), 2L))

        f <- function(x, y = 1L) y
        formals(f)[["y"]] <- 2L
        formals(f)$z <- 3L
        stopifnot(identical(formals(f), as.pairlist(alist(x = , y = 2L, z = 3L))),
                  identical(f(), 2L))
        formals(f)$y <- NULL
        stopifnot(identical(formals(f), as.pairlist(alist(x = , z = 3L))))
        formals(f)["y"] <- list(4L)
        stopifnot(identical(formals(f), as.pairlist(alist(x = , z = 3L, y = 4L))),
                  identical(formals(f)["y"], list(y = 4L)))
        "#);
}

#[test]
fn comparisons_and_data_frame_masks_survive_collection() {
    run(r#"
        stopifnot(identical(quote(a) == c(a = "a", b = "b"), c(a = TRUE, b = FALSE)),
                  identical(quote(f(1L)) == "f(1L)", TRUE),
                  identical(quote(x) != quote(y), TRUE),
                  identical(quote(a) == NULL, logical(0)),
                  identical(quote(a) == NA_character_, NA),
                  identical(tryCatch(quote(f(1)) < "z", error = conditionMessage),
                            "comparison (<) is not possible for language types"))
        d <- data.frame(a = 1:3, b = 4:6, c = 7:9)
        out <- local({offset <- 1L; transform(d, a = a + offset, b = a * 2L, z = c, c = NULL)})
        stopifnot(identical(names(out), c("a", "b", "z")), identical(out$a, 2:4),
                  identical(out$b, c(2L, 4L, 6L)), identical(out$z, 7:9))
        out <- local({cutoff <- 1L; subset(d, a > cutoff, select = -b)})
        stopifnot(identical(names(out), c("a", "c")), identical(out$a, 2:3),
                  identical(subset(d, a > 1L, select = a, drop = TRUE), 2:3))
        "#);
}

#[test]
fn persistent_sinks_keep_connections_and_local_text_bindings_alive() {
    run(r#"
        open_text <- function() textConnection("local_lines", "w", local = TRUE)
        con <- open_text()
        sink(con)
        cat("outer\n")
        raw_out <- rawConnection(raw(), "w")
        sink(raw_out, split = TRUE)
        cat("both\n")
        stopifnot(identical(sink.number(), 2L))
        sink()
        stopifnot(identical(rawConnectionValue(raw_out), charToRaw("both\n")))
        close(raw_out)
        sink()
        stopifnot(identical(textConnectionValue(con), c("outer", "both")), isOpen(con))

        sink(con, type = "message")
        stopifnot(identical(sink.number("message"), as.integer(con)))
        message("message")
        sink(type = "message")
        stopifnot(identical(sink.number("message"), 2L), identical(sink.number(), 0L),
                  identical(textConnectionValue(con), c("outer", "both", "message")))
        close(con)
        "#);
}
