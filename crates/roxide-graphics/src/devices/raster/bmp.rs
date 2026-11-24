//! Write a Windows v3 bitmap with 24-bit BGR pixels.

use std::io::{self, Write};

use super::fill::{Frame, rgb_on_white};

const PIXEL_OFFSET: u32 = 14 + 40;

fn stride(width: u32) -> u64 {
    (u64::from(width) * 3).next_multiple_of(4)
}

pub(super) fn image_size(width: u32, height: u32) -> io::Result<u32> {
    u32::try_from(stride(width) * u64::from(height) + u64::from(PIXEL_OFFSET))
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "BMP file is too large"))
}

pub(super) fn encode(file: &mut impl Write, frame: &Frame, ppi: f64) -> io::Result<()> {
    let size = image_size(frame.width, frame.height)?;
    let ppm = (ppi / 0.0254).round().min(f64::from(i32::MAX)) as u32;
    file.write_all(b"BM")?;
    for field in [size, 0, PIXEL_OFFSET, 40, frame.width, frame.height] {
        file.write_all(&field.to_le_bytes())?;
    }
    file.write_all(&[1, 0, 24, 0])?;
    for field in [0, size - PIXEL_OFFSET, ppm, ppm, 0, 0] {
        file.write_all(&field.to_le_bytes())?;
    }

    let padding = (stride(frame.width) - u64::from(frame.width) * 3) as usize;
    for row in frame.pixels.chunks_exact(frame.width as usize).rev() {
        for &pixel in row {
            let [r, g, b] = rgb_on_white(pixel);
            file.write_all(&[b, g, r])?;
        }
        file.write_all(&[0; 3][..padding])?;
    }
    Ok(())
}
