//! Core graphics device trait, device factory contract, and per-session device table (`GraphicsExt`; ADR-0015).
//!
//! Ownership seam between Builders D and E:
//! - Builder D owns `devices/**` (including `pdf()`, `svg()`, `png()` R functions and `devices::open_device`),
//!   `engine/dl.rs`, and `graphics/**`.
//! - Builder E owns `grdevices/**` (including `dev.*` table operations calling `open_device` / `open_default_device`),
//!   and `grid/**`.

pub mod dl;

use std::any::Any;

use roxide_eval::{Ctx, CtxExt, EvalResult};
use roxide_gc::{SexpId, Trace, Tracer};
use roxide_host::{Host, HostResult};
use roxide_value::{GLOBAL_ENV, SEXP_NIL, ValueApi};

use dl::DisplayList;

/// Default R 4.x `palette()` colors.
pub const DEFAULT_PALETTE: &[&str] = &[
    "black", "#DF536B", "#61D04F", "#2297E6", "#28E2E5", "#CD0BBC", "#F5C710", "gray62",
];

/// Maximum number of graphics device slots (1..=63; slot 1 is the null device).
pub const MAX_DEVICES: usize = 63;

/// 8-bit sRGB color with alpha channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgba {
    /// Red channel (0..=255).
    pub r: u8,
    /// Green channel (0..=255).
    pub g: u8,
    /// Blue channel (0..=255).
    pub b: u8,
    /// Alpha channel (0 = transparent, 255 = opaque).
    pub a: u8,
}

impl Rgba {
    /// Opaque black (`#000000FF`).
    pub const BLACK: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    /// Opaque white (`#FFFFFFFF`).
    pub const WHITE: Self = Self {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    /// Fully transparent (`#FFFFFF00`).
    pub const TRANSPARENT: Self = Self {
        r: 255,
        g: 255,
        b: 255,
        a: 0,
    };

    /// Construct an [`Rgba`] color from individual channels.
    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// True if the color is completely transparent (`a == 0`).
    #[must_use]
    pub const fn is_transparent(self) -> bool {
        self.a == 0
    }
}

/// Axis-aligned rectangle in device coordinates (1/72 inch, bottom-left origin, y pointing up).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    /// Minimum x coordinate (points).
    pub x0: f64,
    /// Minimum y coordinate (points).
    pub y0: f64,
    /// Maximum x coordinate (points).
    pub x1: f64,
    /// Maximum y coordinate (points).
    pub y1: f64,
}

impl Rect {
    /// Construct a normalized rectangle (`x0 <= x1`, `y0 <= y1`).
    #[must_use]
    pub fn normalized(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Self {
            x0: x0.min(x1),
            y0: y0.min(y1),
            x1: x0.max(x1),
            y1: y0.max(y1),
        }
    }

    /// Width of the rectangle in device points.
    #[must_use]
    pub fn width(self) -> f64 {
        (self.x1 - self.x0).abs()
    }

    /// Height of the rectangle in device points.
    #[must_use]
    pub fn height(self) -> f64 {
        (self.y1 - self.y0).abs()
    }
}

/// Stroke line cap style (`par("lend")`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineEnd {
    /// Round cap (0 / `"round"`).
    #[default]
    Round = 0,
    /// Butt cap (1 / `"butt"`).
    Butt = 1,
    /// Square cap (2 / `"square"`).
    Square = 2,
}

/// Stroke line join style (`par("ljoin")`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineJoin {
    /// Round join (0 / `"round"`).
    #[default]
    Round = 0,
    /// Mitre join (1 / `"mitre"`).
    Mitre = 1,
    /// Bevel join (2 / `"bevel"`).
    Bevel = 2,
}

/// Device-independent graphics drawing context passed to [`Device`] primitives.
#[derive(Clone, Debug, PartialEq)]
pub struct GContext {
    /// Stroke / foreground color.
    pub col: Rgba,
    /// Fill / background color.
    pub fill: Rgba,
    /// Line width multiplier (1.0 = 1/96 in = 0.75 pt).
    pub lwd: f64,
    /// Dash pattern in units of `lwd` (empty = solid).
    pub lty: Vec<f64>,
    /// Stroke cap style.
    pub lend: LineEnd,
    /// Stroke join style.
    pub ljoin: LineJoin,
    /// Mitre limit.
    pub lmitre: f64,
    /// Character expansion multiplier.
    pub cex: f64,
    /// Base pointsize (typically 12.0).
    pub ps: f64,
    /// Font face (1 = plain, 2 = bold, 3 = italic, 4 = bold italic, 5 = symbol).
    pub fontface: u8,
    /// Font family name (`""`, `"sans"`, `"serif"`, `"mono"`).
    pub fontfamily: String,
    /// Line height multiplier.
    pub lineheight: f64,
}

