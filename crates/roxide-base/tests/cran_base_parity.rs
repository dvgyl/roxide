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
