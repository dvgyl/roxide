#![forbid(unsafe_code)]

//! Check file structure, rendered pixels, and R constructors.

use std::{io::Read, path::Path, time::Duration};

use flate2::{Crc, read::ZlibDecoder};
use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_graphics::engine::{
    Device, DeviceSpec, GContext, GraphicsExt, LineEnd, LineJoin, Rect, Rgba, dl::DisplayList,
    open_device,
};
use roxide_host::{CommandSpec, Host, Limits, NativeHost, OpenMode, PureHost};
use roxide_value::{GLOBAL_ENV, ValueApi};

const RED: Rgba = Rgba::new(255, 0, 0, 128);
const BLUE: Rgba = Rgba::new(0, 0, 255, 255);

fn pdf(file: &str, onefile: bool, compress: bool) -> DeviceSpec {
    DeviceSpec::Pdf {
        file: file.into(),
        width: 4.0,
        height: 3.0,
        onefile,
        compress,
        bg: Rgba::TRANSPARENT,
        pointsize: 12.0,
    }
}

fn svg(file: &str, onefile: bool) -> DeviceSpec {
    DeviceSpec::Svg {
        filename: file.into(),
        width: 4.0,
        height: 3.0,
        onefile,
        bg: Rgba::WHITE,
        pointsize: 12.0,
    }
}

fn png(file: &str, width_px: u32, height_px: u32, res: f64) -> DeviceSpec {
    DeviceSpec::Png {
        filename: file.into(),
        width_px,
        height_px,
        res,
        bg: Rgba::WHITE,
        pointsize: 12.0,
        device_name: "png".into(),
    }
}

fn read(host: &mut dyn Host, path: &str) -> Vec<u8> {
    let mut output = Vec::new();
    host.open(Path::new(path), OpenMode::read_only())
        .expect("open device output")
        .read_to_end(&mut output)
        .expect("read device output");
    output
}

fn draw(device: &mut dyn Device) {
    let gc = GContext {
        col: RED,
        fill: BLUE,
        lwd: 2.0,
        lty: vec![2.0, 1.0],
        lend: LineEnd::Square,
        ljoin: LineJoin::Mitre,
        ..GContext::default()
    };
    device.clip(Rect::normalized(10.0, 10.0, 280.0, 210.0));
    device.polyline(&[(8.0, 20.0), (70.0, 30.0), (90.0, 15.0)], &gc);
    device.polygon(&[(15.0, 40.0), (35.0, 40.0), (25.0, 60.0)], &gc);
    device.path(
        &[
            &[(40.0, 50.0), (80.0, 50.0), (65.0, 80.0)],
            &[(55.0, 56.0), (70.0, 56.0), (63.0, 66.0)],
        ],
        false,
        &gc,
    );
    device.rect(Rect::normalized(95.0, 40.0, 125.0, 80.0), &gc);
    device.circle((135.0, 65.0), 12.0, &gc);
    device.text((100.0, 100.0), "A<&(\\)é", 30.0, 0.5, &gc);
}

fn render(host: &mut dyn Host, spec: &DeviceSpec) {
    let mut device = open_device(host, spec).expect("open graphics device");
    for _ in 0..2 {
        device.new_page(host, Rgba::WHITE).expect("start a page");
        draw(device.as_mut());
    }
    device.close(host).expect("close graphics device");
    device.close(host).expect("close is idempotent");
}

fn check_xref(bytes: &[u8]) {
    let text = String::from_utf8_lossy(bytes);
    let (_, tail) = text.rsplit_once("startxref\n").expect("PDF startxref");
    let offset: usize = tail.lines().next().unwrap().parse().unwrap();
    let xref = std::str::from_utf8(&bytes[offset..]).expect("ASCII xref");
    let mut lines = xref.lines();
    assert_eq!(lines.next(), Some("xref"));
    let count: usize = lines
        .next()
        .unwrap()
        .strip_prefix("0 ")
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(lines.next(), Some("0000000000 65535 f "));
    for id in 1..count {
        let offset: usize = lines.next().unwrap()[..10].parse().unwrap();
        assert!(bytes[offset..].starts_with(format!("{id} 0 obj\n").as_bytes()));
    }
    assert_eq!(lines.next(), Some("trailer"));
    assert!(text.ends_with("%%EOF\n"));
}

