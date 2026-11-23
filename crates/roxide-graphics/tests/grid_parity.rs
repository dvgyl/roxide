#![forbid(unsafe_code)]

//! Check grid state and unit operations with garbage collection at every allocation.

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::PureHost;
use roxide_value::GLOBAL_ENV;

fn run(script: &str) {
    let mut ctx = Ctx::new(Box::new(PureHost::new()), GcPolicy::Torture).expect("create context");
    roxide_base::register_all(&mut ctx).expect("register base");
    roxide_graphics::register(&mut ctx).expect("register graphics");
    let (result, stderr) = ctx
        .capture_streams(false, true, false, |ctx| {
            Ok(ctx.run_script("grid_parity.R", script, GLOBAL_ENV, false))
        })
        .expect("capture diagnostics");
    assert!(result.is_ok(), "{}", String::from_utf8_lossy(&stderr));
}

#[test]
fn viewport_trees_survive_device_changes_and_page_reset() {
    run(r#"
        grDevices::pdf("/tmp/first.pdf", width = 8, height = 4)
        grid::grid.newpage()
        layout <- grid::grid.layout(2, 2,
            widths = grid::unit(c(1, 2), "null"), respect = TRUE)
        grid::pushViewport(grid::viewport(name = "parent", layout = layout,
            gp = grid::gpar(alpha = .5, cex = 2)))
        grid::pushViewport(grid::viewport(name = "child", layout.pos.row = 1,
            layout.pos.col = 2, gp = grid::gpar(alpha = .5, cex = 3)))
        stopifnot(identical(as.character(grid::current.vpPath()), "parent::child"))
        stopifnot(identical(grid::get.gpar("alpha")$alpha, .25))
        stopifnot(identical(grid::get.gpar("cex")$cex, 6))
        stopifnot(abs(grid::convertWidth(grid::unit(1, "npc"), "in", TRUE) - 4) < 1e-12)
        grid::upViewport(0)
        stopifnot(identical(grid::current.viewport()$name, "ROOT"))
        grDevices::pdf("/tmp/second.pdf", width = 5, height = 5)
        grid::grid.newpage()
        stopifnot(identical(grid::current.viewport()$xscale, c(0, 360)))
        grDevices::dev.set(2L)
        stopifnot(identical(grid::seekViewport("child"), 2L))
        stopifnot(identical(grid::current.parent()$name, "parent"))
        stopifnot(tryCatch(grid::seekViewport("missing"), error = function(e) TRUE))
        stopifnot(is.null(grid::current.vpPath()))
        grid::seekViewport("child")
        grid::popViewport()
        stopifnot(tryCatch(grid::downViewport("child"), error = function(e) TRUE))
        grid::grid.newpage()
        stopifnot(is.null(grid::current.vpPath()))
        stopifnot(identical(grid::current.vpTree()$name, "ROOT"))
        grDevices::graphics.off()
        "#);
}

#[test]
fn composite_units_keep_data_and_use_device_metrics() {
    run(r#"
        grDevices::pdf("/tmp/text.pdf", width = 8, height = 4)
        grid::grid.newpage()
        w <- grid::stringWidth(c("AV", "g", "a\nb"))
        x <- grid::unit(1, "in") + w
        stopifnot(identical(grid::unitType(x), rep("sum", 3L)))
        stopifnot(identical(as.character(x[2]), "sum(1inches, 1strwidth)"))
        stopifnot(abs(grid::convertWidth(x[2], "bigpts", TRUE) - 78.672) < 1e-10)
        x[2] <- grid::unit(72, "bigpts")
        stopifnot(identical(as.character(x[2]), "72bigpts"))
        stopifnot(abs(grid::convertHeight(grid::stringDescent("g"), "bigpts", TRUE) - 2.64) < 1e-10)
        grDevices::graphics.off()
        "#);
}
