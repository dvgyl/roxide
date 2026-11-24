set.seed(42)

draws <- rnorm(16384L)
x <- 0.25 + draws * 1.5
density <- dnorm(x)
probability <- pnorm(x)
ordered <- sort(x)

n <- 1024L
design <- cbind(1, x[1:n], x[(n + 1L):(2L * n)])
response <- 1 + 2 * design[, 2L] - 0.5 * design[, 3L] +
    draws[(2L * n + 1L):(3L * n)]
gram <- t(design) %*% design
fit <- lm.fit(design, response)

checks <- c(sum(x), mean(x), ordered[c(1L, 8192L, 16384L)],
            sum(density), sum(probability), sum(draws * seq_along(draws)),
            gram[1L, 2L], gram[2L, 3L], gram[3L, 3L],
            fit$coefficients, sum(fit$residuals ^ 2))
cat("RESULT:", paste(sprintf("%.6f", checks), collapse = ","), "\n", sep = "")
