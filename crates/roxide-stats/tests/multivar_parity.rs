//! Exercise multivariate methods under garbage collection and vector limits.

#![forbid(unsafe_code)]

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::{Host, Limits, NativeHost, PureHost};
use roxide_value::GLOBAL_ENV;

fn check(host: Box<dyn Host>, script: &str) {
    let mut ctx = Ctx::new(host, GcPolicy::Torture).expect("Create session");
    roxide_base::register_all(&mut ctx).expect("Register base");
    roxide_stats::register(&mut ctx).expect("Register stats");
    if let Err(error) = ctx.run_script("Multivariate parity", script, GLOBAL_ENV, false) {
        panic!("{error:?}");
    }
}

#[test]
fn multivariate_results_survive_collection_and_callbacks() {
    check(
        Box::new(PureHost::new()),
        r#"
x <- rbind(a=c(0,0), b=c(0,2), c=c(10,0), d=c(10,2))
dimnames(x) <- list(c("a","b","c","d"), c("u","v"))
d <- dist(x)
stopifnot(identical(attr(d, "Labels"), c("a","b","c","d")))
stopifnot(identical(attr(as.dist(as.matrix(d)), "Labels"), c("a","b","c","d")))
warnings <- 0L
withCallingHandlers(dist(rbind(c(Inf, 0), c(1, 0)), "binary"), warning = function(w) {
    warnings <<- warnings + 1L
    invokeRestart("muffleWarning")
})
stopifnot(identical(warnings, 1L))
tree <- hclust(d)
stopifnot(identical(as.integer(cutree(tree, 2L)), c(1L,1L,2L,2L)))
fit <- kmeans(x, x[c(1,3),])
stopifnot(identical(fitted(fit, "classes"), fit$cluster))
p <- prcomp(x, scale. = TRUE)
stopifnot(max(abs(predict(p, x[, 2:1]) - p$x)) < 1e-12)
q <- princomp(x)
stopifnot(max(abs(predict(q, x) - q$scores)) < 1e-12)
stopifnot(max(abs(cmdscale(d, eig = TRUE)$GOF - 1)) < 1e-12)
"#,
    );
}

#[test]
fn oversized_distances_fail_before_allocation() {
    let limits = Limits {
        max_vector_bytes: 65536,
        ..Limits::default()
    };
    check(
        Box::new(NativeHost::with_limits(limits)),
        r#"
size_error <- function(expr) tryCatch(expr, error = function(e) conditionMessage(e))
stopifnot(identical(size_error(dist(matrix(0, 200, 1))), "vector size limit exceeded"))
d <- structure(numeric(), Size = 200L, class = "dist")
stopifnot(identical(size_error(hclust(d)), "vector size limit exceeded"))
stopifnot(identical(size_error(as.matrix(d)), "vector size limit exceeded"))
"#,
    );
}

#[test]
fn kmeans_uses_the_base_rng_stream() {
    check(
        Box::new(PureHost::new()),
        r#"
x <- matrix(c(0, 0, 1, 9, 10, 11, 0, 1, 0, 9, 9, 10), 6)
set.seed(123)
kmeans(x, 2L, nstart = 3L, algorithm = "Lloyd")
actual_seed <- .Random.seed
set.seed(123)
for (i in 1:3) sample.int(nrow(x), 2L)
stopifnot(identical(.Random.seed, actual_seed))
"#,
    );
}

#[test]
fn live_transfers_and_additive_scaling() {
    check(
        Box::new(PureHost::new()),
        r#"
x <- rbind(c(1,-1),c(2,-3),c(2,-2),c(-2,-1),c(3,-3),c(1,-2),c(0,2),c(0,2),c(-2,0),c(1,1))
fit <- kmeans(x, rbind(c(3,-3),c(2,-3),c(2,-2)))
stopifnot(abs(fit$tot.withinss - 7.43333333333333) < 1e-12, fit$iter == 3L)
d <- as.dist(rbind(c(0,1,1),c(1,0,3),c(1,3,0)))
stopifnot(identical(dim(cmdscale(d, add=TRUE)$points), c(3L,2L)))
d <- structure(numeric(), class="dist", Size=-1L)
stopifnot(identical(tryCatch(cmdscale(d), error=conditionMessage), "invalid 'nrow' value (< 0)"))
"#,
    );
}
