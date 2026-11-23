//! Headless PDF 1.4, SVG 1.1, and PNG graphics devices + font metrics (owned by M3 Builder D).
//!
//! The R-level `pdf()`, `svg()`, and `png()` functions live in this module (`devices::MODULE`).

use roxide_eval::Module;
use roxide_host::Host;

use crate::engine::{Device, DeviceSpec, RecordingDevice};

/// Device constructor primitives (`pdf`, `svg`, `png`; owned by M3 Builder D).
pub const MODULE: Module = Module {
    prims: &[],
    prelude: "",
};

/// Construct a concrete graphics [`Device`] from `spec` (Builder D replaces the stub branches in W1-D).
pub fn open_device(_host: &mut dyn Host, spec: &DeviceSpec) -> Result<Box<dyn Device>, String> {
    match spec {
        DeviceSpec::Recording {
            name,
            width,
            height,
        } => Ok(Box::new(RecordingDevice::new(
            name.clone(),
            *width,
            *height,
        ))),
        DeviceSpec::Pdf { width, height, .. } => {
            Ok(Box::new(RecordingDevice::new("pdf", *width, *height)))
        }
        DeviceSpec::Svg { width, height, .. } => {
            Ok(Box::new(RecordingDevice::new("svg", *width, *height)))
        }
        DeviceSpec::Png {
            width_px,
            height_px,
            res,
            device_name,
            ..
        } => {
            let ppi = if *res > 0.0 { *res } else { 72.0 };
            Ok(Box::new(RecordingDevice::new(
                device_name.clone(),
                f64::from(*width_px) / ppi,
                f64::from(*height_px) / ppi,
            )))
        }
    }
}