#[test]
fn pdf_pages_streams_and_resources() {
    let spec = pdf("multi.pdf", true, true);
    let mut host = PureHost::new();
    render(&mut host, &spec);
    let bytes = read(&mut host, "multi.pdf");
    let text = String::from_utf8_lossy(&bytes);
    assert!(bytes.starts_with(b"%PDF-1.4"));
    check_xref(&bytes);
    for field in [
        "/Count 2",
        "/MediaBox [0 0 288 216]",
        "/Filter /FlateDecode",
        "/ExtGState",
        "/ca 1 /CA 0.501961",
        "/BaseFont /Helvetica-BoldOblique",
        "/CreationDate (D:20231114221320Z)",
    ] {
        assert!(text.contains(field), "{field}");
    }
    let start = bytes
        .windows(7)
        .position(|bytes| bytes == b"stream\n")
        .unwrap()
        + 7;
    let mut decoded = String::new();
    ZlibDecoder::new(&bytes[start..])
        .read_to_string(&mut decoded)
        .unwrap();
    for command in [
        " re\nW n", " m\n", " l\n", "h\nB*", " c\n", " Tm\n", " Tj\n", "ET\nQ\n",
    ] {
        assert!(decoded.contains(command), "{command}\n{decoded}");
    }
    assert!(decoded.contains(r"\(\\\)\351"));
    let mut second = PureHost::new();
    render(&mut second, &spec);
    assert_eq!(bytes, read(&mut second, "multi.pdf"));

    render(&mut host, &pdf("m%02d.pdf", false, false));
    for name in ["m01.pdf", "m02.pdf"] {
        let bytes = read(&mut host, name);
        check_xref(&bytes);
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/Count 1"));
        assert!(text.contains("stream\nq\n"));
        assert!(!text.contains("/Filter"));
    }
}

#[test]
fn svg_pages_clipping_text_and_opacity() {
    let mut host = PureHost::new();
    render(&mut host, &svg("multi%02d.svg", true));
    render(&mut host, &svg("page%02d.svg", false));
    for (file, pages) in [("multi01.svg", 2), ("page01.svg", 0), ("page02.svg", 0)] {
        let text = String::from_utf8(read(&mut host, file)).unwrap();
        assert_eq!(text.matches("<page>").count(), pages);
        assert_eq!(text.matches("<pageSet>").count(), usize::from(pages > 0));
        for part in [
            "version=\"1.1\"",
            "width=\"288pt\"",
            "height=\"216pt\"",
            "viewBox=\"0 0 288 216\"",
            "stroke-opacity=\"0.501961\"",
            "<clipPath",
            "<polyline",
            "<polygon",
            "<circle",
            "fill-rule=\"evenodd\"",
            "rotate(-30",
            "A&lt;&amp;",
            "8,196",
        ] {
            assert!(text.contains(part), "{part}");
        }
    }
}

struct Png {
    width: usize,
    height: usize,
    ppm: u32,
    rows: Vec<u8>,
}

impl Png {
    fn pixel(&self, x: usize, y: usize) -> &[u8] {
        let start = (self.height - 1 - y) * (1 + self.width * 4) + 1 + x * 4;
        &self.rows[start..start + 4]
    }
}

fn be(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(bytes.try_into().unwrap())
}

