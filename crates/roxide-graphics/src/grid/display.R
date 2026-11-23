.grid_record <- function(state, type, value, count = NULL)
    invisible(.Internal(C_grid_record(state, type, value, count)))

.grid_enter <- function(vp, state) {
    if (is.null(vp)) return(invisible(NULL))
    if (is.character(vp) || inherits(vp, "vpPath"))
        .grid_down(state, state$current, vp, FALSE)
    else .grid_push(vp, state)
    invisible(NULL)
}

.grid_scope <- function(x, state) {
    .grid_enter(x$vp, state)
    node <- state$current
    old <- node$gp
    if (!is.null(x$gp) && length(x$gp)) node$gp <- .grid_inherit_gp(old, x$gp)
    list(node = node, gp = old)
}

.grid_render <- function(x, state) {
    if (inherits(x, "recordedGrob")) {
        .grid_replay(list(x$list[[1L]]), state)
        return(invisible(NULL))
    }
    origin <- state$current
    scope <- NULL
    on.exit({
        if (!is.null(scope)) scope$node$gp <- scope$gp
        .grid_select(state, origin)
    })
    x <- makeContext(x)
    scope <- .grid_scope(x, state)
    preDrawDetails(x)
    x <- makeContent(x)
    drawDetails(x, FALSE)
    if (inherits(x, "gTree")) {
        .grid_enter(x$childrenvp, state)
        .grid_select(state, scope$node)
        for (child in x$children) .grid_render(child, state)
    }
    postDrawDetails(x)
    invisible(NULL)
}

grid.draw <- function(x, recording = TRUE) {
    if (is.null(x)) return(invisible(NULL))
    state <- .grid_state()
    if (is.grob(x)) {
        .grid_render(x, state)
        if (recording) .grid_record(state, "grob", x)
    } else if (inherits(x, "gList")) {
        for (child in x) grid.draw(child, recording)
    } else if (inherits(x, "viewport") || inherits(x, "vpList") ||
               inherits(x, "vpStack") || inherits(x, "vpTree")) {
        pushViewport(x, recording = recording)
    } else if (inherits(x, "vpPath")) {
        downViewport(x, recording = recording)
    } else stop("invalid 'x' value")
    invisible(NULL)
}

.grid_angle <- function(theta) {
    if (is.character(theta)) {
        compass <- c(east = 0, north = 90, west = 180, south = 270)
        theta <- unname(compass[theta])
    }
    if (!is.numeric(theta) || !length(theta) || any(!is.finite(theta))) stop("invalid 'theta'")
    theta
}

.grid_grob_unit <- function(x, value, type) {
    if (inherits(x, "gList")) return(unit(rep(value, length.out = length(x)), type, unclass(x)))
    unit(value, type, x)
}

grobX <- function(x, theta) .grid_grob_unit(x, .grid_angle(theta), "grobx")
grobY <- function(x, theta) .grid_grob_unit(x, .grid_angle(theta), "groby")
grobWidth <- function(x) .grid_grob_unit(x, 1, "grobwidth")
grobHeight <- function(x) .grid_grob_unit(x, 1, "grobheight")
grobAscent <- function(x) .grid_grob_unit(x, 1, "grobascent")
grobDescent <- function(x) .grid_grob_unit(x, 1, "grobdescent")

widthDetails <- function(x) UseMethod("widthDetails")
heightDetails <- function(x) UseMethod("heightDetails")
ascentDetails <- function(x) UseMethod("ascentDetails")
descentDetails <- function(x) UseMethod("descentDetails")
xDetails <- function(x, theta) UseMethod("xDetails")
yDetails <- function(x, theta) UseMethod("yDetails")

.grid_bounds <- function(x, theta = 0) {
    state <- .grid_state()
    just <- if (is.null(x$just)) c(.5, .5) else .grid_just(x$just)
    target <- state$measure.target
    if (is.null(target)) target <- state$current
    .Internal(C_grid_bounds(x, state$current, just, theta, target))
}

.grid_x <- function(x, theta) unit(.grid_bounds(x, theta)[1L], "inches")
.grid_y <- function(x, theta) unit(.grid_bounds(x, theta)[2L], "inches")
.grid_w <- function(x) unit(.grid_bounds(x)[3L], "inches")
.grid_h <- function(x) unit(.grid_bounds(x)[4L], "inches")
.grid_ascent <- function(x) unit(.grid_bounds(x)[5L], "inches")
.grid_descent <- function(x) unit(.grid_bounds(x)[6L], "inches")

xDetails.default <- function(x, theta) .grid_x(x, theta)
yDetails.default <- function(x, theta) .grid_y(x, theta)
widthDetails.default <- function(x) .grid_w(x)
heightDetails.default <- function(x) .grid_h(x)
ascentDetails.default <- function(x) .grid_ascent(x)
descentDetails.default <- function(x) .grid_descent(x)

