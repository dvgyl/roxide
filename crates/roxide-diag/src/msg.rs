//! Standard GNU R 4.6.2 diagnostic message templates (clean-room).

/// Binary operator applied to non-numeric operands.
pub const NON_NUMERIC_BINOP: &str = "non-numeric argument to binary operator";

/// Unary operator applied to a non-numeric operand.
pub const NON_NUMERIC_UNOP: &str = "invalid argument to unary operator";

/// Integer arithmetic overflow producing `NA_integer_`.
pub const INT_OVERFLOW: &str = "NAs produced by integer overflow";

/// Vector recycling length mismatch warning.
pub const RECYCLE_MISMATCH: &str =
    "longer object length is not a multiple of shorter object length";

/// Recursive promise evaluation error (empirically verified against GNU R 4.6.2).
pub const RECURSIVE_PROMISE: &str =
    "promise already under evaluation: recursive default argument reference or earlier problems?";

/// Restarting an interrupted promise warning (empirically verified against GNU R 4.6.2).
pub const RESTART_PROMISE: &str = "restarting interrupted promise evaluation";

/// Attempting to apply a non-function value.
pub const NOT_A_FUNCTION: &str = "attempt to apply non-function";

/// Embedded NUL byte in a character string.
pub const EMBEDDED_NUL: &str = "embedded nul in string";

/// `break` or `next` outside a loop.
pub const NO_LOOP_FOR_BREAK_NEXT: &str = "no loop for break/next, jumping to top level";
