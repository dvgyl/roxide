set.seed(42)

total <- 0
for (i in 1:200000) {
    term <- (3.0 * i - 2.0) / 8.0
    total <- total + term
}

cat("RESULT:", sprintf("%.3f", total), "\n", sep = "")
