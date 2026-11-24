.gd_white_xy <- rbind(
    A = c(.44757, .40745), B = c(.34842, .35161), C = c(.31006, .31616),
    D50 = c(.34574, .35867), D55 = c(.33250, .34761),
    D65 = c(.31370, .32910), E = c(1/3, 1/3)
)

.gd_white_xyz <- function(white) {
    if (is.character(white) && any(!white %in% rownames(.gd_white_xy)))
        stop("subscript out of bounds")
    x <- .gd_white_xy[white, 1L][1L]
    y <- .gd_white_xy[white, 2L][1L]
    xyz <- c(x/y, 1, (1 - x - y)/y)
    if (identical(white, "E")) xyz <- c(1, 1, 1)
    if (is.character(white) && length(white)) names(xyz) <- c("x", "", "y")
    xyz
}

.gd_color_row <- function(x) matrix(x, nrow = 1L)

.gd_color_frame <- function(frame) {
    if (exists("as.matrix", mode = "function")) return(as.matrix(frame))
    rows <- if (.Internal(C_gd_color_named_rows(frame))) rownames(frame) else NULL
    if (!ncol(frame)) return(matrix(logical(), nrow(frame), 0L,
                                    dimnames = list(rows, NULL)))
    columns <- as.list(frame)
    if (any(vapply(columns, function(x) is.factor(x) || is.character(x), TRUE)))
        columns <- lapply(columns, as.character)
    result <- do.call(cbind, columns)
    dimnames(result) <- list(rows, colnames(result))
    result
}

.gd_vectorize_converter <- function(convert) {
    function(color, white) {
        if (is.matrix(color) && nrow(color) == 0L) {
            sample <- vector(typeof(color), ncol(color))
            converted <- convert(sample, white)
            return(matrix(converted[0L], 0L, 0L))
        }
        converted <- apply(color, 1L, convert, white)
        if (!is.null(dim(converted))) return(t(converted))
        if (is.list(converted) && all(lengths(converted) == 0L))
            converted <- unlist(converted, use.names = FALSE)
        matrix(converted, nrow = nrow(color))
    }
}

colorConverter <- function(toXYZ, fromXYZ, name, white = NULL, vectorized = FALSE) {
    forward <- if (vectorized) toXYZ else .gd_vectorize_converter(toXYZ)
    reverse <- if (vectorized) fromXYZ else .gd_vectorize_converter(fromXYZ)
    structure(list(toXYZ = forward, fromXYZ = reverse, name = name,
                   white = white, reference.white = white),
              class = "colorConverter")
}

.gd_transfer <- function(x, gamma, encode) {
    if (!is.numeric(x) && !is.logical(x)) stop("non-numeric argument to binary operator")
    if (!identical(gamma, "sRGB")) return(sign(x) * abs(x)^gamma)
    result <- .Internal(C_gd_srgb_transfer(x, encode))
    attributes(result) <- attributes(x)
    result
}

.gd_rgb_matrix <- function(red, green, blue, white) {
    xy <- rbind(red[1:2], green[1:2], blue[1:2])
    labels <- colnames(xy)
    primary <- unname(cbind(xy, 1 - rowSums(xy)))
    xyz <- .gd_white_xyz(white)
    transform <- if (anyNA(primary) || anyNA(xyz)) matrix(NA_real_, 3L, 3L)
                 else primary * as.vector(solve(t(primary), xyz))
    if (!is.null(labels)) colnames(transform) <- c(labels[1L], "", labels[2L])
    transform
}

make.rgb <- function(red, green, blue, name = NULL, white = "D65", gamma = 2.2) {
    if (is.null(name)) name <- deparse(sys.call())[1L]
    if (is.numeric(gamma) && length(gamma) == 1L) {
        decode_gamma <- gamma
        encode_gamma <- 1/gamma
    } else {
        if (gamma != "sRGB") stop("'gamma' must be a scalar or 'sRGB'")
        decode_gamma <- encode_gamma <- "sRGB"
    }
    transform <- .gd_rgb_matrix(red, green, blue, white)
    to_xyz <- function(rgb, ...) .gd_transfer(rgb, decode_gamma, FALSE) %*% transform
    from_xyz <- function(xyz, ...) {
        rgb <- .gd_transfer(xyz %*% solve(transform), encode_gamma, TRUE)
        if (nrow(rgb) == 1L) rgb[1L, , drop = TRUE] else rgb
    }
    converter <- colorConverter(to_xyz, from_xyz, name, white, vectorized = TRUE)
    converter$gamma <- gamma
    class(converter) <- c("RGBcolorConverter", "colorConverter")
    converter
}