fn decode_png(file: &[u8]) -> Png {
    assert_eq!(&file[..8], b"\x89PNG\r\n\x1a\n");
    let (mut width, mut height, mut ppm) = (0, 0, 0);
    let mut compressed = Vec::new();
    let mut pos = 8;
    loop {
        let len = be(&file[pos..pos + 4]) as usize;
        let chunk = &file[pos + 4..pos + 8 + len];
        let crc_value = be(&file[pos + 8 + len..pos + 12 + len]);
        let mut crc = Crc::new();
        crc.update(chunk);
        assert_eq!(crc.sum(), crc_value);
        pos += 12 + len;
        match &chunk[..4] {
            b"IHDR" => {
                assert_eq!(len, 13);
                width = be(&chunk[4..8]) as usize;
                height = be(&chunk[8..12]) as usize;
                assert_eq!(&chunk[12..], &[8, 6, 0, 0, 0]);
            }
            b"pHYs" => {
                ppm = be(&chunk[4..8]);
                assert_eq!(be(&chunk[8..12]), ppm);
                assert_eq!(chunk[12], 1);
            }
            b"IDAT" => compressed.extend_from_slice(&chunk[4..]),
            b"IEND" => {
                assert_eq!(len, 0);
                break;
            }
            _ => {}
        }
    }
    assert_eq!(pos, file.len());
    let mut rows = Vec::new();
    ZlibDecoder::new(&compressed[..])
        .read_to_end(&mut rows)
        .unwrap();
    assert_eq!(rows.len(), height * (1 + width * 4));
    for row in rows.chunks_exact(1 + width * 4) {
        assert_eq!(row[0], 0);
    }
    Png {
        width,
        height,
        ppm,
        rows,
    }
}

#[test]
fn png_pages_crc_resolution_and_bounds() {
    let mut host = PureHost::new();
    for (width, height, res, ppm) in [(300, 240, 72.0, 2835), (400, 300, 100.0, 3937)] {
        render(&mut host, &png("p%02d.png", width, height, res));
        for name in ["p01.png", "p02.png"] {
            let image = decode_png(&read(&mut host, name));
            assert_eq!(
                (image.width, image.height, image.ppm),
                (width as usize, height as usize, ppm)
            );
            assert!(
                (0..image.height)
                    .any(|y| (0..image.width).any(|x| image.pixel(x, y) == [0, 0, 255, 255]))
            );
            assert_eq!(image.pixel(0, 0), [255; 4]);
        }
    }
    let too_large = open_device(&mut host, &png("bad.png", 32_768, 32_768, 72.0));
    assert!(too_large.err().unwrap().contains("host vector limit"));
    for name in ["unsafe%s.png", "p%099d.png", "%d%d.png"] {
        assert!(open_device(&mut host, &png(name, 1, 1, 72.0)).is_err());
    }
}

#[test]
fn raster_winding_clip_dashes_and_text() {
    let mut host = PureHost::new();
    let mut d = open_device(&mut host, &png("r%d.png", 80, 64, 72.0)).unwrap();
    let fill = GContext {
        col: Rgba::TRANSPARENT,
        fill: RED,
        ..GContext::default()
    };
    let line = GContext {
        lend: LineEnd::Butt,
        lwd: 4.0,
        lty: vec![1.0, 1.0],
        ..GContext::default()
    };
    for winding in [false, true] {
        d.new_page(&mut host, Rgba::TRANSPARENT).unwrap();
        d.clip(Rect::normalized(7.0, 0.0, 80.0, 64.0));
        d.path(
            &[
                &[(2.0, 2.0), (30.0, 2.0), (30.0, 30.0), (2.0, 30.0)],
                &[(10.0, 10.0), (20.0, 10.0), (20.0, 20.0), (10.0, 20.0)],
            ],
            winding,
            &fill,
        );
        d.polyline(&[(10.0, 33.0), (45.0, 33.0)], &line);
        d.text((10.0, 40.0), "Hershey", 0.0, 0.0, &GContext::default());
    }
    d.close(&mut host).unwrap();
    for (name, center) in [("r1.png", [0; 4]), ("r2.png", [255, 0, 0, 128])] {
        let image = decode_png(&read(&mut host, name));
        assert_eq!(image.pixel(5, 12), [0; 4]);
        assert_eq!(image.pixel(7, 12), [255, 0, 0, 128]);
        assert_eq!(image.pixel(15, 15), center);
        assert_eq!(image.pixel(11, 32), [0, 0, 0, 255]);
        assert_eq!(image.pixel(14, 32), [0; 4]);
        assert!((40..55).any(|y| (10..60).any(|x| image.pixel(x, y)[3] != 0)));
    }
}

fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-10, "{actual} != {expected}");
}

#[test]
fn helvetica_faces_and_kerning() {
    let mut host = PureHost::new();
    let pdf = open_device(&mut host, &pdf("metrics.pdf", true, true)).unwrap();
    let svg = open_device(&mut host, &svg("metrics.svg", true)).unwrap();
    let png = open_device(&mut host, &png("metrics.png", 1, 1, 100.0)).unwrap();
    for device in [&pdf, &svg, &png] {
        for (face, width) in [(1, 14.676), (2, 15.336), (3, 14.676), (4, 15.336)] {
            let gc = GContext {
                fontface: face,
                ..GContext::default()
            };
            close(device.str_width("Aé", &gc), width);
            close(device.metric_info(' ', &gc).2, 3.336);
            close(
                device.metric_info('g', &gc).2,
                if face % 2 == 0 { 7.332 } else { 6.672 },
            );
        }
    }
    let gc = GContext::default();
    close(pdf.str_width("AV", &gc), 15.168);
    close(svg.str_width("AV", &gc), 16.008);
    let (ascent, descent, _) = pdf.metric_info('g', &gc);
    close(ascent, 6.456);
    close(descent, 2.64);
}

fn r(ctx: &mut Ctx, script: &str) {
    let (result, stderr) = ctx
        .capture_streams(false, true, false, |ctx| {
            Ok(ctx.run_script("devices_parity.R", script, GLOBAL_ENV, false))
        })
        .unwrap();
    assert!(
        result.is_ok(),
        "{script}\n{}",
        String::from_utf8_lossy(&stderr)
    );
}

fn context() -> Ctx {
    let mut ctx = Ctx::new(Box::new(PureHost::new()), GcPolicy::Torture).unwrap();
    roxide_base::register_all(&mut ctx).unwrap();
    roxide_graphics::register(&mut ctx).unwrap();
    ctx
}