.grid_grob_measure <- function(data, code, value, node) {
    if (inherits(data, "gPath")) {
        key <- data$name
        data <- grid.get(data)
        if (is.null(data)) stop(paste0("grob '", key, "' not found"))
    }
    state <- .grid_state()
    origin <- state$current
    previous <- state$measure.target
    state$current <- node
    on.exit({
        state$measure.target <- previous
        .grid_select(state, origin)
    })
    scope <- .grid_scope(makeContext(data), state)
    on.exit(scope$node$gp <- scope$gp, add = TRUE, after = FALSE)
    state$measure.target <- node
    preDrawDetails(data)
    data <- makeContent(data)
    size <- if (code == 19L) xDetails(data, value)
        else if (code == 20L) yDetails(data, value)
        else if (code == 21L) widthDetails(data)
        else if (code == 22L) heightDetails(data)
        else if (code == 23L) ascentDetails(data)
        else descentDetails(data)
    postDrawDetails(data)
    axis <- if (code == 19L || code == 21L) "x" else "y"
    result <- .grid_inches(size, state$current, axis, if (code <= 20L) "location" else "dimension")
    if (code <= 20L) result[1L] else value * result[1L]
}

.grid_grobs <- function(state) {
    display <- state$display
    out <- list()
    for (i in seq_along(display)) {
        if (display[[i]]$type == "grob") out[[length(out) + 1L]] <- display[[i]]$value
    }
    out
}

getNames <- function() vapply(.grid_grobs(.grid_state()), function(grob) grob$name, "")

.grid_gpath <- function(path) {
    if (!inherits(path, "gPath")) path <- gPath(path)
    strsplit(as.character.gPath(path), "::", fixed = TRUE)[[1L]]
}

.grid_matches <- function(grobs, path, strict, grep) {
    parts <- .grid_gpath(path)
    match <- function(items, branch, depth) {
        hits <- list()
        for (i in seq_along(items)) {
            item <- items[[i]]
            matches <- if (grep) grepl(parts[depth], item$name) else item$name == parts[depth]
            current <- c(branch, i)
            if (matches) {
                if (depth == length(parts)) hits[[length(hits) + 1L]] <- current
                else hits <- c(hits, match(item$children, current, depth + 1L))
            }
            if (!strict && depth == 1L) hits <- c(hits, match(item$children, current, depth))
        }
        hits
    }
    match(grobs, integer(), 1L)
}

.grid_index <- function(grobs, path) {
    x <- grobs[[path[1L]]]
    for (i in path[-1L]) x <- x$children[[i]]
    x
}

.grid_replace <- function(grobs, path, fun) {
    i <- path[1L]
    if (length(path) == 1L) grobs[[i]] <- fun(grobs[[i]])
    else {
        tree <- grobs[[i]]
        children <- .grid_replace(tree$children, path[-1L], fun)
        grobs[[i]] <- .grid_children(tree, children)
    }
    grobs
}

grid.get <- function(gPath, strict = FALSE, grep = FALSE, global = FALSE, allDevices = FALSE) {
    if (allDevices) stop("'allDevices' not yet implemented")
    grobs <- .grid_grobs(.grid_state())
    matches <- .grid_matches(grobs, gPath, strict, grep)
    if (!length(matches)) return(NULL)
    if (!global || length(matches) == 1L) return(.grid_index(grobs, matches[[1L]]))
    do.call(gList, lapply(matches, function(path) .grid_index(grobs, path)))
}

.grid_change <- function(x, changes) {
    for (name in names(changes)) x[name] <- list(changes[[name]])
    .grid_validate_grob(x)
}

editGrob <- function(grob, gPath = NULL, ..., strict = FALSE, grep = FALSE,
                     global = FALSE, warn = TRUE) {
    changes <- list(...)
    if (is.null(gPath)) return(.grid_change(grob, changes))
    matches <- .grid_matches(list(grob), gPath, strict, grep)
    if (!length(matches) && warn)
        warning(paste0("'gPath' (", paste(.grid_gpath(gPath), collapse = "::"), ") not found"))
    for (path in matches[seq_len(if (global) length(matches) else min(1L, length(matches)))])
        grob <- .grid_replace(list(grob), path, function(x) .grid_change(x, changes))[[1L]]
    grob
}

