skip <- function(message = "Skipping") {
  condition <- structure(list(message = message, call = NULL), class = c("skip", "condition"))
  signalCondition(condition)
  invisible(NULL)
}

skip_if <- function(condition, message = NULL) {
  if (isTRUE(condition)) skip(if (is.null(message)) expression_label(substitute(condition)) else message)
  invisible(NULL)
}

skip_if_not <- function(condition, message = NULL) {
  if (!isTRUE(condition)) skip(if (is.null(message)) expression_label(substitute(condition)) else message)
  invisible(NULL)
}

skip_on_cran <- function() {
  if (!identical(Sys.getenv("NOT_CRAN"), "true")) skip("On CRAN")
}

skip_on_os <- function(os, arch = NULL) {
  system <- tolower(Sys.info()[["sysname"]])
  if (system %in% os && (is.null(arch) || R.version$arch %in% arch)) {
    skip(paste("On", system))
  }
}

skip_on_ci <- function() {
  if (tolower(Sys.getenv("CI")) %in% c("true", "1")) skip("On continuous integration")
}

skip_if_not_installed <- function(pkg, minimum_version = NULL) {
  if (!requireNamespace(pkg, quietly = TRUE)) skip(paste(pkg, "is not installed"))
  if (!is.null(minimum_version) && numeric_version(getNamespaceVersion(pkg)) < minimum_version) {
    skip(paste(pkg, "must be at least version", minimum_version))
  }
}

skip_if_offline <- function(host = "captive.apple.com") skip("Network access is disabled")
