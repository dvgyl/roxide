//! Check grouped statistics with a collection at each allocation.

#![forbid(unsafe_code)]

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::{Limits, NativeHost, PureHost};
use roxide_value::GLOBAL_ENV;

#[test]
fn grouped_statistics_survive_gc_torture() {
    let mut ctx =
        Ctx::new(Box::new(PureHost::new()), GcPolicy::default()).expect("Create the evaluator");
    roxide_base::register_all(&mut ctx).expect("Register base");
    roxide_stats::register(&mut ctx).expect("Register stats");
    ctx.session_mut().heap.set_policy(GcPolicy::Torture);
    ctx.run_script(
        "Grouped statistics",
        r#"
d <- data.frame(a = 1:4, b = c(1L, NA, 5L, 7L),
                group = factor(c("z", "a", "z", "a"), levels = c("z", "u", "a")))
out <- aggregate(. ~ group, d, sum)
stopifnot(identical(out, data.frame(group = d$group[1:2], a = c(4L, 4L), b = c(6L, 7L))))
out <- aggregate(cbind(a, b) ~ group, d, sum, subset = a > 1L, na.action = na.pass)
stopifnot(identical(out$a, c(3L, 6L)), identical(out$b, c(5L, NA_integer_)))
out <- aggregate(d["a"], list(group = d$group),
                 function(x) c(low = min(x), high = max(x)), drop = FALSE)
stopifnot(identical(out$a, matrix(c(1L, NA, 2L, 3L, NA, 4L), 3L, 2L,
                                  dimnames = list(NULL, c("low", "high")))))
stopifnot(identical(ave(d$a, d$group, FUN = sum), c(4L, 6L, 4L, 6L)))
stopifnot(identical(levels(relevel(d$group, "a")), c("a", "z", "u")))
ordered <- reorder(d$group, d$a)
stopifnot(identical(levels(ordered), c("z", "a", "u")))
stopifnot(identical(as.vector(attr(ordered, "scores")), c(2, NA, 3)))
"#,
        GLOBAL_ENV,
        false,
    )
    .expect("Check grouped statistics");
}

#[test]
fn aggregate_limits_the_size_of_empty_combinations() {
    let host = NativeHost::with_limits(Limits {
        max_vector_bytes: 4_096,
        ..Limits::default()
    });
    let mut ctx = Ctx::new(Box::new(host), GcPolicy::default()).expect("Create the evaluator");
    roxide_base::register_all(&mut ctx).expect("Register base");
    roxide_stats::register(&mut ctx).expect("Register stats");
    ctx.run_script(
        "Aggregate size limit",
        r#"
group <- factor("1", levels = as.character(1:100))
keys <- list(group, group, group, group, group)
stopifnot(identical(aggregate(1L, keys, sum)$x, 1L))
message <- tryCatch(aggregate(1L, keys, sum, drop = FALSE), error = conditionMessage)
stopifnot(identical(message, "vector size limit exceeded"))
"#,
        GLOBAL_ENV,
        false,
    )
    .expect("Check the size limit");
}