impl Default for GContext {
    fn default() -> Self {
        Self {
            col: Rgba::BLACK,
            fill: Rgba::TRANSPARENT,
            lwd: 1.0,
            lty: Vec::new(),
            lend: LineEnd::Round,
            ljoin: LineJoin::Round,
            lmitre: 10.0,
            cex: 1.0,
            ps: 12.0,
            fontface: 1,
            fontfamily: String::new(),
            lineheight: 1.2,
        }
    }
}

/// Device construction specification passed across the Builder D / Builder E factory seam.
#[derive(Clone, Debug)]
pub enum DeviceSpec {
    /// PDF 1.4 vector device (`grDevices::pdf`).
    Pdf {
        /// Output file path or `printf`-style numbered pattern (e.g. `"Rplots.pdf"`, `"m%02d.pdf"`).
        file: String,
        /// Page width in inches (default 7.0).
        width: f64,
        /// Page height in inches (default 7.0).
        height: f64,
        /// Store all pages in one file (`true`) or numbered files (`false`).
        onefile: bool,
        /// Compress content streams with `FlateDecode` (`true` by default).
        compress: bool,
        /// Initial background color.
        bg: Rgba,
        /// Base pointsize (default 12.0).
        pointsize: f64,
    },
    /// SVG 1.1 vector device (`grDevices::svg`).
    Svg {
        /// Output file path or numbered pattern.
        filename: String,
        /// Width in inches (default 7.0).
        width: f64,
        /// Height in inches (default 7.0).
        height: f64,
        /// Multi-page `<pageSet>`/`<page>` (`true`) vs numbered files (`false`).
        onefile: bool,
        /// Initial background color.
        bg: Rgba,
        /// Base pointsize (default 12.0).
        pointsize: f64,
    },
    /// Headless raster PNG device (`grDevices::png`).
    Png {
        /// Output file path or numbered pattern.
        filename: String,
        /// Width in pixels.
        width_px: u32,
        /// Height in pixels.
        height_px: u32,
        /// Nominal resolution in PPI (default 72.0).
        res: f64,
        /// Initial background color.
        bg: Rgba,
        /// Base pointsize (default 12.0).
        pointsize: f64,
        /// Reported device name (`"quartz_off_screen"` or `"png"`).
        device_name: String,
    },
    /// In-memory headless recording device (used for unit tests and pre-device fallback).
    Recording {
        /// Device name (e.g. `"pdf"`).
        name: String,
        /// Width in inches.
        width: f64,
        /// Height in inches.
        height: f64,
    },
}

/// Backend graphics device contract in 1/72-inch, bottom-left origin, y-up coordinates (ADR-0015).
pub trait Device {
    /// Device name reported by `names(dev.cur())` (e.g. `"pdf"`, `"svg"`, `"png"`, `"quartz_off_screen"`).
    fn name(&self) -> &str;
    /// Device page dimensions in inches `(width_in, height_in)`.
    fn size_in(&self) -> (f64, f64);
    /// Start a new page with background color `bg` (flushing any previous page if applicable).
    fn new_page(&mut self, host: &mut dyn Host, bg: Rgba) -> HostResult<()>;
    /// Update the active clipping rectangle in device coordinates (points).
    fn clip(&mut self, r: Rect);
    /// Stroke an open polyline through `pts`.
    fn polyline(&mut self, pts: &[(f64, f64)], gc: &GContext);
    /// Fill and/or stroke a closed polygon through `pts`.
    fn polygon(&mut self, pts: &[(f64, f64)], gc: &GContext);
    /// Fill and/or stroke a compound path of closed subpaths (`winding`: true = nonzero, false = even-odd).
    fn path(&mut self, subpaths: &[&[(f64, f64)]], winding: bool, gc: &GContext);
    /// Fill and/or stroke an axis-aligned rectangle.
    fn rect(&mut self, r: Rect, gc: &GContext);
    /// Fill and/or stroke a circle centered at `c` with radius `r` (points).
    fn circle(&mut self, c: (f64, f64), r: f64, gc: &GContext);
    /// Draw text `s` anchored at `at`, rotated `rot` degrees CCW, with horizontal adjustment `hadj` in `[0, 1]`.
    fn text(&mut self, at: (f64, f64), s: &str, rot: f64, hadj: f64, gc: &GContext);
    /// Return the advance width of `s` in device points under `gc`.
    fn str_width(&self, s: &str, gc: &GContext) -> f64;
    /// Return `(ascent, descent, width)` in device points for character `c` under `gc`.
    fn metric_info(&self, c: char, gc: &GContext) -> (f64, f64, f64);
    /// Return the number of bytes held in a raster framebuffer.
    fn framebuffer_bytes(&self) -> usize {
        0
    }
    /// Return whether the device starts with display-list recording enabled.
    fn records_by_default(&self) -> bool {
        false
    }
    /// Finalize and write any buffered output via `host`.
    fn close(&mut self, host: &mut dyn Host) -> HostResult<()>;
}