#[test]
fn constructors_open_slots_under_gc_torture() {
    let mut ctx = context();
    r(
        &mut ctx,
        r##"
    stopifnot(identical(withVisible(grDevices::pdf("/tmp/r.pdf", 4, 3,
        title = "Example", useKerning = FALSE, compress = FALSE)), list(value = NULL, visible = FALSE)))
    stopifnot(is.null(svg("/tmp/s%02d.svg", 4, 3, onefile = TRUE, bg = "#11223380")))
    options(bitmapType = "quartz")
    stopifnot(is.null(png("/tmp/q%02d.png", 300, 240, bg = "gray50")))
    stopifnot(is.null(png("/tmp/c%02d.png", 4, 3, "in", res = 100, type = "cairo")))
    "##,
    );
    assert_eq!(ctx.ext::<GraphicsExt>().open_ids(), [2, 3, 4, 5]);
    for (i, name) in ["pdf", "svg", "quartz_off_screen", "png"]
        .into_iter()
        .enumerate()
    {
        let mut slot = ctx.ext::<GraphicsExt>().slots[i + 2].take().unwrap();
        assert_eq!(slot.device.name(), name);
        draw(slot.device.as_mut());
        slot.device.close(ctx.host()).unwrap();
    }
    let pdf = String::from_utf8_lossy(&read(ctx.host(), "/tmp/r.pdf")).into_owned();
    assert!(pdf.contains("/Title (Example)"));
    assert!(!pdf.contains("/FlateDecode"));
    let svg = String::from_utf8(read(ctx.host(), "/tmp/s01.svg")).unwrap();
    assert!(svg.contains("<page>"));
    assert!(svg.contains(r##"fill="#112233" fill-opacity="0.501961""##));
    let quartz = decode_png(&read(ctx.host(), "/tmp/q01.png"));
    assert_eq!((quartz.width, quartz.height), (300, 240));
    assert_eq!(quartz.pixel(0, 0), [127, 127, 127, 255]);
    let cairo = decode_png(&read(ctx.host(), "/tmp/c01.png"));
    assert_eq!((cairo.width, cairo.height), (400, 300));
}

#[test]
fn display_list_snapshot_survives_gc_and_replays_in_order() {
    let mut ctx = context();
    r(
        &mut ctx,
        "n <- 1L; commands <- list(quote(n <- n + 2L), quote(n <- n * 3L))",
    );
    let name = ctx.session_mut().intern("commands").unwrap();
    let value = ctx.eval(name.as_sexp(), GLOBAL_ENV).unwrap();
    let entries = ctx.session().list_slice(value).unwrap().to_vec();
    let device = Box::new(roxide_graphics::engine::RecordingDevice::new(
        "recording",
        4.0,
        3.0,
    ));
    let ext = ctx.ext::<GraphicsExt>();
    ext.open_slot(device).unwrap();
    let list = &mut ext.active_mut().unwrap().display_list;
    list.set_enabled(true);
    for entry in entries {
        list.push(entry);
    }
    r(&mut ctx, "commands <- NULL");
    ctx.safepoint();
    let entries = ctx
        .ext::<GraphicsExt>()
        .active()
        .unwrap()
        .display_list
        .entries()
        .to_vec();
    let snapshot = DisplayList::snapshot_entries(&mut ctx, &entries).unwrap();
    ctx.protect(snapshot);
    ctx.ext::<GraphicsExt>()
        .active_mut()
        .unwrap()
        .display_list
        .clear();
    DisplayList::replay(&mut ctx, snapshot, GLOBAL_ENV).unwrap();
    r(&mut ctx, "stopifnot(identical(n, 9L))");
}

const ORACLE_PNG_PROBE: &str = r#"
    check_png <- function(type, name, width, height, units, res, w, h) {
        suppressWarnings(png(f(name), width, height, units, type = type, res = res))
        if (dev.cur() == 1L) return(FALSE)
        stopifnot(names(dev.cur()) == if (type == "quartz") "quartz_off_screen" else "png")
        par(mar = rep(0, 4))
        for (i in 1:2) {
            plot.new()
            rect(.1, .1, .9, .9, col = "blue")
        }
        invisible(dev.off())
        for (i in 1:2) {
            con <- file(f(sprintf(name, i)), "rb")
            stopifnot(identical(as.integer(readBin(con, "raw", 8L)), c(137L,80L,78L,71L,13L,10L,26L,10L)))
            stopifnot(readBin(con, "integer", 1L, endian = "big") == 13L)
            stopifnot(rawToChar(readBin(con, "raw", 4L)) == "IHDR")
            stopifnot(identical(readBin(con, "integer", 2L, endian = "big"), c(w, h)))
            close(con)
        }
        TRUE
    }
"#;

fn oracle(script: &str) -> Option<String> {
    let mut host = NativeHost::new();
    let oracle = host.env_var("ROXIDE_ORACLE_RSCRIPT")?;
    let script = format!(
        "setTimeLimit(elapsed = 20)
stopifnot(getRversion() == '4.6.2')
f <- function(name) file.path(tempdir(), name)
{ORACLE_PNG_PROBE}
{script}"
    );
    let result = host
        .spawn(&CommandSpec {
            program: String::from_utf8(oracle).unwrap(),
            args: vec!["--vanilla".into(), "-e".into(), script],
            stdin: None,
            timeout: Some(Duration::from_secs(25)),
        })
        .unwrap();
    assert_eq!(
        result.status,
        0,
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    Some(String::from_utf8(result.stdout).unwrap())
}

#[test]
fn live_r_pdf_and_quartz() {
    let script = r#"
        pdf(f("multi.pdf"), width = 4, height = 3)
        plot.new()
        cat(sprintf("%.3f\n", strwidth("AV", units = "inches") * 72))
        plot.new()
        invisible(dev.off())
        con <- file(f("multi.pdf"), "rb")
        stopifnot(rawToChar(readBin(con, "raw", 8L)) == "%PDF-1.4")
        close(con)
        lines <- suppressWarnings(readLines(f("multi.pdf"), skipNul = TRUE))
        stopifnot(any(grepl("/Count 2", lines, fixed = TRUE, useBytes = TRUE)))
        stopifnot(any(grepl("/MediaBox [0 0 288 216]", lines, fixed = TRUE, useBytes = TRUE)))
        pdf(f("m%02d.pdf"), width = 4, height = 3, onefile = FALSE)
        plot.new()
        plot.new()
        invisible(dev.off())
        stopifnot(file.exists(f("m01.pdf"), f("m02.pdf")))
    "#;
    if let Some(stdout) = oracle(script) {
        let mut pure = PureHost::new();
        let dev = open_device(&mut pure, &pdf("oracle.pdf", true, true)).unwrap();
        assert_eq!(
            stdout.trim(),
            format!("{:.3}", dev.str_width("AV", &GContext::default()))
        );
    }
    oracle(
        r#"if (Sys.info()[["sysname"]] == "Darwin") {
        stopifnot(check_png("quartz", "q%02d.png", 300, 240, "px", NA, 300L, 240L))
    }"#,
    );
}

#[test]
fn live_r_cairo_png_and_svg() {
    let script = r##"
        if (!check_png("cairo", "c%02d.png", 4, 3, "in", 100, 400L, 300L)) {
            cat("cairo unavailable\n")
        } else {
            svg(f("multi%02d.svg"), width = 4, height = 3, onefile = TRUE)
            for (i in 1:2) {
                plot.new()
                plot.window(c(0, 1), c(0, 1))
                segments(.1, .1, .9, .9, col = "#FF000080")
            }
            invisible(dev.off())
            lines <- readLines(f("multi01.svg"))
            for (part in c('width="288pt"', 'height="216pt"', 'viewBox="0 0 288 216"', '<pageSet>', '<page>', 'stroke-opacity="0.501961"'))
                if (!any(grepl(part, lines, fixed = TRUE))) stop(paste("missing SVG field", part))
            svg(f("page%02d.svg"), width = 4, height = 3)
            for (i in 1:2) plot.new()
            invisible(dev.off())
            stopifnot(file.exists(f("page01.svg"), f("page02.svg")))
        }
    "##;
    if oracle(script).is_some_and(|s| s.contains("cairo unavailable")) {
        eprintln!("GNU R Cairo is unavailable. The Roxide Cairo file checks ran separately.");
    }
}

#[test]
fn png_devices_share_and_release_framebuffer_budget() {
    let host = NativeHost::with_limits(Limits {
        max_vector_bytes: 1 << 20,
        ..Limits::default()
    });
    let mut ctx = Ctx::new(Box::new(host), GcPolicy::default()).unwrap();
    roxide_base::register_all(&mut ctx).unwrap();
    roxide_graphics::register(&mut ctx).unwrap();
    r(
        &mut ctx,
        r#"
        png("/tmp/a.png", 400, 400)
        fail <- tryCatch(png("/tmp/b.png", 400, 400), error = function(e) conditionMessage(e))
        stopifnot(identical(fail, "PNG framebuffer exceeds the host vector limit"),
                  length(dev.list()) == 1L)
        dev.off()
        png("/tmp/b.png", 400, 400)
        stopifnot(length(dev.list()) == 1L)
        "#,
    );
    assert_eq!(ctx.ext::<GraphicsExt>().framebuffer_bytes(), 640_000);
    r(&mut ctx, "dev.off()");
    assert_eq!(ctx.ext::<GraphicsExt>().framebuffer_bytes(), 0);
}
