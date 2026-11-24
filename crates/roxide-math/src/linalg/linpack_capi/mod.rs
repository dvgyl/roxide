//! LINPACK kernels for column-major matrices with explicit leading dimensions.
//!
//! An entry at row `i` and column `j` has index `i + j * ld`.
//! Matrix slices may omit padding after the last column. Invalid dimensions
//! or short buffers cause no writes. Routines with an `info` result use
//! `-1` for invalid arguments and positive values for numerical failures.

#![deny(missing_docs)]

mod cholesky;
mod qr;
mod svd;
mod tri;

pub use cholesky::{dpbfa, dpbsl, dpoco, dpodi, dpofa, dposl};
pub use qr::{dqrdc, dqrsl};
pub use svd::dsvdc;
pub use tri::{dtrco, dtrsl};

fn fits(len: usize, ld: usize, rows: usize, cols: usize) -> bool {
    ld >= rows
        && (rows == 0
            || cols == 0
            || (cols - 1)
                .checked_mul(ld)
                .and_then(|offset| offset.checked_add(rows))
                .is_some_and(|end| end <= len))
}

fn index_info(index: usize) -> i32 {
    i32::try_from(index + 1).unwrap_or(i32::MAX)
}
