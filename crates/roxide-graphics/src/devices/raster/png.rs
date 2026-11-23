//! Encode an 8-bit RGBA PNG from a premultiplied framebuffer.

use std::io::{self, Write as _};

use flate2::{Compression, Crc, write::ZlibEncoder};

use super::fill::Frame;

pub(super) fn encode(frame: &Frame, ppi: f64) -> io::Result<Vec<u8>> {
    let mut file = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = [0; 13];
    header[..4].copy_from_slice(&frame.width.to_be_bytes());
    header[4..8].copy_from_slice(&frame.height.to_be_bytes());
    header[8] = 8;
    header[9] = 6;
    chunk(&mut file, b"IHDR", &header)?;

    let resolution = ((ppi / 0.0254).round() as u32).to_be_bytes();
    let mut phys = [1; 9];
    phys[..4].copy_from_slice(&resolution);
    phys[4..8].copy_from_slice(&resolution);
    chunk(&mut file, b"pHYs", &phys)?;

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    let mut row = Vec::with_capacity(1 + frame.width as usize * 4);
    for pixels in frame.pixels.chunks_exact(frame.width as usize) {
        row.clear();
        row.push(0);
        for &[r, g, b, a] in pixels {
            row.extend([
                unpremultiply(r, a),
                unpremultiply(g, a),
                unpremultiply(b, a),
                a,
            ]);
        }
        encoder.write_all(&row)?;
    }
    for bytes in encoder.finish()?.chunks(64 * 1024) {
        chunk(&mut file, b"IDAT", bytes)?;
    }
    chunk(&mut file, b"IEND", &[])?;
    Ok(file)
}

fn unpremultiply(component: u8, alpha: u8) -> u8 {
    if alpha == 0 {
        return 0;
    }
    let alpha = u16::from(alpha);
    ((u16::from(component) * 255 + alpha / 2) / alpha) as u8
}

fn chunk(file: &mut Vec<u8>, tag: &[u8; 4], bytes: &[u8]) -> io::Result<()> {
    let length = u32::try_from(bytes.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "PNG chunk is too large"))?;
    let mut crc = Crc::new();
    crc.update(tag);
    crc.update(bytes);
    file.extend_from_slice(&length.to_be_bytes());
    file.extend_from_slice(tag);
    file.extend_from_slice(bytes);
    file.extend_from_slice(&crc.sum().to_be_bytes());
    Ok(())
}
