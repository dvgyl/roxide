//! Write an 8-bit baseline JPEG with full-resolution Y, Cb, and Cr components.
//!
//! The transform and entropy coding follow ITU-T T.81. The APP0 marker follows JFIF 1.01.

mod tables;

use std::{
    f64::consts,
    io::{self, Write},
};

use super::fill::{Frame, rgb_on_white};
use tables::{HUFFMAN, QUANTIZATION, ZIGZAG};

pub(super) fn encode(
    file: &mut impl Write,
    frame: &Frame,
    ppi: Option<f64>,
    quality: u8,
) -> io::Result<()> {
    let width = dimension(frame.width)?;
    let height = dimension(frame.height)?;
    file.write_all(&[0xff, 0xd8])?;

    let density =
        (ppi.unwrap_or(72.0).round().clamp(1.0, f64::from(u16::MAX)) as u16).to_be_bytes();
    let [hi, lo] = density;
    segment(
        file,
        0xe0,
        &[
            b'J',
            b'F',
            b'I',
            b'F',
            0,
            1,
            1,
            u8::from(ppi.is_some()),
            hi,
            lo,
            hi,
            lo,
            0,
            0,
        ],
    )?;

    let quantization = quantization(quality);
    for (id, table) in quantization.iter().enumerate() {
        let mut payload = [id as u8; 65];
        for (dst, index) in payload[1..].iter_mut().zip(ZIGZAG) {
            *dst = table[index];
        }
        segment(file, 0xdb, &payload)?;
    }

    let [h1, h2] = height.to_be_bytes();
    let [w1, w2] = width.to_be_bytes();
    // Three components, each with one horizontal and one vertical sample per position.
    segment(
        file,
        0xc0,
        &[8, h1, h2, w1, w2, 3, 1, 0x11, 0, 2, 0x11, 1, 3, 0x11, 1],
    )?;
    for table in HUFFMAN {
        let mut payload = Vec::with_capacity(17 + table.symbols.len());
        payload.push(table.id);
        payload.extend_from_slice(&table.counts);
        payload.extend_from_slice(table.symbols);
        segment(file, 0xc4, &payload)?;
    }

    // Component selectors and DC/AC table selectors, followed by the full spectral range.
    segment(file, 0xda, &[3, 1, 0, 2, 0x11, 3, 0x11, 0, 63, 0])?;
    let dct = dct_basis();
    let [y_dc, y_ac, c_dc, c_ac] = HUFFMAN.map(|table| table.codes());
    let mut previous_dc = [0; 3];
    let mut bits = Entropy {
        file,
        buffer: 0,
        length: 0,
    };
    for y in (0..usize::from(height)).step_by(8) {
        for x in (0..usize::from(width)).step_by(8) {
            let [luma, cb, cr] = samples(frame, x, y);
            let coefficients = transform(&luma, &dct, &quantization[0]);
            bits.block(&coefficients, &mut previous_dc[0], &y_dc, &y_ac)?;
            // Each chroma component has its own DC predictor.
            for (samples, previous) in [cb, cr].iter().zip(&mut previous_dc[1..]) {
                let coefficients = transform(samples, &dct, &quantization[1]);
                bits.block(&coefficients, previous, &c_dc, &c_ac)?;
            }
        }
    }
    bits.finish()?;
    file.write_all(&[0xff, 0xd9])
}

fn dimension(value: u32) -> io::Result<u16> {
    u16::try_from(value)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid JPEG dimensions"))
}

fn segment(file: &mut impl Write, marker: u8, payload: &[u8]) -> io::Result<()> {
    let length = u16::try_from(payload.len() + 2)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "JPEG segment is too large"))?;
    file.write_all(&[0xff, marker])?;
    file.write_all(&length.to_be_bytes())?;
    file.write_all(payload)
}

fn quantization(quality: u8) -> [[u8; 64]; 2] {
    let quality = u32::from(quality.clamp(1, 100));
    let scale = if quality < 50 {
        5000 / quality
    } else {
        200 - 2 * quality
    };
    QUANTIZATION.map(|table| table.map(|q| ((u32::from(q) * scale + 50) / 100).clamp(1, 255) as u8))
}

