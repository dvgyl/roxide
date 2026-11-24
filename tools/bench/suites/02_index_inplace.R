set.seed(42)

n <- 100000L
x <- numeric(n)
x[1L] <- 1
for (i in 2L:n) {
    x[i] <- x[i - 1L] + i * 0.5
}

cat("RESULT:", sprintf("%.2f,%.2f", sum(x), x[n]), "\n", sep = "")
