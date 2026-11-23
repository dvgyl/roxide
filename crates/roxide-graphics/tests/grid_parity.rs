#![forbid(unsafe_code)]

//! Check grid state and unit operations with garbage collection at every allocation.

use std::{io::Read, path::Path};

use flate2::read::ZlibDecoder;
use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::{OpenMode, PureHost};
use roxide_value::GLOBAL_ENV;

fn run(script: &str) -> Ctx {
    let mut ctx = Ctx::new(Box::new(PureHost::new()), GcPolicy::default()).expect("create context");
    roxide_base::register_all(&mut ctx).expect("register base");
    roxide_graphics::register(&mut ctx).expect("register graphics");
    ctx.session_mut().heap.set_policy(GcPolicy::Torture);
    let (result, stderr) = ctx
        .capture_streams(false, true, false, |ctx| {
            Ok(ctx.run_script("grid_parity.R", script, GLOBAL_ENV, false))
        })
        .expect("capture diagnostics");
    assert!(result.is_ok(), "{}", String::from_utf8_lossy(&stderr));
    ctx
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

fn output(ctx: &mut Ctx, file: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    ctx.host()
        .open(Path::new(file), OpenMode::read_only())
        .expect("open device file")
        .read_to_end(&mut bytes)
        .expect("read device file");
    bytes
}

#[test]
fn grid_draws_the_same_scene_on_three_devices() {
    let mut ctx = run(r#"
        scene <- grid::gTree(name = "scene", children = grid::gList(
            grid::rectGrob(width = .5, height = .5, gp = grid::gpar(fill = "red", col = NA), name = "square"),
            grid::circleGrob(x = .2, y = .2, r = .1, gp = grid::gpar(fill = "blue"), name = "circle"),
            grid::textGrob("grid", y = .8, name = "label")))
        draw <- function() {
            grid::grid.newpage()
            grid::grid.draw(scene)
            stopifnot(identical(grid::grid.get("scene::label")$label, "grid"))
            grid::grid.edit("scene::label", label = "edited", redraw = FALSE)
            stopifnot(identical(grid::childNames(grid::grid.grab()), "scene"))
            grid::grid.remove("scene::label", redraw = FALSE)
            stopifnot(is.null(grid::grid.get("label")))
            invisible(dev.off())
        }
        pdf("/tmp/grid.pdf", 2, 2, compress = FALSE, useKerning = FALSE)
        draw()
        svg("/tmp/grid.svg", 2, 2)
        draw()
        png("/tmp/grid.png", 144, 144, res = 72)
        draw()
        "#);

    let pdf = output(&mut ctx, "/tmp/grid.pdf");
    assert!(pdf.starts_with(b"%PDF"));
    assert!(pdf.windows(7).any(|bytes| bytes == b"(grid) "));
    let svg = output(&mut ctx, "/tmp/grid.svg");
    let svg = std::str::from_utf8(&svg).expect("UTF-8 SVG");
    assert!(svg.contains("<circle"));
    assert!(svg.contains(">grid</text>"));

    let png = output(&mut ctx, "/tmp/grid.png");
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let mut idat = Vec::new();
    let mut chunks = &png[8..];
    while chunks.len() >= 12 {
        let n = u32::from_be_bytes(chunks[..4].try_into().expect("chunk length")) as usize;
        if &chunks[4..8] == b"IDAT" {
            idat.extend_from_slice(&chunks[8..8 + n]);
        }
        chunks = &chunks[12 + n..];
    }
    let mut pixels = Vec::new();
    ZlibDecoder::new(idat.as_slice())
        .read_to_end(&mut pixels)
        .expect("decode PNG pixels");
    let center = 72 * (144 * 4 + 1) + 1 + 72 * 4;
    assert_eq!(&pixels[center..center + 4], &[255, 0, 0, 255]);
}
