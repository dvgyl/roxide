//! Check graphics device bindings with a deterministic host and forced garbage collection.

#![forbid(unsafe_code)]

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_graphics::engine::{GraphicsExt, RecordingDevice, Rgba};
use roxide_graphics::grdevices::parse_rgba;
use roxide_host::PureHost;
use roxide_value::{GLOBAL_ENV, ValueApi};

fn graphics_context() -> Ctx {
    let mut ctx =
        Ctx::new(Box::new(PureHost::new()), GcPolicy::Torture).expect("Create the R session");
    roxide_base::register_all(&mut ctx).expect("Register the base package");
    roxide_graphics::register(&mut ctx).expect("Register the graphics packages");
    ctx
}

#[test]
fn colors_preserve_names_and_palette_state_during_collection() {
    let mut ctx = graphics_context();
    ctx.run_script(
        "colors.R",
        r##"
v <- grDevices::col2rgb(c(a = "red", b = "#0008"), alpha = TRUE)
stopifnot(identical(dim(v), c(4L, 2L)),
          identical(dimnames(v), list(c("red", "green", "blue", "alpha"), c("a", "b"))),
          identical(as.integer(v), c(255L, 0L, 0L, 255L, 0L, 0L, 0L, 136L)),
          length(colors()) == 657L, length(colors(TRUE)) == 502L)
old <- withVisible(palette(c("#010203", "red")))
stopifnot(!old$visible, identical(palette(), c("#010203", "red")))
        "##,
        GLOBAL_ENV,
        false,
    )
    .expect("Check color conversions");
    let index = ctx
        .session_mut()
        .scalar_int(3)
        .expect("Make a palette index");
    assert_eq!(parse_rgba(&mut ctx, index), Ok(Rgba::new(1, 2, 3, 255)));
}

#[test]
fn device_table_keeps_slot_order_and_uses_device_dimensions() {
    let mut ctx = graphics_context();
    let graphics = ctx.ext::<GraphicsExt>();
    assert_eq!(
        graphics.open_slot(Box::new(RecordingDevice::new("pdf", 5.0, 4.0))),
        Some(2)
    );
    assert_eq!(
        graphics.open_slot(Box::new(RecordingDevice::new("png", 2.0, 3.0))),
        Some(3)
    );
    ctx.run_script(
        "device_table.R",
        r#"
stopifnot(identical(dev.list(), c(pdf = 2L, png = 3L)),
          identical(dev.cur(), c(png = 3L)),
          identical(dev.next(99), c(pdf = 2L)),
          identical(dev.prev(99), c(png = 3L)),
          identical(dev.set(99), c(pdf = 2L)),
          identical(dev.size(), c(5, 4)),
          identical(dev.size("px"), c(360, 288)),
          max(abs(dev.size("cm") - c(12.7, 10.16))) < 1e-12,
          identical(dev.off(2), c(png = 3L)))
graphics.off()
stopifnot(is.null(dev.list()), identical(dev.cur(), c("null device" = 1L)))
        "#,
        GLOBAL_ENV,
        false,
    )
    .expect("Check device state");
}

#[test]
fn ranges_and_coordinates_work_without_native_io() {
    let mut ctx = graphics_context();
    ctx.run_script(
        "range_helpers.R",
        r#"
stopifnot(identical(pretty(2), c(0, 2)),
          identical(pretty(c(2, 8)), 2:8),
          identical(pretty(c(Inf, NaN)), numeric()),
          max(abs(extendrange(c(1, NA, 5), f = c(.1, .2)) - c(.6, 5.8))) < 1e-12,
          identical(n2mfrow(7, asp = .5), c(4, 2)),
          identical(xy.coords(7:9), list(x = c(1, 2, 3), y = c(7, 8, 9),
                                        xlab = "Index", ylab = NULL)))
        "#,
        GLOBAL_ENV,
        false,
    )
    .expect("Check range and coordinate helpers");
}

#[test]
fn extreme_pretty_coords_and_default_device_options() {
    let mut ctx = graphics_context();
    ctx.run_script(
        "device_hardening.R",
        r#"
br <- pretty(c(-1.7e308, 1.7e308))
stopifnot(length(br) == 7L, all(is.finite(br)), br[4L] == 0,
          br[1L] == -1.7e308, br[7L] == 1.7e308)
xyz <- xyz.coords(1:3, recycle = TRUE)
stopifnot(identical(xyz$y, numeric()), identical(xyz$z, numeric()))
xy <- xy.coords(cbind(foo = 1, bar = 2), ylab = "matrix")
stopifnot(identical(xy$xlab, "foo"), identical(xy$ylab, "bar"))
options(device = function(...) pdf(NULL, ...))
dev.new(width = 5, height = 4)
stopifnot(identical(dev.size(), c(5,4)), identical(names(par("mar","cex")), c("mar","cex")))
dev.off()
        "#,
        GLOBAL_ENV,
        false,
    )
    .expect("Check device and coordinate behavior");
}
