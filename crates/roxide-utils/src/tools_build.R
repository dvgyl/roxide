.rx_makevars_home <- function() {
    home <- Sys.getenv("R_USER")
    if (nzchar(home)) home else Sys.getenv("HOME", unset = path.expand("~"))
}

.rx_existing_makevars <- function(paths, home) {
    expanded <- paths
    expanded[paths == "~"] <- home
    relative <- startsWith(paths, "~/")
    if (any(relative)) {
        expanded[relative] <- file.path(home, substring(paths[relative], 3L))
    }
    paths[file.exists(expanded)]
}

makevars_user <- function() {
    home <- .rx_makevars_home()
    path <- Sys.getenv("R_MAKEVARS_USER", unset = NA_character_)
    if (is.na(path)) {
        path <- file.path(home, ".R", c(paste0("Makevars-", R.version$platform), "Makevars"))
    }
    path <- .rx_existing_makevars(path, home)
    if (length(path)) path[1L] else character()
}

makevars_site <- function() {
    path <- Sys.getenv("R_MAKEVARS_SITE", unset = file.path(R.home("etc"), "Makevars.site"))
    .rx_existing_makevars(path, .rx_makevars_home())
}

SIGHUP <- 1L
SIGINT <- 2L
SIGQUIT <- 3L
SIGKILL <- 9L
SIGTERM <- 15L
if (.Internal(rox_tools_bsd_signals())) {
    SIGSTOP <- 17L
    SIGTSTP <- 18L
    SIGCONT <- 19L
    SIGCHLD <- 20L
    SIGUSR1 <- 30L
    SIGUSR2 <- 31L
} else {
    SIGSTOP <- 19L
    SIGTSTP <- 20L
    SIGCONT <- 18L
    SIGCHLD <- 17L
    SIGUSR1 <- 10L
    SIGUSR2 <- 12L
}

.check_packages <- function(args = NULL, no.q = interactive(), warnOption = 1) {
    stop("R CMD check is unavailable in Roxide", call. = FALSE)
}
