//! R predicates for missing and non-finite numbers.

use std::ffi::c_int;

use roxide_value::{is_na_real, is_nan_not_na};

macro_rules! predicate {
    ($name:ident, $test:expr, $doc:literal) => {
        #[doc = $doc]
        #[allow(non_snake_case)]
        #[unsafe(no_mangle)]
        pub extern "C" fn $name(x: f64) -> c_int {
            c_int::from($test(x))
        }
    };
}

predicate!(R_IsNA, is_na_real, "Test for the R NA payload.");
predicate!(R_IsNaN, is_nan_not_na, "Test for a NaN other than R NA.");
predicate!(R_finite, f64::is_finite, "Test whether a number is finite.");
predicate!(R_isnancpp, f64::is_nan, "Test for any NaN, including R NA.");
