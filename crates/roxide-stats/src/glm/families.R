.glm_expression <- function(expr) .Internal(C_glm_initializer(substitute(expr)))

family <- function(object, ...) UseMethod("family")
family.glm <- function(object, ...) object[["family"]]
family.lm <- function(object, ...) gaussian()
family.default <- function(object, ...) {
    if (is.list(object) && !is.null(object[["family"]])) return(object[["family"]])
    stop("no applicable method for 'family'")
}

print.family <- function(x, ...) {
    cat("\nFamily:", x$family, "\n")
    cat("Link function:", x$link, "\n\n")
    invisible(x)
}

.glm_unit_log <- function(y, mu) {
    size <- max(length(y), length(mu))
    y <- rep(y, length.out = size)
    mu <- rep(mu, length.out = size)
    out <- y + mu
    keep <- !is.na(y) & y != 0
    out[keep] <- y[keep] * log(y[keep] / mu[keep])
    out[!is.na(y) & y == 0] <- 0
    out
}

.glm_family_parts <- function(kind, quasi = FALSE) {
    options <- list(
        constant = list(
            variance = function(mu) rep(1, length(mu)),
            dev.resids = function(y, mu, wt) wt * (y - mu)^2,
            validmu = function(mu) TRUE),
        mu = list(
            variance = function(mu) mu,
            dev.resids = function(y, mu, wt) {
                value <- 2 * wt * (.glm_unit_log(y, mu) + (mu - y))
                w <- rep(wt, length.out = length(value))
                value[!is.na(w) & w == 0] <- 0
                value
            },
            validmu = if (quasi) function(mu) all(mu > 0)
                      else function(mu) all(is.finite(mu)) && all(mu > 0)),
        "mu^2" = list(
            variance = function(mu) mu^2,
            dev.resids = function(y, mu, wt) {
                ratio <- y / mu
                y <- rep(y, length.out = length(ratio))
                zero <- !is.na(y) & y == 0
                ratio[zero] <- 1
                value <- 2 * wt * ((y - mu) / mu - log(ratio))
                if (quasi) value[rep(zero, length.out = length(value))] <- 0
                value
            },
            validmu = if (quasi) function(mu) all(mu > 0)
                      else function(mu) all(is.finite(mu)) && all(mu > 0)),
        "mu^3" = list(
            variance = function(mu) mu^3,
            dev.resids = function(y, mu, wt) wt * (y - mu)^2 / (y * mu^2),
            validmu = if (quasi) function(mu) all(mu > 0) else function(mu) TRUE),
        "mu(1-mu)" = list(
            variance = function(mu) mu * (1 - mu),
            dev.resids = function(y, mu, wt)
                2 * wt * (.glm_unit_log(y, mu) + .glm_unit_log(1 - y, 1 - mu)),
            validmu = if (quasi) function(mu) all(mu > 0 & mu < 1)
                      else function(mu) all(is.finite(mu)) && all(mu > 0 & mu < 1)))
    options[[kind]]
}

.glm_make_family <- function(name, link, parts, initialize, aic = function(y, n, mu, wt, dev) NA_real_,
                             simulate = NULL, varfun = NULL, dispersion = NA_real_) {
    ans <- list(family = name, link = link$name, linkfun = link$linkfun, linkinv = link$linkinv,
                variance = parts$variance, dev.resids = parts$dev.resids, aic = aic,
                mu.eta = link$mu.eta, initialize = initialize, validmu = parts$validmu,
                valideta = link$valideta)
    if (!is.null(simulate)) ans$simulate <- simulate
    if (!is.null(varfun)) ans$varfun <- varfun
    ans$dispersion <- dispersion
    structure(ans, class = "family")
}