.grid_update <- function(path, fun, strict = FALSE, grep = FALSE, global = FALSE,
                         redraw = TRUE, remove = FALSE, warn = TRUE) {
    state <- .grid_state()
    display <- state$display
    indices <- integer()
    for (i in seq_along(display))
        if (display[[i]]$type == "grob") indices <- c(indices, i)
    grobs <- lapply(display[indices], function(row) row$value)
    matches <- .grid_matches(grobs, path, strict, grep)
    if (!length(matches)) {
        if (!warn) return(invisible(NULL))
        name <- paste(.grid_gpath(path), collapse = "::")
        if (remove) stop(paste0("gPath (", name, ") not found"))
        stop(paste0("'gPath' (", name, ") not found"))
    }
    if (!global) matches <- matches[1L]
    if (remove) matches <- rev(matches)
    for (path in matches) {
        at <- indices[path[1L]]
        grob <- .grid_replace(list(display[[at]]$value), c(1L, path[-1L]), fun)
        if (length(grob)) display[[at]]$value <- grob[[1L]]
        else display[at] <- list(NULL)
    }
    state$display <- display[!vapply(display, is.null, FALSE)]
    if (redraw) grid.refresh()
    invisible(NULL)
}

grid.edit <- function(gPath, ..., strict = FALSE, grep = FALSE, global = FALSE,
                      allDevices = FALSE, redraw = TRUE) {
    if (allDevices) stop("'allDevices' not yet implemented")
    changes <- list(...)
    .grid_update(gPath, function(x) .grid_change(x, changes), strict, grep, global, redraw)
}

grid.remove <- function(gPath, warn = TRUE, strict = FALSE, grep = FALSE,
                        global = FALSE, allDevices = FALSE, redraw = TRUE) {
    if (allDevices) stop("'allDevices' not yet implemented")
    .grid_update(gPath, function(x) NULL, strict, grep, global, redraw, TRUE, warn)
}

.grid_replay <- function(display, state) {
    for (entry in display) {
        type <- entry$type
        if (type == "grob") .grid_render(entry$value, state)
        else if (type == "push") .grid_push(entry$value, state)
        else if (type == "pop") popViewport(entry$count, recording = FALSE)
        else if (type == "up") upViewport(entry$count, recording = FALSE)
        else if (type == "down") downViewport(entry$value, recording = FALSE)
        else if (type == "seek") seekViewport(entry$value, recording = FALSE)
    }
}

grid.refresh <- function() {
    display <- .grid_state()$display
    grid.newpage()
    state <- .grid_state()
    state$display <- display
    .grid_replay(display, state)
    invisible(NULL)
}

.grid_ls_vps <- function(vp) {
    if (is.null(vp)) return(character())
    if (inherits(vp, "viewport")) return(vp$name)
    if (inherits(vp, "vpTree")) return(c(.grid_ls_vps(vp$parent), .grid_ls_vps(vp$children)))
    if (inherits(vp, "vpList") || inherits(vp, "vpStack"))
        return(as.character(.grid_flatten(lapply(vp, .grid_ls_vps))))
    if (inherits(vp, "vpPath")) return(.grid_path(vp))
    as.character(vp)
}

grid.ls <- function(x = NULL, grobs = TRUE, viewports = FALSE, fullNames = FALSE,
                    recursive = TRUE, print = TRUE, flatten = TRUE, ...) {
    rows <- list()
    vp <- character()
    label <- function(type, name) if (fullNames) paste0(type, "[", name, "]") else name
    entry <- function(name, gp, type) {
        rows[[length(rows) + 1L]] <<- list(name = name, gDepth = as.numeric(length(gp)),
            vpDepth = as.numeric(length(vp)), gPath = paste(gp, collapse = "::"),
            vpPath = paste(vp, collapse = "::"), type = type)
    }
    push <- function(names, gp, lookup = FALSE) {
        if (!viewports) return(invisible(NULL))
        for (name in names) {
            name <- label("viewport", name)
            entry(name, gp, if (lookup) "vpNameListing" else "vpListing")
            vp <<- c(vp, name)
        }
    }
    up <- function(n, gp, pop = FALSE) {
        if (!viewports || n == 0L) return(invisible(NULL))
        entry(label(if (pop) "popViewport" else "upViewport", as.character(n)), gp, "vpUpListing")
        vp <<- vp[seq_len(max(0L, length(vp) - n))]
    }
    visit <- function(x, gp) {
        if (!is.grob(x)) return(invisible(NULL))
        x <- makeContext(x)
        vps <- .grid_ls_vps(x$vp)
        push(vps, gp)
        tree <- inherits(x, "gTree") && recursive
        name <- label(class(x)[1L], x$name)
        if (grobs) entry(name, gp, if (tree) "gTreeListing" else "grobListing")
        if (tree) {
            next.path <- c(gp, name)
            cvp <- .grid_ls_vps(x$childrenvp)
            push(cvp, next.path)
            up(length(cvp), next.path)
            for (child in x$children) visit(child, next.path)
        }
        up(length(vps), gp)
    }
    if (is.null(x)) {
        push("ROOT", character())
        for (row in .grid_state()$display) {
            if (row$type == "grob") visit(row$value, character())
            else if (row$type == "up" || row$type == "pop") up(row$count, character(), row$type == "pop")
            else {
                if (row$type == "seek") up(max(0L, length(vp) - 1L), character())
                if (row$type == "down" || row$type == "seek") {
                    path <- row$node$path
                    start <- if (viewports) length(vp) else 1L
                    if (length(path) >= start)
                        push(path[seq.int(start, length(path))], character(), TRUE)
                } else push(.grid_ls_vps(row$value), character())
            }
        }
    } else if (is.grob(x)) visit(x, character())
    else if (inherits(x, "gList")) for (child in x) visit(child, character())
    else push(.grid_ls_vps(x), character())
    result <- structure(list(name = vapply(rows, function(x) x$name, ""),
        gDepth = vapply(rows, function(x) x$gDepth, 0.0),
        vpDepth = vapply(rows, function(x) x$vpDepth, 0.0),
        gPath = vapply(rows, function(x) x$gPath, ""),
        vpPath = vapply(rows, function(x) x$vpPath, ""),
        type = vapply(rows, function(x) x$type, "")), class = "flatGridListing")
    if (is.function(print)) print(result, ...)
    else if (isTRUE(print)) print.flatGridListing(result, ...)
    invisible(result)
}

