//! Check graphics device bindings with a deterministic host and forced garbage collection.

#![forbid(unsafe_code)]

use std::time::Duration;

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_graphics::engine::{GraphicsExt, RecordingDevice, Rgba};
use roxide_graphics::grdevices::parse_rgba;
use roxide_host::{CommandSpec, Host, NativeHost, PureHost};
use roxide_value::{GLOBAL_ENV, ValueApi};

fn graphics_context(gc: GcPolicy) -> Ctx {
    let mut ctx = Ctx::new(Box::new(PureHost::new()), gc).expect("Create the R session");
    roxide_base::register_all(&mut ctx).expect("Register the base package");
    roxide_graphics::register(&mut ctx).expect("Register the graphics packages");
    ctx
}

fn run_color_test(name: &str, script: &str) {
    let mut ctx = graphics_context(GcPolicy::default());
    let (result, stderr) = ctx
        .capture_streams(false, true, false, |ctx| {
            Ok(ctx.run_script(name, script, GLOBAL_ENV, false))
        })
        .expect("Capture the color conversion results");
    assert!(result.is_ok(), "{}", String::from_utf8_lossy(&stderr));
}

#[test]
fn colors_preserve_names_and_palette_state_during_collection() {
    let mut ctx = graphics_context(GcPolicy::Torture);
    ctx.run_script(
        "colors.R",
        r##"
v <- grDevices::col2rgb(c(a = "red", b = "#0008"), alpha = TRUE)
stopifnot(identical(dim(v), c(4L, 2L)),
          identical(dimnames(v), list(c("red", "green", "blue", "alpha"), c("a", "b"))),
          identical(as.integer(v), c(255L, 0L, 0L, 255L, 0L, 0L, 0L, 136L)),
          length(colors()) == 657L, length(colors(TRUE)) == 502L)
lab <- grDevices::convertColor(t(v[1:3, ]), "sRGB", "Lab", scale.in = 255)
stopifnot(identical(dimnames(lab), list(c("a", "b"), c("L", "a", "b"))))
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
fn convert_color_preserves_shapes_and_applies_rgb_scaling_and_clipping() {
    run_color_test(
        "convert_color.R",
        r#"
convert <- grDevices::convertColor
spaces <- grDevices::colorspaces
stopifnot(identical(names(spaces), c("XYZ", "Apple RGB", "sRGB", "CIE RGB", "Lab", "Luv")))

lab <- convert(c(255, 0, 0), "s", "Lab", scale.in = 255)
stopifnot(identical(dim(lab), c(1L, 3L)),
          identical(colnames(lab), c("L", "a", "b")),
          max(abs(lab - c(53.484183785248, 80.0102716478952, 67.3840746825475))) < 1e-6,
          identical(as.vector(convert(lab, "Lab", "sRGB", scale.out = 255)), c(255, 0, 0)))

x <- matrix(c(1.4, -.2, .1, .3, .2, .4), 2L,
            dimnames = list(c("red", "green"), c("r", "g", "b")))
clipped <- convert(x, "sRGB", "sRGB", clip = TRUE)
marked <- convert(x, "sRGB", "sRGB", clip = NA)
stopifnot(identical(dimnames(clipped), list(c("red", "green"), NULL)),
          identical(as.vector(clipped), c(1, 0, .1, .3, .2, .4)),
          identical(as.vector(marked), c(NaN, NaN, .1, .3, .2, .4)),
          identical(as.vector(convert(x, "sRGB", "sRGB", clip = FALSE)), as.vector(x)),
          identical(as.vector(convert(c(-1e-6, 1.000001, .23456789), "sRGB", "sRGB",
                                      clip = NA)), c(0, 1, .23457)))

empty <- matrix(numeric(), 0L, 3L)
stopifnot(identical(convert(empty, "sRGB", "Luv"),
                    matrix(numeric(), 0L, 3L, dimnames = list(NULL, c("L", "u", "v")))),
          identical(convert(empty, "Lab", "Apple RGB"), empty))

white <- convert(c(100, 0, 0), "Lab", "XYZ", from.ref.white = "D50")
stopifnot(max(abs(white - c(.963950149162182, 1, .824128028549921))) < 1e-6,
          max(abs(convert(c(.2, .3, .4), "XYZ", "XYZ",
                          from.ref.white = "D50", to.ref.white = "D65") -
                  c(.208318429518569, .300682361014403, .526803770310632))) < 1e-6)
luv_xyz <- spaces$Luv$toXYZ(matrix(c(.2, .3, .4), 1L), c(.9, 2, 1.1))
stopifnot(identical(names(luv_xyz), c("X", "Y", "Z")),
          max(abs(luv_xyz - c(.000161610474277045, .000221411291975891,
                              -.000184344669435284))) < 1e-9)
stopifnot(identical(as.vector(convert(c(Inf, 0, 0), "Luv", "XYZ")), c(NaN, Inf, NaN)),
          identical(as.vector(convert(c(0, Inf, Inf), "Luv", "XYZ")), c(0, 0, 0)))

custom <- grDevices::make.rgb(c(.64, .33), c(.30, .60), c(.15, .06),
                              name = "Test RGB", gamma = c(named = "sRGB"))
stopifnot(inherits(custom, "RGBcolorConverter"),
          identical(custom$reference.white, "D65"),
          identical(convert(x, custom, "XYZ"), convert(x, spaces$sRGB, "XYZ")))
named <- grDevices::make.rgb(c(x = .64, y = .33), c(.30, .60), c(.15, .06))
stopifnot(identical(colnames(named$toXYZ(x)), c("x", "", "y")))

double <- grDevices::colorConverter(function(color, white) color * 2,
                                   function(xyz, white) xyz / 2, "Double")
stopifnot(identical(convert(x, double, "XYZ"), x * 2),
          identical(convert(x, "XYZ", double), x / 2))
scalar <- grDevices::colorConverter(function(color, white) color[1L],
                                   function(xyz, white) xyz[1L], "Scalar")
lazy <- grDevices::colorConverter(stop("Unused callback"),
                                 function(xyz, white) xyz[1L], "Lazy")
stopifnot(identical(scalar$fromXYZ(color = matrix(1:6, 2L), white = NULL),
                    matrix(1:2, 2L)),
          identical(scalar$fromXYZ(color = matrix(integer(), 0L, 3L), white = NULL),
                    matrix(integer(), 0L, 0L)),
          identical(lazy$fromXYZ(matrix(1:6, 2L), NULL), matrix(1:2, 2L)))

text <- grDevices::colorConverter(
    function(color, white) matrix(as.double(color), nrow = nrow(color)),
    function(xyz, white) as.character(xyz[, 1L]),
    "Text", vectorized = TRUE)
stopifnot(identical(convert(c("1", "2", "3"), text, "XYZ", scale.in = NULL),
                    matrix(c(1, 2, 3), 1L)),
          identical(convert(c(1, 2, 3), "XYZ", text, scale.out = NULL), matrix("1", 1L)),
          identical(convert(data.frame(r = .1, g = .2, b = .3), "sRGB", "Lab"),
                    convert(c(.1, .2, .3), "sRGB", "Lab")))
frame <- data.frame(r = c(.1, .4), g = c(.2, .5), b = c(.3, .6),
                    row.names = c("one", "two"))
stopifnot(identical(rownames(convert(frame, "sRGB", "Lab")), c("one", "two")))
"#,
    );
}

const COLOR_PAIRS: &str = r#"
spaces <- c("XYZ", "Apple RGB", "sRGB", "CIE RGB", "Lab", "Luv")
sample <- matrix(c(.2, .65, 0, .04045, -.07, 1, 0, .6, .4, .3, 0, .015),
                 4L, dimnames = list(letters[1:4], c("p", "q", "r")))
samples <- list(XYZ = sample, "Apple RGB" = sample, sRGB = sample, "CIE RGB" = sample,
                Lab = sample * 100, Luv = sample * 100)
pairs <- list(from = rep(spaces, times = length(spaces)),
              to = rep(spaces, each = length(spaces)))
run <- function(i) {
    from <- pairs$from[i]
    to <- pairs$to[i]
    suppressWarnings(grDevices::convertColor(samples[[from]], from, to, clip = FALSE))
}
"#;

#[test]
fn convert_color_matches_live_r_for_every_builtin_pair() {
    let mut host = NativeHost::new();
    let Some(oracle) = host.env_var("ROXIDE_ORACLE_RSCRIPT") else {
        return;
    };
    let probe = format!(
        "setTimeLimit(elapsed = 20)\nstopifnot(getRversion() == '4.6.2')\n{COLOR_PAIRS}\n\
         dput(lapply(seq_along(pairs$from), run))"
    );
    let output = host
        .spawn(&CommandSpec {
            program: String::from_utf8(oracle).expect("Read the oracle path"),
            args: vec!["--vanilla".into(), "-e".into(), probe],
            stdin: None,
            timeout: Some(Duration::from_secs(25)),
        })
        .expect("Run the GNU R oracle");
    assert_eq!(
        output.status,
        0,
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected = String::from_utf8(output.stdout).expect("Read the oracle matrices");
    let script = format!(
        r#"
{COLOR_PAIRS}
expected <- {expected}
for (i in seq_along(expected)) {{
    observed <- run(i)
    target <- expected[[i]]
    if (!identical(dim(observed), dim(target)) ||
        !identical(dimnames(observed), dimnames(target)) ||
        !identical(is.na(observed), is.na(target)) ||
        !identical(is.nan(observed), is.nan(target)) ||
        any(abs(observed - target) > 1e-6, na.rm = TRUE))
        stop(sprintf("Color conversion differs for %s to %s", pairs$from[i], pairs$to[i]))
}}
"#
    );
    run_color_test("color_pairs.R", &script);
}

#[test]
fn device_table_keeps_slot_order_and_uses_device_dimensions() {
    let mut ctx = graphics_context(GcPolicy::Torture);
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
    let mut ctx = graphics_context(GcPolicy::Torture);
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
    let mut ctx = graphics_context(GcPolicy::Torture);
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
