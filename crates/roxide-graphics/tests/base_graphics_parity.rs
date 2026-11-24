#![forbid(unsafe_code)]

//! Check base graphics state, recorded calls, and rendered files.

use std::{io::Read, path::Path};

use flate2::read::ZlibDecoder;
use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_graphics::engine::{GraphicsExt, RecordingDevice, dl::DisplayList};
use roxide_host::{OpenMode, PureHost};
use roxide_value::GLOBAL_ENV;

fn run(ctx: &mut Ctx, script: &str) {
    let (result, stderr) = ctx
        .capture_streams(false, true, false, |ctx| {
            Ok(ctx.run_script("base_graphics_parity.R", script, GLOBAL_ENV, false))
        })
        .expect("capture graphics results");
    assert!(
        result.is_ok(),
        "{script}\n{}",
        String::from_utf8_lossy(&stderr)
    );
}

fn context(policy: GcPolicy) -> Ctx {
    let mut ctx = Ctx::new(Box::new(PureHost::new()), policy).expect("create session");
    roxide_base::register_all(&mut ctx).expect("register base");
    roxide_graphics::register(&mut ctx).expect("register graphics");
    ctx
}

fn output(ctx: &mut Ctx, path: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    ctx.host()
        .open(Path::new(path), OpenMode::read_only())
        .expect("open output file")
        .read_to_end(&mut bytes)
        .expect("read output file");
    bytes
}

#[test]
fn base_state_is_per_device_and_display_list_survives_gc() {
    for policy in [GcPolicy::default(), GcPolicy::Torture] {
        let mut ctx = context(policy);
        ctx.ext::<GraphicsExt>()
            .open_slot(Box::new(RecordingDevice::new("recording", 8.0, 4.0)))
            .expect("open recording device");
        run(
            &mut ctx,
            r#"
            par(mfrow = c(2,2), mar = rep(0,4))
            stopifnot(identical(par("cex"), .83), par("page"))
            plot.new()
            plot.window(c(1,100), c(0,8), log = "x")
            stopifnot(all(abs(grconvertX(c(1,10,100), "user", "npc") - c(1/27,.5,26/27)) < 1e-10))
            points(1:3, 1:3, pch = 19)
            plot.new()
            plot.window(c(0,1), c(1,0), xaxs = "i", yaxs = "i")
            segments(.25,.25,.75,.75)
            stopifnot(identical(par("mfg"), c(1L,2L,2L,2L)))
            pdf("/tmp/other.pdf")
            stopifnot(identical(par("mfrow"), c(1L,1L)))
            dev.off()
            stopifnot(identical(par("mfg"), c(1L,2L,2L,2L)))
        "#,
        );
        ctx.safepoint();
        let slot = ctx.ext::<GraphicsExt>().active().expect("recording device");
        assert_eq!(slot.page_count, 1);
        let entries = slot.display_list.entries().to_vec();
        assert_eq!(entries.len(), 6);
        let snapshot = DisplayList::snapshot_entries(&mut ctx, &entries).expect("recorded calls");
        ctx.protect(snapshot);
        ctx.ext::<GraphicsExt>()
            .open_slot(Box::new(RecordingDevice::new("recording", 8.0, 4.0)))
            .expect("open replay device");
        run(&mut ctx, "par(mfrow = c(2,2), mar = rep(0,4))");
        DisplayList::replay(&mut ctx, snapshot, GLOBAL_ENV).expect("replay recorded calls");
        run(
            &mut ctx,
            r#"stopifnot(identical(par("mfg"), c(1L,2L,2L,2L)))"#,
        );
    }
}