gaussian <- function(link = "identity") {
    link <- .glm_resolve_link(link, substitute(link), "gaussian")
    .glm_make_family("gaussian", link, .glm_family_parts("constant"),
        .glm_expression({
            n <- rep(1, nobs)
            if (is.null(etastart) && is.null(start) && is.null(mustart)) {
                if (family$link == "inverse" && any(y == 0))
                    stop("cannot find valid starting values: please specify some")
                if (family$link == "log" && any(y <= 0))
                    stop("cannot find valid starting values: please specify some")
            }
            mustart <- y
        }),
        aic = function(y, n, mu, wt, dev) {
            count <- length(y)
            count * (log(2 * 3.141592653589793 * dev / count) + 1) + 2 - sum(log(wt))
        })
}

.glm_binomial_start <- function(label) substitute({
    columns <- if (is.matrix(y)) ncol(y) else 1L
    if (columns == 1L) {
        if (is.factor(y)) y <- as.double(as.integer(y) != 1L)
        if (is.matrix(y)) y <- y[, 1L]
        n <- rep(1, nobs)
        y[weights == 0] <- 0
        if (any(y < 0 | y > 1)) stop("y values must be 0 <= y <= 1")
        mustart <- (weights * y + 0.5) / (weights + 1)
        successes <- weights * y
        if (family_name == "binomial" && any(abs(successes - round(successes)) > 0.001))
            warning("non-integer #successes in a binomial glm!")
    } else if (columns == 2L) {
        if (family_name == "binomial" && any(abs(y - round(y)) > 0.001))
            warning("non-integer counts in a binomial glm!")
        n <- y[, 1L] + y[, 2L]
        y <- y[, 1L] / n
        y[n == 0] <- 0
        weights <- weights * n
        mustart <- (n * y + 0.5) / (n + 1)
    } else {
        stop(paste0("for the '", family_name, "' family, y must be a vector of 0 and 1's\n",
                    "or a 2 column matrix where col 1 is no. successes and col 2 is no. failures"))
    }
}, list(family_name = label))

.glm_sim_binomial <- function(object, nsim) {
    w <- object$prior.weights
    mu <- object$fitted.values
    n <- length(mu)
    response <- model.response(model.frame(object))
    if (any(w != round(w))) stop("cannot simulate from non-integer prior.weights")
    if (is.factor(response)) {
        values <- rbinom(n * nsim, 1, mu)
        out <- lapply(seq_len(nsim), function(i) {
            indices <- (i - 1L) * n + seq_len(n)
            factor(values[indices], 0:1, levels(response))
        })
        return(setNames(out, as.character(seq_len(nsim))))
    }
    if (is.matrix(response) && ncol(response) == 2L) {
        return(lapply(seq_len(nsim), function(i) {
            successes <- rbinom(n, w, mu)
            matrix(c(successes, w - successes), n, 2L, dimnames = dimnames(response))
        }))
    }
    rbinom(n * nsim, w, mu) / w
}

binomial <- function(link = "logit") {
    link <- .glm_resolve_link(link, substitute(link), "binomial")
    .glm_make_family("binomial", link, .glm_family_parts("mu(1-mu)"), .glm_binomial_start("binomial"),
        aic = function(y, n, mu, wt, dev) {
            trials <- if (any(n > 1)) n else wt
            keep <- trials > 0
            -2 * sum(wt[keep] / trials[keep] *
                     dbinom(round(trials[keep] * y[keep]), round(trials[keep]), mu[keep], log = TRUE))
        }, simulate = .glm_sim_binomial, dispersion = 1)
}

quasibinomial <- function(link = "logit") {
    link <- .glm_resolve_link(link, substitute(link), "quasibinomial")
    .glm_make_family("quasibinomial", link, .glm_family_parts("mu(1-mu)"), .glm_binomial_start("quasibinomial"))
}

poisson <- function(link = "log") {
    link <- .glm_resolve_link(link, substitute(link), "poisson")
    .glm_make_family("poisson", link, .glm_family_parts("mu"),
        .glm_expression({
            if (any(y < 0)) stop("negative values not allowed for the 'Poisson' family")
            n <- rep(1, nobs)
            mustart <- y + 0.1
        }), aic = function(y, n, mu, wt, dev) -2 * sum(wt * dpois(y, mu, log = TRUE)),
        simulate = function(object, nsim) {
            if (any(object$prior.weights != 1)) warning("ignoring prior weights")
            mu <- object$fitted.values
            rpois(length(mu) * nsim, mu)
        }, dispersion = 1)
}

