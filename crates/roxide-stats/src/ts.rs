//! FFT, convolution, and core time-series helpers (`fft`, `mvfft`, `nextn`, `convolve`, `ts`, `filter`, `acf`, `diff`) (W7B).

use roxide_eval::Module;

/// Submodule primitive and prelude registration table.
pub const MODULE: Module = Module {
    prims: &[],
    prelude: "",
};