#[test]
fn base_functions_draw_on_pdf_svg_and_png() {
    for policy in [GcPolicy::default(), GcPolicy::Torture] {
        let mut ctx = context(policy);
        for open in [
            r#"pdf("/tmp/base.pdf", 4, 3, compress = FALSE)"#,
            r#"svg("/tmp/base.svg", 4, 3)"#,
            r#"png("/tmp/base.png", 288, 216, res = 72)"#,
        ] {
            run(
                &mut ctx,
                &format!(
                    r#"
                {open}
                par(mar = rep(0,4), bg = "white")
                plot.new()
                plot.window(c(0,1), c(0,1), xaxs = "i", yaxs = "i")
                lines(c(.1,.2,NA,.6,.9), c(.2,.3,NA,.6,.7))
                rect(.2,.2,.8,.8, col = "red", border = NA)
                points(.5,.5, pch = 21, bg = "blue")
                text(.25,.7, "ABC")
                dev.off()
            "#
                ),
            );
        }
        let pdf = String::from_utf8_lossy(&output(&mut ctx, "/tmp/base.pdf")).into_owned();
        assert!(pdf.contains("/Count 1"));
        assert!(pdf.contains("/MediaBox [0 0 288 216]"));
        assert!(pdf.contains("57.6 43.2 172.8 129.6 re"));
        let svg = String::from_utf8(output(&mut ctx, "/tmp/base.svg")).expect("SVG text");
        assert_eq!(svg.matches("<polyline ").count(), 2);
        assert!(svg.contains(r##"fill="#FF0000""##));
        assert!(svg.contains("<circle "));
        assert!(svg.contains(">ABC</text>"));
        let png = output(&mut ctx, "/tmp/base.png");
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&png[16..24], &[0, 0, 1, 32, 0, 0, 0, 216]);
        run(
            &mut ctx,
            r#"
            pdf("/tmp/hatch.pdf", 4, 3, compress = FALSE)
            par(mar = rep(0,4))
            plot.new()
            rect(.2,.2,.8,.8, density = 2, angle = 0, border = NA)
            dev.off()
        "#,
        );
        let hatch = String::from_utf8_lossy(&output(&mut ctx, "/tmp/hatch.pdf")).into_owned();
        assert!(hatch.contains("64 80 m\n224 80 l"));
        let mut idat = Vec::new();
        let mut chunk = &png[8..];
        while !chunk.is_empty() {
            let len = u32::from_be_bytes(chunk[..4].try_into().expect("chunk size")) as usize;
            if &chunk[4..8] == b"IDAT" {
                idat.extend_from_slice(&chunk[8..8 + len]);
            }
            chunk = &chunk[12 + len..];
        }
        let mut rows = Vec::new();
        ZlibDecoder::new(idat.as_slice())
            .read_to_end(&mut rows)
            .expect("decode PNG pixels");
        let row = 100 * (1 + 4 * 288);
        assert_eq!(rows[row], 0);
        let pixel = row + 1 + 4 * 100;
        assert_eq!(&rows[pixel..pixel + 4], &[255, 0, 0, 255]);
    }
}

#[test]
fn record_and_replay_restore_state_and_keep_one_copy_of_the_calls() {
    let mut ctx = context(GcPolicy::default());
    run(
        &mut ctx,
        r#"
        pdf(NULL)
        plot.new()
        stopifnot(is.null(recordPlot()[[1L]]))
        dev.control("enable")
        par(mfrow = c(1L,2L), mar = rep(1,4L))
        plot.default(1:3, axes = FALSE, ann = FALSE)
        plot.default(c(1,10), c(3,5), log = "x", axes = FALSE, ann = FALSE)
        saved <- recordPlot()
        n <- length(saved[[1L]])
        expected <- par(c("mfrow","mfg","usr","xlog"))
        stopifnot(n > 0L, identical(attr(saved,"engineVersion"), 17L))
        pdf(NULL)
        dev.control("enable")
        replayPlot(saved)
        stopifnot(identical(par(c("mfrow","mfg","usr","xlog")), expected),
                  length(recordPlot()[[1L]]) == n)
        replayPlot(saved)
        stopifnot(length(recordPlot()[[1L]]) == n)
        "#,
    );
}

#[test]
fn base_and_grid_distinguish_blank_solid_and_numeric_dash_lines() {
    let mut ctx = context(GcPolicy::Torture);
    run(
        &mut ctx,
        r#"
        svg("/tmp/base-lines.svg", 2, 2)
        par(mar = rep(0, 4))
        plot.new()
        plot.window(c(0, 1), c(0, 1))
        lines(c(.2, .8), c(.2, .2), lty = 0)
        lines(c(.2, .8), c(.4, .4), lty = 1)
        lines(c(.2, .8), c(.6, .6), lty = 2)
        dev.off()
        svg("/tmp/grid-lines.svg", 2, 2)
        grid::grid.newpage()
        grid::grid.lines(c(.2, .8), c(.2, .2), gp = grid::gpar(lty = "blank"))
        grid::grid.lines(c(.2, .8), c(.4, .4), gp = grid::gpar(lty = 1))
        grid::grid.lines(c(.2, .8), c(.6, .6), gp = grid::gpar(lty = 2))
        dev.off()
        "#,
    );
    for path in ["/tmp/base-lines.svg", "/tmp/grid-lines.svg"] {
        let svg = String::from_utf8(output(&mut ctx, path)).expect("SVG text");
        let visible: Vec<_> = svg
            .lines()
            .filter(|line| line.contains("<polyline ") && line.contains(r##"stroke="#000000""##))
            .collect();
        assert_eq!(visible.len(), 2, "{path}\n{svg}");
        assert!(!visible[0].contains("stroke-dasharray"), "{path}\n{svg}");
        assert!(
            visible[1].contains(r#"stroke-dasharray="3 3""#),
            "{path}\n{svg}"
        );
    }
}