/// Built-in null device occupying slot 1.
#[derive(Default)]
pub struct NullDevice;

impl Device for NullDevice {
    fn name(&self) -> &str {
        "null device"
    }
    fn size_in(&self) -> (f64, f64) {
        (0.0, 0.0)
    }
    fn new_page(&mut self, _host: &mut dyn Host, _bg: Rgba) -> HostResult<()> {
        Ok(())
    }
    fn clip(&mut self, _r: Rect) {}
    fn polyline(&mut self, _pts: &[(f64, f64)], _gc: &GContext) {}
    fn polygon(&mut self, _pts: &[(f64, f64)], _gc: &GContext) {}
    fn path(&mut self, _subpaths: &[&[(f64, f64)]], _winding: bool, _gc: &GContext) {}
    fn rect(&mut self, _r: Rect, _gc: &GContext) {}
    fn circle(&mut self, _c: (f64, f64), _r: f64, _gc: &GContext) {}
    fn text(&mut self, _at: (f64, f64), _s: &str, _rot: f64, _hadj: f64, _gc: &GContext) {}
    fn str_width(&self, _s: &str, _gc: &GContext) -> f64 {
        0.0
    }
    fn metric_info(&self, _c: char, _gc: &GContext) -> (f64, f64, f64) {
        (0.0, 0.0, 0.0)
    }
    fn close(&mut self, _host: &mut dyn Host) -> HostResult<()> {
        Ok(())
    }
}

/// Lightweight in-memory recording device usable by tests and Builder E before file backends land.
pub struct RecordingDevice {
    name: String,
    width: f64,
    height: f64,
    /// Number of drawing primitives observed on this device.
    pub ops: usize,
    /// Number of pages started on this device.
    pub pages: usize,
    /// True once `close()` has been called.
    pub closed: bool,
}

impl RecordingDevice {
    /// Construct a new recording device with the given name and inch dimensions.
    #[must_use]
    pub fn new(name: impl Into<String>, width: f64, height: f64) -> Self {
        Self {
            name: name.into(),
            width,
            height,
            ops: 0,
            pages: 0,
            closed: false,
        }
    }
}

