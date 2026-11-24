# Milestone 4 Wave 0 Black-Box GNU R 4.6.2 Probes

All observations below were gathered strictly by executing compiled black-box C snippets (zero GNU R headers or sources read) and R expressions against the official GNU R 4.6.2 binary oracle (`$ROXIDE_ORACLE_RSCRIPT --vanilla`).

## 1. C API Error & Warning Texts

| Condition | Exact GNU R 4.6.2 Behavior |
| --- | --- |
| `Rf_protect` overflow (`> 50000` entries) | Error: `protect(): protection stack overflow` |
| `Rf_unprotect_ptr` on absent `SEXP` | Error: `unprotect_ptr: pointer not found` |
| `INTEGER(x)` on a `REALSXP` | Error: `INTEGER() can only be applied to a 'integer', not a 'double'` |
| `.Call` returning C `NULL` (`(SEXP)0`) | Warning: `converting NULL pointer to R NULL` (returns `NULL`) |
| Protect-stack imbalance on `.Call` exit | Stderr: `Warning: stack imbalance in '.Call', <before> then <after>` |
| `.C(..., NAOK = FALSE)` receiving `NA` | Error: `NAs in foreign function call (arg 1)` |
| `Rf_error` message length truncation | Truncated to `getOption("warning.length")` (default 1000 bytes, 999 chars visible) |

## 2. Bytecode JIT Compilation Trigger (`compiler::enableJIT`)

- Default `compiler::enableJIT(-1)` = **`3`**.
- Plain function without loops (`f <- function(x) x + 1`):
  - After call 1: not compiled (`FALSE`).
  - After call 2: compiled (`TRUE`, prints `<bytecode: 0x...>`).
- Function containing a loop (`for` / `while` / `repeat`):
  - After call 1: compiled immediately (`TRUE`).

## 3. Permanent `R_*Symbol` Print Names (`CHAR(PRINTNAME(sym))`)

Verified all 41 exported `R_*Symbol` / `R_dot_*` data symbols (`R_BaseSymbol == R_baseSymbol == "base"`, `R_SpecSymbol == "spec"`, `R_PreviousSymbol == "previous"`, `R_NamespaceEnvSymbol == ".__NAMESPACE__."`, `R_LastvalueSymbol == ".Last.value"`, `R_SeedsSymbol == ".Random.seed"`, `R_DeviceSymbol == ".Device"`, `R_dot_Generic == ".Generic"`, `R_dot_Method == ".Method"`, `R_dot_defined == ".defined"`, `R_dot_packageName == ".packageName"`, `R_dot_target == ".target"`, etc.) and allocated all 40 distinct symbols in `roxide-value::session` permanent slots `17..=56`.
