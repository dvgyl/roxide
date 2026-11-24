set.seed(42)

score <- function(x, offset) UseMethod("score")
score.linear <- function(x, offset) unclass(x) + offset
score.square <- function(x, offset) unclass(x) * unclass(x) - offset
score.default <- function(x, offset) unclass(x) * offset

objects <- list(structure(3, class = c("special", "linear")),
                structure(5, class = "square"),
                structure(7, class = "other"))
totals <- numeric(3)
for (i in 1:12000) {
    index <- (i - 1L) %% 3L + 1L
    totals[index] <- totals[index] + score(objects[[index]], i * 0.5)
}

cat("RESULT:", paste(sprintf("%.1f", totals), collapse = ","), "\n", sep = "")