impl Device for RecordingDevice {
    fn records_by_default(&self) -> bool {
        true
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn size_in(&self) -> (f64, f64) {
        (self.width, self.height)
    }
    fn new_page(&mut self, _host: &mut dyn Host, _bg: Rgba) -> HostResult<()> {
        self.pages += 1;
        Ok(())
    }
    fn clip(&mut self, _r: Rect) {}
    fn polyline(&mut self, _pts: &[(f64, f64)], _gc: &GContext) {
        self.ops += 1;
    }
    fn polygon(&mut self, _pts: &[(f64, f64)], _gc: &GContext) {
        self.ops += 1;
    }
    fn path(&mut self, _subpaths: &[&[(f64, f64)]], _winding: bool, _gc: &GContext) {
        self.ops += 1;
    }
    fn rect(&mut self, _r: Rect, _gc: &GContext) {
        self.ops += 1;
    }
    fn circle(&mut self, _c: (f64, f64), _r: f64, _gc: &GContext) {
        self.ops += 1;
    }
    fn text(&mut self, _at: (f64, f64), _s: &str, _rot: f64, _hadj: f64, _gc: &GContext) {
        self.ops += 1;
    }
    fn str_width(&self, s: &str, gc: &GContext) -> f64 {
        (s.chars().count() as f64) * gc.ps * gc.cex * 0.55
    }
    fn metric_info(&self, _c: char, gc: &GContext) -> (f64, f64, f64) {
        let sz = gc.ps * gc.cex;
        (sz * 0.72, sz * 0.22, sz * 0.55)
    }
    fn close(&mut self, _host: &mut dyn Host) -> HostResult<()> {
        self.closed = true;
        Ok(())
    }
}

/// Construct a concrete [`Device`] from a [`DeviceSpec`] (delegates to `crate::devices::open_device`).
pub fn open_device(host: &mut dyn Host, spec: &DeviceSpec) -> Result<Box<dyn Device>, String> {
    crate::devices::open_device(host, spec)
}

/// Open the session's default graphics device (invoked on demand by `dev.new()`, `dev.size()`,
/// `dev.set(1)`, and plotting functions when only the null device is active).
pub fn open_default_device(ctx: &mut Ctx) -> EvalResult<usize> {
    ctx.scoped(|ctx| {
        let dev_new = ctx.session_mut().intern("dev.new")?.as_sexp();
        let call = ctx.session_mut().lcons(dev_new, SEXP_NIL, SEXP_NIL)?;
        ctx.protect(call);
        ctx.eval(call, GLOBAL_ENV)?;
        let id = ctx.ext::<GraphicsExt>().current;
        if id == 1 {
            return Err(ctx.stop(SEXP_NIL, "no active graphics device"));
        }
        Ok(id)
    })
}

/// Active state held for one open graphics device slot.
pub struct DeviceSlot {
    /// Underlying graphics backend.
    pub device: Box<dyn Device>,
    /// Recorded display list for `recordPlot()` / `replayPlot()`.
    pub display_list: DisplayList,
    /// Whether new drawing calls are recorded.
    pub recording: bool,
    /// Optional typed Rust `par()` state owned by `graphics::par` (dropped automatically on `dev.off()`).
    pub par_data: Option<Box<dyn Any>>,
    /// Call that installed the current custom layout.
    pub par_state: SexpId,
    /// Base graphics state at the start of the recorded page.
    pub initial_state: SexpId,
    /// Per-device R-level `grid` state (`SEXP_NIL` until initialized).
    pub grid_state: SexpId,
    /// Whether `plot.new()` has initialized a base graphics frame on this device.
    pub frame_open: bool,
    /// Number of pages started on this device.
    pub page_count: usize,
}

impl DeviceSlot {
    /// Wrap a newly opened [`Device`] in a fresh [`DeviceSlot`].
    #[must_use]
    pub fn new(device: Box<dyn Device>) -> Self {
        let recording = device.records_by_default();
        let mut display_list = DisplayList::default();
        display_list.set_enabled(recording);
        Self {
            device,
            display_list,
            recording,
            par_data: None,
            par_state: SEXP_NIL,
            initial_state: SEXP_NIL,
            grid_state: SEXP_NIL,
            frame_open: false,
            page_count: 0,
        }
    }
}

/// Per-session graphics device table, current device slot, and color palette (`CtxExt`).
pub struct GraphicsExt {
    /// Device slots indexed by 1-based device ID (`1..=63`); slot 0 is unused, slot 1 is null device.
    pub slots: Vec<Option<DeviceSlot>>,
    /// Active 1-based device ID (1 when no user device is open).
    pub current: usize,
    /// Active color palette strings (default [`DEFAULT_PALETTE`]).
    pub palette: Vec<String>,
}

impl Default for GraphicsExt {
    fn default() -> Self {
        let mut slots = Vec::with_capacity(MAX_DEVICES + 1);
        for _ in 0..=MAX_DEVICES {
            slots.push(None);
        }
        slots[1] = Some(DeviceSlot::new(Box::new(NullDevice)));
        Self {
            slots,
            current: 1,
            palette: DEFAULT_PALETTE.iter().map(|&s| s.to_owned()).collect(),
        }
    }
}

impl GraphicsExt {
    /// Allocate the lowest free device slot (`2..=63`), make it current, and return its ID.
    pub fn open_slot(&mut self, device: Box<dyn Device>) -> Option<usize> {
        for id in 2..=MAX_DEVICES {
            if self.slots[id].is_none() {
                self.slots[id] = Some(DeviceSlot::new(device));
                self.current = id;
                return Some(id);
            }
        }
        None
    }

    /// Borrow the currently active [`DeviceSlot`].
    #[must_use]
    pub fn active(&self) -> Option<&DeviceSlot> {
        self.slots.get(self.current).and_then(Option::as_ref)
    }

    /// Mutably borrow the currently active [`DeviceSlot`].
    #[must_use]
    pub fn active_mut(&mut self) -> Option<&mut DeviceSlot> {
        self.slots.get_mut(self.current).and_then(Option::as_mut)
    }

    /// Return the total framebuffer bytes held by open devices.
    pub fn framebuffer_bytes(&self) -> usize {
        self.slots.iter().flatten().fold(0usize, |total, slot| {
            total.saturating_add(slot.device.framebuffer_bytes())
        })
    }

    /// Return all open non-null device IDs in ascending order.
    #[must_use]
    pub fn open_ids(&self) -> Vec<usize> {
        (2..=MAX_DEVICES)
            .filter(|&id| self.slots[id].is_some())
            .collect()
    }
}

impl Trace for GraphicsExt {
    fn trace(&self, tracer: &mut Tracer<'_>) {
        for slot in self.slots.iter().flatten() {
            slot.display_list.trace(tracer);
            tracer.edge(slot.par_state);
            tracer.edge(slot.initial_state);
            tracer.edge(slot.grid_state);
        }
    }
}

impl CtxExt for GraphicsExt {}
