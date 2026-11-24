//! Write a little-endian Baseline TIFF 6.0 RGB image with one uncompressed strip.

use std::io::{self, Write};

use super::fill::{Frame, rgb_on_white};

const IFD_OFFSET: u32 = 8;
const TAG_COUNT: u16 = 12;
const BITS_OFFSET: u32 = IFD_OFFSET + 2 + 12 * TAG_COUNT as u32 + 4;
const RESOLUTION_OFFSET: u32 = BITS_OFFSET + 6;
const PIXEL_OFFSET: u32 = RESOLUTION_OFFSET + 8;

pub(super) fn image_size(width: u32, height: u32) -> io::Result<u32> {
    u32::try_from(u64::from(width) * u64::from(height) * 3 + u64::from(PIXEL_OFFSET))
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "TIFF file is too large"))
}

pub(super) fn encode(file: &mut impl Write, frame: &Frame, ppi: Option<f64>) -> io::Result<()> {
    let strip_bytes = image_size(frame.width, frame.height)? - PIXEL_OFFSET;
    const SHORT: u16 = 3;
    const LONG: u16 = 4;
    const RATIONAL: u16 = 5;
    let tags = [
        (256_u16, LONG, 1_u32, frame.width),
        (257, LONG, 1, frame.height),
        (258, SHORT, 3, BITS_OFFSET),
        (259, SHORT, 1, 1), // Compression = none.
        (262, SHORT, 1, 2), // PhotometricInterpretation = RGB.
        (273, LONG, 1, PIXEL_OFFSET),
        (277, SHORT, 1, 3), // SamplesPerPixel.
        (278, LONG, 1, frame.height),
        (279, LONG, 1, strip_bytes),
        (282, RATIONAL, 1, RESOLUTION_OFFSET),
        (283, RATIONAL, 1, RESOLUTION_OFFSET),
        (296, SHORT, 1, if ppi.is_some() { 2 } else { 1 }),
    ];
    file.write_all(b"II\x2a\x00")?;
    file.write_all(&IFD_OFFSET.to_le_bytes())?;
    file.write_all(&TAG_COUNT.to_le_bytes())?;
    for (tag, field_type, count, value) in tags {
        file.write_all(&tag.to_le_bytes())?;
        file.write_all(&field_type.to_le_bytes())?;
        file.write_all(&count.to_le_bytes())?;
        file.write_all(&value.to_le_bytes())?;
    }
    file.write_all(&0_u32.to_le_bytes())?;
    file.write_all(&[8, 0, 8, 0, 8, 0])?;
    let (numerator, denominator) = ppi.map_or((1, 1_u32), |ppi| {
        let denominator = (f64::from(u32::MAX) / ppi).clamp(1.0, 1000.0) as u32;
        let numerator = (ppi * f64::from(denominator)).round().max(1.0) as u32;
        (numerator, denominator)
    });
    file.write_all(&numerator.to_le_bytes())?;
    file.write_all(&denominator.to_le_bytes())?;
    for &pixel in &frame.pixels {
        file.write_all(&rgb_on_white(pixel))?;
    }
    Ok(())
}