quasipoisson <- function(link = "log") {
    link <- .glm_resolve_link(link, substitute(link), "quasipoisson")
    .glm_make_family("quasipoisson", link, .glm_family_parts("mu"),
        .glm_expression({
            if (any(y < 0)) stop("negative values not allowed for the 'quasiPoisson' family")
            n <- rep(1, nobs)
            mustart <- y + 0.1
        }))
}

.glm_gamma_simulate <- function(object, nsim) {
    mu <- object$fitted.values
    weights <- object$prior.weights
    if (any(weights != 1)) message("using weights as shape parameters")
    .Internal(C_lm_size(as.double(length(mu)) * nsim))
    if (object$deviance <= 0) return(rep(mu, nsim))
    if (any(weights == 0)) {
        warning("NaNs produced")
        stop("missing value where TRUE/FALSE needed")
    }
    shape <- .Internal(C_glm_gamma_shapes(weights, object$deviance))
    rgamma(length(mu) * nsim, shape, scale = mu / shape)
}

Gamma <- function(link = "inverse") {
    link <- .glm_resolve_link(link, substitute(link), "Gamma")
    .glm_make_family("Gamma", link, .glm_family_parts("mu^2"),
        .glm_expression({
            if (any(y <= 0)) stop("non-positive values not allowed for the 'Gamma' family")
            n <- rep(1, nobs)
            mustart <- y
        }), aic = function(y, n, mu, wt, dev) {
            dispersion <- dev / sum(wt)
            -2 * sum(wt * dgamma(y, shape = 1 / dispersion, scale = mu * dispersion, log = TRUE)) + 2
        }, simulate = .glm_gamma_simulate)
}

inverse.gaussian <- function(link = "1/mu^2") {
    link <- .glm_resolve_link(link, substitute(link), "inverse.gaussian")
    .glm_make_family("inverse.gaussian", link, .glm_family_parts("mu^3"),
        .glm_expression({
            if (any(y <= 0)) stop("positive values only are allowed for the 'inverse.gaussian' family")
            n <- rep(1, nobs)
            mustart <- y
        }), aic = function(y, n, mu, wt, dev) {
            count <- sum(wt)
            count * (log(2 * 3.141592653589793 * dev / count) + 1) + 3 * sum(wt * log(y)) + 2
        }, simulate = function(object, nsim)
            stop("need CRAN package 'SuppDists' for simulation from the 'inverse.gaussian' family"))
}

quasi <- function(link = "identity", variance = "constant") {
    link <- .glm_resolve_link(link, substitute(link), "quasi")
    init <- .glm_expression({
        n <- rep(1, nobs)
        mustart <- y
    })
    code <- substitute(variance)
    kind <- if (is.character(code)) code else gsub(" ", "", deparse1(code), fixed = TRUE)
    if (length(kind) != 1L || !.glm_in(kind, c("constant", "mu", "mu^2", "mu^3", "mu(1-mu)"))) {
        if (is.list(variance))
            return(.glm_make_family("quasi", link, variance, variance$initialize, varfun = variance$name))
        if (!is.character(variance)) stop("$ operator is invalid for atomic vectors")
        stop(paste0("'variance' \"", kind,
                    "\" is invalid: possible values are \"mu(1-mu)\", \"mu\", \"mu^2\", \"mu^3\" and \"constant\""))
    }
    if (kind == "mu(1-mu)") init <- .glm_expression({
        n <- rep(1, nobs)
        mustart <- .glm_ceiling(.glm_floor(y, 0.001), 0.999)
    })
    if (.glm_in(kind, c("mu", "mu^2", "mu^3"))) init <- .glm_expression({
        n <- rep(1, nobs)
        mustart <- y
        mustart[y == 0] <- 0.1
    })
    .glm_make_family("quasi", link, .glm_family_parts(kind, quasi = TRUE), init, varfun = kind)
}
