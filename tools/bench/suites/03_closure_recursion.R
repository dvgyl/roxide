set.seed(42)

fib <- function(n) {
    if (n < 2L) {
        return(n)
    }
    fib(n - 1L) + fib(n - 2L)
}

collatz_steps <- function(n) {
    steps <- 0L
    while (n > 1) {
        if (n %% 2 == 0) {
            n <- n / 2
        } else {
            n <- 3 * n + 1
        }
        steps <- steps + 1L
    }
    steps
}

make_polynomial <- function(a, b, c) {
    function(x) (a * x + b) * x + c
}

polynomial <- make_polynomial(0.5, -2, 3)
step_total <- 0
polynomial_total <- 0
for (i in 1:2000) {
    step_total <- step_total + collatz_steps(i)
    polynomial_total <- polynomial_total + polynomial(i / 16)
}

cat("RESULT:", sprintf("%d,%.0f,%.9f", fib(19L), step_total, polynomial_total),
    "\n", sep = "")
