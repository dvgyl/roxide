.glm_in <- function(x, choices) !is.na(.model_match(x, choices))

.glm_eps <- 2^-52

.glm_link <- function(name, linkfun, linkinv, mu.eta, valideta = function(eta) TRUE) {
    structure(list(linkfun = linkfun, linkinv = linkinv, mu.eta = mu.eta,
                   valideta = valideta, name = name), class = "link-glm")
}

.glm_identity <- function() {
    .glm_link("identity", function(mu) mu, function(eta) eta,
              function(eta) rep(1, length(eta)))
}

.glm_log <- function(floor) {
    inverse <- function(eta) .glm_floor(exp(eta), floor)
    .glm_link("log", function(mu) log(mu), inverse, inverse)
}

.glm_floor <- function(x, bound) {
    x[!is.na(x) & x < bound] <- bound
    x
}
.glm_ceiling <- function(x, bound) {
    x[!is.na(x) & x > bound] <- bound
    x
}

.glm_logit <- function() {
    .glm_link("logit", function(mu) {
        if (is.numeric(mu)) {
            bad <- .lm_which(!is.na(mu) & (mu < 0 | mu > 1))
            if (length(bad)) {
                value <- tolower(sprintf("%.6g", mu[bad[1L]]))
                stop(paste0("Value ", value, " out of range (0, 1)"))
            }
        }
        .Internal(C_glm_link_math(mu, 1L))
    }, function(eta) .Internal(C_glm_link_math(eta, 2L)),
              function(eta) .Internal(C_glm_link_math(eta, 3L)))
}

make.link <- function(link) {
    positive <- function(eta) all(is.finite(eta)) && all(eta > 0)
    if (is.null(link) || length(link) != 1L) stop("EXPR must be a length 1 vector")
    if (!is.character(link)) stop("object 'valideta' not found")
    if (is.na(link)) stop("‘NA’ link not recognised")
    if (link == "logit") return(.glm_logit())
    if (link == "probit") {
        bound <- -qnorm(.glm_eps)
        return(.glm_link(link, function(mu) qnorm(mu),
                         function(eta) pnorm(.glm_floor(.glm_ceiling(eta, bound), -bound)),
                         function(eta) .glm_floor(dnorm(eta), .glm_eps)))
    }
    if (link == "cauchit") {
        bound <- -qcauchy(.glm_eps)
        return(.glm_link(link, function(mu) qcauchy(mu),
                         function(eta) pcauchy(.glm_floor(.glm_ceiling(eta, bound), -bound)),
                         function(eta) .glm_floor(dcauchy(eta), .glm_eps)))
    }
    if (link == "cloglog") {
        return(.glm_link(link, function(mu) .Internal(C_glm_link_math(mu, 4L)),
                         function(eta) .Internal(C_glm_link_math(eta, 5L)),
                         function(eta) .Internal(C_glm_link_math(eta, 6L))))
    }
    if (link == "identity") return(.glm_identity())
    if (link == "log") return(.glm_log(.glm_eps))
    if (link == "sqrt") return(.glm_link(link, function(mu) sqrt(mu), function(eta) eta^2,
                                         function(eta) 2 * eta, positive))
    if (link == "1/mu^2") return(.glm_link(link, function(mu) 1 / mu^2, function(eta) 1 / sqrt(eta),
                                           function(eta) -0.5 / eta^1.5, positive))
    if (link == "inverse") return(.glm_link(link, function(mu) 1 / mu, function(eta) 1 / eta,
                                            function(eta) -1 / eta^2,
                                            function(eta) all(is.finite(eta)) && all(eta != 0)))
    stop(paste0("‘", link, "’ link not recognised"))
}

power <- function(lambda = 1) {
    if (lambda <= 0) return(make.link("log"))
    if (lambda == 1) return(.glm_identity())
    .glm_link(paste0("mu^", lambda), function(mu) mu^lambda,
              function(eta) .glm_floor(eta^(1 / lambda), .glm_eps),
              function(eta) .glm_floor(eta^(1 / lambda - 1) / lambda, .glm_eps),
              function(eta) all(is.finite(eta)) && all(eta > 0))
}

.glm_resolve_link <- function(link, expression, family) {
    standard <- c("logit", "probit", "cauchit", "cloglog", "identity",
                  "log", "sqrt", "1/mu^2", "inverse")
    if (typeof(expression) == "symbol" && .glm_in(as.character(expression), standard))
        return(make.link(as.character(expression)))
    if (inherits(link, "link-glm")) return(link)
    if (is.character(link)) return(make.link(link))
    binomial <- c("logit", "probit", "cloglog", "cauchit", "log", "identity")
    poisson <- c("log", "identity", "sqrt")
    gaussian <- c("inverse", "log", "identity")
    choices <- list(gaussian = gaussian, Gamma = gaussian, "inverse.gaussian" = c(gaussian, "1/mu^2"),
                    binomial = binomial, quasibinomial = binomial,
                    poisson = poisson, quasipoisson = poisson)
    if (family == "quasi") stop("$ operator is invalid for atomic vectors")
    stop(paste0("link \"", deparse1(expression), "\" not available for ", family,
                " family; available links are ", paste0("‘", choices[[family]], "’", collapse = ", ")))
}