fn samples(frame: &Frame, x: usize, y: usize) -> [[[f64; 8]; 8]; 3] {
    let width = frame.width as usize;
    let height = frame.height as usize;
    let [mut luma, mut cb, mut cr] = [[[0.0; 8]; 8]; 3];
    for (row, luma_row) in luma.iter_mut().enumerate() {
        let start = (y + row).min(height - 1) * width;
        for (col, luma_sample) in luma_row.iter_mut().enumerate() {
            // Repeat the edge sample to fill a partial 8-by-8 block.
            let [r, g, b] =
                rgb_on_white(frame.pixels[start + (x + col).min(width - 1)]).map(f64::from);
            *luma_sample = (0.299 * r + 0.587 * g + 0.114 * b).round() - 128.0;
            cb[row][col] = (-0.168736 * r - 0.331264 * g + 0.5 * b)
                .round()
                .clamp(-128.0, 127.0);
            cr[row][col] = (0.5 * r - 0.418688 * g - 0.081312 * b)
                .round()
                .clamp(-128.0, 127.0);
        }
    }
    [luma, cb, cr]
}

fn dct_basis() -> [[f64; 8]; 8] {
    std::array::from_fn(|frequency| {
        std::array::from_fn(|position| {
            if frequency == 0 {
                0.5 * consts::FRAC_1_SQRT_2
            } else {
                0.5 * (consts::PI * ((2 * position + 1) * frequency) as f64 / 16.0).cos()
            }
        })
    })
}

fn transform(samples: &[[f64; 8]; 8], basis: &[[f64; 8]; 8], quant: &[u8; 64]) -> [i16; 64] {
    let horizontal = samples.map(|row| basis.map(|b| dot(row, b)));
    ZIGZAG.map(|i| {
        let (v, u) = (i / 8, i % 8);
        let column = horizontal.map(|row| row[u]);
        (dot(column, basis[v]) / f64::from(quant[i])).round() as i16
    })
}

fn dot(a: [f64; 8], b: [f64; 8]) -> f64 {
    a.into_iter().zip(b).map(|(a, b)| a * b).sum()
}

type Codes = [(u16, u8); 256];

struct Entropy<'a, W> {
    file: &'a mut W,
    buffer: u32,
    length: u8,
}

impl<W: Write> Entropy<'_, W> {
    fn write(&mut self, bits: u16, length: u8) -> io::Result<()> {
        self.buffer = (self.buffer << length) | u32::from(bits);
        self.length += length;
        while self.length >= 8 {
            self.length -= 8;
            let byte = (self.buffer >> self.length) as u8;
            self.file.write_all(&[byte])?;
            if byte == 0xff {
                self.file.write_all(&[0])?;
            }
        }
        Ok(())
    }

    fn symbol(&mut self, table: &Codes, symbol: u8) -> io::Result<()> {
        let (bits, length) = table[usize::from(symbol)];
        if length == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid JPEG Huffman symbol",
            ));
        }
        self.write(bits, length)
    }

    fn coefficient(&mut self, table: &Codes, zeros: u8, n: i16) -> io::Result<()> {
        let size = (u16::BITS - n.unsigned_abs().leading_zeros()) as u8;
        self.symbol(table, (zeros << 4) | size)?;
        // Negative coefficients use the ones' complement of the magnitude bits.
        let mask = (1_u16 << size) - 1;
        let magnitude = n.unsigned_abs();
        self.write(if n < 0 { mask ^ magnitude } else { magnitude }, size)
    }

    fn block(
        &mut self,
        coefficients: &[i16; 64],
        previous_dc: &mut i16,
        dc: &Codes,
        ac: &Codes,
    ) -> io::Result<()> {
        self.coefficient(dc, 0, coefficients[0] - *previous_dc)?;
        *previous_dc = coefficients[0];
        let mut zeros = 0;
        for &n in &coefficients[1..] {
            if n == 0 {
                zeros += 1;
                continue;
            }
            while zeros >= 16 {
                self.symbol(ac, 0xf0)?;
                zeros -= 16;
            }
            self.coefficient(ac, zeros, n)?;
            zeros = 0;
        }
        if zeros > 0 {
            self.symbol(ac, 0)?;
        }
        Ok(())
    }

    fn finish(mut self) -> io::Result<()> {
        let padding = (8 - self.length) % 8;
        self.write((1 << padding) - 1, padding)
    }
}
