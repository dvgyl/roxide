#![forbid(unsafe_code)]

//! Clean-room GNU R 4.6.2 `grDevices`, `graphics`, and `grid` engine with headless PDF/SVG/PNG devices (ADR-0015).

pub mod devices;
pub mod engine;
pub mod graphics;
pub mod grdevices;
pub mod grid;

use roxide_eval::{Ctx, InternalError, Package};

/// Package descriptor for `grDevices` (attached on `search()` as `package:grDevices`).
pub const GRDEVICES_PACKAGE: Package = Package {
    name: "grDevices",
    attach: true,
    exports: None,
    modules: &[&grdevices::MODULE, &devices::MODULE],
};

/// Package descriptor for `graphics` (attached on `search()` as `package:graphics`).
pub const GRAPHICS_PACKAGE: Package = Package {
    name: "graphics",
    attach: true,
    exports: None,
    modules: &[&graphics::MODULE],
};

/// Package descriptor for `grid` (loaded namespace `namespace:grid`, unattached by default).
pub const GRID_PACKAGE: Package = Package {
    name: "grid",
    attach: false,
    exports: None,
    modules: &[&grid::MODULE],
};

/// Register `grDevices` (attached), `graphics` (attached above `grDevices`), and `grid` (namespace-only).
pub fn register(ctx: &mut Ctx) -> Result<(), InternalError> {
    ctx.register_package(&GRDEVICES_PACKAGE)?;
    ctx.register_package(&GRAPHICS_PACKAGE)?;
    ctx.register_package(&GRID_PACKAGE)?;
    Ok(())
}
