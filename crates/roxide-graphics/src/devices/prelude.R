pdf <- function(file = if (onefile) "Rplots.pdf" else "Rplot%03d.pdf",
                width = 7, height = 7, onefile = TRUE, family = "Helvetica",
                title = "R Graphics Output", fonts = NULL, version = "1.4",
                paper = "special", encoding = "default", bg = "transparent",
                fg = "black", pointsize = 12, pagecentre = TRUE,
                colormodel = "srgb", useDingbats = FALSE, useKerning = TRUE,
                fillOddEven = FALSE, compress = TRUE) {
    invisible(.Internal(rox_gr_pdf(file, width, height, onefile, compress, bg,
                                   pointsize, title, useKerning, fillOddEven, family)))
}

svg <- function(filename = if (onefile) "Rplots.svg" else "Rplot%03d.svg",
                width = 7, height = 7, pointsize = 12, onefile = FALSE,
                family = "sans", bg = "white",
                antialias = c("default", "none", "gray", "subpixel"),
                symbolfamily) {
    invisible(.Internal(rox_gr_svg(filename, width, height, pointsize, onefile, bg, family)))
}

png <- function(filename = "Rplot%03d.png", width = 480, height = 480,
                units = "px", pointsize = 12, bg = "white", res = NA,
                ..., type = getOption("bitmapType"), antialias) {
    invisible(.Internal(rox_gr_png(filename, width, height, units, pointsize, bg, res, type)))
}

bmp <- function(filename = "Rplot%03d.bmp", width = 480, height = 480,
                units = "px", pointsize = 12, bg = "white", res = NA,
                ..., type = c("cairo", "Xlib", "quartz"), antialias) {
    type <- if (missing(type)) getOption("bitmapType") else .gd_choice(type, c("cairo", "Xlib", "quartz"))
    invisible(.Internal(rox_gr_bmp(filename, width, height, units, pointsize, bg, res, type)))
}

jpeg <- function(filename = "Rplot%03d.jpeg", width = 480, height = 480,
                 units = "px", pointsize = 12, quality = 75, bg = "white", res = NA,
                 ..., type = c("cairo", "Xlib", "quartz"), antialias) {
    type <- if (missing(type)) getOption("bitmapType") else .gd_choice(type, c("cairo", "Xlib", "quartz"))
    invisible(.Internal(rox_gr_jpeg(filename, width, height, units, pointsize, bg, res, type, quality)))
}

tiff <- function(filename = "Rplot%03d.tiff", width = 480, height = 480,
                 units = "px", pointsize = 12,
                 compression = c("none", "rle", "lzw", "jpeg", "zip", "lzw+p", "zip+p",
                                 "lerc", "lzma", "zstd", "webp"),
                 bg = "white", res = NA, ..., type = c("cairo", "Xlib", "quartz"), antialias) {
    type <- if (missing(type)) getOption("bitmapType") else .gd_choice(type, c("cairo", "Xlib", "quartz"))
    if (is.character(compression)) {
        compression <- .gd_choice(compression, c("none", "rle", "lzw", "jpeg", "zip",
                                                 "lzw+p", "zip+p", "lerc", "lzma", "zstd", "webp"))
    }
    if (!identical(compression, "none") &&
        !(is.numeric(compression) && length(compression) == 1L && isTRUE(compression == 1))) {
        if (identical(type, "quartz")) {
            warning('compression is not supported for type = "quartz"')
        } else {
            stop("only uncompressed TIFF is supported")
        }
    }
    invisible(.Internal(rox_gr_tiff(filename, width, height, units, pointsize, bg, res, type)))
}
