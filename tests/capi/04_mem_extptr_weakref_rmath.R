# SPDX-License-Identifier: MIT OR Apache-2.0
dll <- dyn.load(Sys.getenv("ROXIDE_FIXTURE_SO"))
native <- function(name, ...) .Call(name, ..., PACKAGE = dll[["name"]])
stopifnot(identical(native("fixture_memory"), 42L))
pointer <- native("fixture_pointer", 7L, FALSE)
reference <- native("fixture_weak", pointer, 9L)
stopifnot(identical(native("fixture_weak_fields", reference), list(pointer, 9L)))
invisible(native("fixture_preserve", pointer))
pointer <- NULL
invisible(gc())
stopifnot(identical(native("fixture_pending"), 0L))
local({
    fields <- native("fixture_weak_fields", reference)
    stopifnot(typeof(fields[[1L]]) == "externalptr", identical(fields[[2L]], 9L))
    invisible(native("fixture_release", fields[[1L]]))
})
invisible(gc())
stopifnot(identical(native("fixture_pending"), 1L))
stopifnot(identical(native("fixture_pending"), 1L))
stopifnot(identical(native("fixture_weak_fields", reference), list(NULL, NULL)))

RNGkind("Mersenne-Twister", "Inversion", "Rejection")
prob <- c(0.2, 0.3, 0.5)
set.seed(42)
observed <- c(rnorm(1), native("fixture_random"), runif(1), native("fixture_random"))
counts <- native("fixture_multinomial")
seed <- .Random.seed
set.seed(42)
expected <- c(rnorm(1), rnorm(1), runif(1), runif(1), rnorm(1), runif(1))
expected_counts <- as.vector(rmultinom(1L, 12L, prob))
bits <- function(x) writeBin(x, raw(), size = 8L, endian = "little")
stopifnot(identical(bits(observed), bits(expected)),
          identical(counts, expected_counts), identical(seed, .Random.seed))
cat("RNG bytes", paste(as.integer(bits(observed)), collapse = ","), "\n")
cat("Multinomial counts", counts, "\n")
observed <- native("fixture_math")
expected <- c(dnorm(1.5), pnorm(2, lower.tail = FALSE, log.p = TRUE),
              qnorm(0.975), dgamma(2, 3, scale = 2), dbeta(0.25, 2, 3),
              choose(10, 3), lgamma(4.5))
stopifnot(isTRUE(all.equal(observed, expected, tolerance = 1e-13)))
cat("Math values", sprintf("%.13g", observed), "\n")
stopifnot(identical(native("fixture_sort"), list(c(-3, 2, 8), c(2L, 3L, 1L))))
exit_pointer <- native("fixture_pointer", NULL, TRUE)
cat("Memory, weak reference, and sort checks passed.\n")