.gd_perceptual <- function(x, white, luv, to_xyz) {
    if (!luv && length(x) != 3L && !isTRUE(ncol(x) == 3L)) {
        if (to_xyz) stop("ncol(Lab) == 3L || length(Lab) == 3 is not TRUE")
        stop("length(XYZ) == 3 || ncol(XYZ) == 3L is not TRUE")
    }
    if (is.null(dim(x)) && !luv) x <- .gd_color_row(x)
    if (!is.matrix(x)) stop("incorrect number of dimensions")
    if (ncol(x) < 3L) stop("subscript out of bounds")
    if (ncol(x) > 3L) x <- x[, 1:3, drop = FALSE]
    if ((!is.numeric(x) && !is.logical(x)) ||
        (!is.numeric(white) && !is.logical(white)))
        stop("non-numeric argument to binary operator")
    if (!luv) white <- rep_len(white, 3L)
    values <- .Internal(C_gd_perceptual_color(x, white, luv, to_xyz))
    if (nrow(values) == 1L) values[1L, , drop = TRUE] else values
}

.gd_xyz_identity <- function(x, w) x

colorspaces <- list(
    XYZ = colorConverter(.gd_xyz_identity, .gd_xyz_identity, "XYZ", vectorized = TRUE),
    "Apple RGB" = make.rgb(c(.625, .34), c(.28, .595), c(.155, .07),
                          "Apple RGB", gamma = 1.8),
    sRGB = make.rgb(c(.64, .33), c(.30, .60), c(.15, .06), "sRGB", gamma = "sRGB"),
    "CIE RGB" = make.rgb(c(.735, .265), c(.274, .717), c(.167, .009),
                        "CIE RGB", white = "E"),
    Lab = colorConverter(
        function(Lab, white) .gd_perceptual(Lab, white, FALSE, TRUE),
        function(XYZ, white) .gd_perceptual(XYZ, white, FALSE, FALSE),
        "Lab", vectorized = TRUE
    ),
    Luv = colorConverter(
        function(Luv, white) .gd_perceptual(Luv, white, TRUE, TRUE),
        function(XYZ, white) .gd_perceptual(XYZ, white, TRUE, FALSE),
        "Luv", vectorized = TRUE
    )
)

.gd_find_converter <- function(space, argument) {
    if (is.character(space)) return(colorspaces[[.gd_choice(space, names(colorspaces))]])
    if (!inherits(space, "colorConverter"))
        stop(sprintf("'%s' must be a \"colorConverter\" object or a string", argument))
    space
}

.gd_converter_white <- function(converter, white, argument) {
    fixed <- converter$reference.white
    if (is.null(fixed)) return(white)
    if (!is.null(white) && white != fixed)
        stop(sprintf("'%s' disagrees with definition of %s", argument, converter$name))
    fixed
}

# Hunt-Pointer-Estevez cone response matrix for the von Kries transform.
.gd_cone_response <- matrix(c(.40024, .70760, -.08081,
                              -.22630, 1.16532, .04570,
                              0, 0, .91822), 3L, byrow = TRUE)

.gd_adapt_white <- function(xyz, from, to) {
    response <- .gd_cone_response
    scale <- as.vector(response %*% to) / as.vector(response %*% from)
    xyz %*% t(solve(response, scale * response))
}

convertColor <- function(color, from, to, from.ref.white = NULL, to.ref.white = NULL,
                         scale.in = 1, scale.out = 1, clip = TRUE) {
    from <- .gd_find_converter(from, "from")
    to <- .gd_find_converter(to, "to")
    from_white <- .gd_converter_white(from, from.ref.white, "from.ref.white")
    to_white <- .gd_converter_white(to, to.ref.white, "to.ref.white")
    if (is.null(from_white)) from_white <- if (is.null(to_white)) "D65" else to_white
    if (is.null(to_white)) to_white <- from_white
    from_xyz <- .gd_white_xyz(from_white)
    to_xyz <- .gd_white_xyz(to_white)

    if (is.data.frame(color)) color <- .gd_color_frame(color)
    if (is.null(dim(color))) color <- .gd_color_row(color)
    if (!is.null(scale.in)) color <- color / scale.in
    xyz <- from$toXYZ(color, from_xyz)
    if (is.null(dim(xyz))) xyz <- .gd_color_row(xyz)
    if (!identical(as.vector(from_xyz), as.vector(to_xyz))) {
        if (!is.null(from$reference.white) || !is.null(to$reference.white))
            warning("color spaces use different reference whites")
        xyz <- .gd_adapt_white(xyz, from_xyz, to_xyz)
    }
    result <- to$fromXYZ(xyz, to_xyz)
    if (inherits(to, "RGBcolorConverter")) {
        result <- round(result, 5L)
        if (is.na(clip)) {
            result[.gd_indices(result < 0 | result > 1)] <- NaN
        } else if (clip) {
            result[.gd_indices(result < 0)] <- 0
            result[.gd_indices(result > 1)] <- 1
        }
    }
    if (is.null(dim(result))) result <- rbind(result, deparse.level = 0L)
    if (!is.null(scale.out)) result <- result * scale.out
    result
}