print.flatGridListing <- function(x, ...) {
    for (i in seq_along(x$name)) {
        cat(paste(rep("  ", x$gDepth[i] + x$vpDepth[i]), collapse = ""), x$name[i], "\n", sep = "")
    }
    invisible(x)
}

.grid_grab_vp <- function(tree, path, vp) {
    if (!length(path)) return(tree)
    key <- path[1L]
    branch <- tree[[key]]
    if (is.null(branch)) branch <- list(vp = NULL, children = list())
    if (length(path) == 1L) branch$vp <- vp
    else branch$children <- .grid_grab_vp(branch$children, path[-1L], vp)
    tree[[key]] <- branch
    tree
}

.grid_grab_vp_list <- function(tree) {
    values <- lapply(tree, function(x) {
        if (length(x$children)) vpTree(x$vp, .grid_grab_vp_list(x$children)) else x$vp
    })
    .grid_vp_group(unname(values), "vpList")
}

grid.grab <- function(warn = 2, wrap = FALSE, wrap.grobs = FALSE, ...) {
    state <- .grid_state()
    grobs <- list()
    tree <- list()
    has.grobs <- FALSE
    for (entry in state$display) {
        node <- entry$node
        path <- node$path
        if (entry$type == "grob") has.grobs <- TRUE
        while (length(node$path)) {
            tree <- .grid_grab_vp(tree, node$path, node$vp)
            node <- node$parent
        }
        if (wrap && (entry$type != "grob" || wrap.grobs)) {
            x <- grob(expr = quote(.grid_replay(list(entry), .grid_state())), list = list(entry),
                cl = "recordedGrob")
        } else if (entry$type == "grob") {
            x <- entry$value
            if (!wrap && length(path)) {
                if (is.null(x$vp)) x$vp <- vpPath(path)
                else if (is.character(x$vp) || inherits(x$vp, "vpPath"))
                    x$vp <- vpPath(c(path, .grid_path(x$vp)))
                else if (warn > 0)
                    warning("grob visibility may depend on previous drawing")
            } else if (!wrap && is.null(x$vp)) x$vp <- NULL
        } else next
        grobs[[length(grobs) + 1L]] <- x
    }
    if (!has.grobs) return(NULL)
    vp <- if (length(tree)) .grid_grab_vp_list(tree)
    gTree(children = structure(grobs, class = "gList"), childrenvp = vp, ...)
}

.grid_capture_device <- function(width, height) {
    grDevices::pdf(file = tempfile(), width = width, height = height)
}

grid.grabExpr <- function(expr, warn = 2, wrap = FALSE, wrap.grobs = FALSE,
                          width = 7, height = 7, device = .grid_capture_device, ...) {
    previous <- grDevices::dev.cur()
    path <- NULL
    if (identical(device, .grid_capture_device)) {
        path <- tempfile()
        grDevices::pdf(file = path, width = width, height = height)
    } else device(width, height)
    capture <- grDevices::dev.cur()
    on.exit({
        grDevices::dev.off(capture)
        if (!is.null(path)) unlink(path)
        if (previous > 1L) grDevices::dev.set(previous)
    })
    grid.newpage()
    eval(substitute(expr), parent.frame())
    grid.grab(warn, wrap, wrap.grobs, ...)
}
