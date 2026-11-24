Reporter <- SilentReporter <- NULL

.onLoad <- function(libname, pkgname) {
  if (!requireNamespace("R6", quietly = TRUE)) return(invisible(NULL))
  namespace <- asNamespace(pkgname)
  reporter <- R6::R6Class("Reporter", public = list(
    initialize = function(file = stdout()) invisible(self),
    start_reporter = function() invisible(NULL),
    end_reporter = function() invisible(NULL),
    start_test = function(context, test) invisible(NULL),
    end_test = function(context, test) invisible(NULL),
    add_result = function(context, test, result) invisible(NULL)
  ))
  assign("Reporter", reporter, envir = namespace)
  assign("SilentReporter", R6::R6Class("SilentReporter", inherit = reporter),
         envir = namespace)
}

with_reporter <- function(reporter, code, start_end_reporter = TRUE) {
  force(reporter)
  before <- options(testthat.callbacks = reporter, testthat.reporter = NULL)
  on.exit(options(before))
  if (start_end_reporter) {
    reporter$start_reporter()
    on.exit(reporter$end_reporter(), add = TRUE, after = FALSE)
  }
  code
}

quasi_label <- function(quo, arg, label = NULL) {
  if (is.null(label)) {
    expr <- if (inherits(quo, "quosure")) quo[[2L]] else quo
    label <- if (is.character(arg) && length(arg) == 1L) arg else expression_label(expr)
  }
  list(val = arg, lab = label)
}

new_reporter <- function() {
  reporter <- new.env()
  reporter$fail <- 0L
  reporter$warn <- 0L
  reporter$skip <- 0L
  reporter$pass <- 0L
  reporter$n <- 0L
  reporter$lines <- list()
  reporter
}

ascii_text <- function(text) gsub("[^ -~\t\n]+", "?", text, perl = TRUE)

report_result <- function(reporter, kind, file, desc, message = NULL) {
  if (is.null(reporter)) return(invisible(NULL))
  reporter[[kind]] <- reporter[[kind]] + 1L
  if (kind == "warn") return(invisible(NULL))

  reporter$n <- reporter$n + 1L
  status <- if (kind == "pass") "ok" else if (kind == "skip") "skip" else "not ok"
  heading <- gsub("[\n\t]", " ", ascii_text(paste0(file, ": ", desc)))
  line <- sprintf("%s %d - %s\n", status, reporter$n, heading)
  if (kind == "fail" && !is.null(message)) {
    line <- paste0(line, "  ", gsub("\n", "\n  ", ascii_text(message), fixed = TRUE), "\n")
  }
  reporter$lines[[reporter$n]] <- line
  invisible(NULL)
}

finish_report <- function(reporter, stop_on_failure, quiet = FALSE) {
  if (!quiet) {
    for (line in reporter$lines) cat(line)
    cat(sprintf("[ FAIL %d | WARN %d | SKIP %d | PASS %d ]\n",
                reporter$fail, reporter$warn, reporter$skip, reporter$pass))
  }
  if (stop_on_failure && reporter$fail > 0L) {
    if (quiet) stop("Tests failed.")
    quit(status = 1L, save = "no")
  }
  invisible(list(fail = reporter$fail, warn = reporter$warn,
                 skip = reporter$skip, pass = reporter$pass))
}
