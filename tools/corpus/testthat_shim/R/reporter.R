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

finish_report <- function(reporter, stop_on_failure) {
  for (line in reporter$lines) cat(line)
  cat(sprintf("[ FAIL %d | WARN %d | SKIP %d | PASS %d ]\n",
              reporter$fail, reporter$warn, reporter$skip, reporter$pass))
  if (stop_on_failure && reporter$fail > 0L) quit(status = 1L, save = "no")
  invisible(list(fail = reporter$fail, warn = reporter$warn,
                 skip = reporter$skip, pass = reporter$pass))
}
