tar <- function(tarfile, files = NULL, compression = c("none", "gzip", "bzip2", "xz", "zstd"),
                compression_level = 6, tar = Sys.getenv("tar"), extra_flags = "") {
    mode <- match.arg(compression)
    sources <- if (is.null(files)) list.files(all.files = TRUE, no.. = TRUE) else as.character(files)
    bytes <- .Internal(C_rox_tar_pack(path.expand(sources), path.expand(tarfile)))
    con <- switch(mode, none = file(tarfile, "wb"),
                  gzip = gzfile(tarfile, "wb", compression = compression_level),
                  bzip2 = bzfile(tarfile, "wb", compression = compression_level),
                  xz = xzfile(tarfile, "wb", compression = compression_level),
                  zstd = zstdfile(tarfile, "wb", compression = compression_level))
    on.exit(close(con))
    writeBin(bytes, con)
    invisible(0L)
}

untar <- function(tarfile, files = NULL, list = FALSE, exdir = ".", compressed = NA,
                  extras = NULL, verbose = FALSE, restore_times = TRUE,
                  support_old_tars = Sys.getenv("R_SUPPORT_OLD_TARS", FALSE), tar = Sys.getenv("TAR")) {
    con <- gzfile(tarfile, "rb")
    on.exit(close(con))
    bytes <- readBin(con, "raw", n = .Internal(C_rox_tar_limit()))
    if (length(readBin(con, "raw", n = 1L))) stop("Archive size limit exceeded")
    result <- .Internal(C_rox_tar_extract(bytes, as.character(files), list, path.expand(exdir)))
    if (list) return(result)
    invisible(result)
}
