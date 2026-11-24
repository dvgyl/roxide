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

## 4. Wave 2 Black-Box Native Dispatch Probes

| Condition | Exact GNU R 4.6.2 Behavior |
| --- | --- |
| `.Call` returning C `NULL` | Stderr: `Warning message:\nIn .Call("...") : converting NULL pointer to R NULL` and returns `NULL` |
| Stack imbalance labels | `Warning: stack imbalance in '.Call'\|'.External'\|'.External2'\|'.C'\|'.Fortran', <before> then <after>` |
| `.External2` `args` pairlist | Includes `.NAME` as its first element (`CAR(args) == .NAME`), followed by tagged arguments |
| `.C(..., NAOK = FALSE)` on integer or logical NA | Error: `NAs in foreign function call (arg N)` |
| `.C(..., NAOK = FALSE)` on double NA, NaN, or Inf | Error: `NA/NaN/Inf in foreign function call (arg N)` |
| `.C(..., NAOK = FALSE)` on complex NA, NaN, or Inf | Error: `complex NA/NaN/Inf in foreign function call (arg N)` |
| `.C(..., NAOK = FALSE)` on `NA_character_` | Allowed (no error) |
| Missing `R_GetCCallable(pkg, sym)` | Error: `function '<sym>' not provided by package '<pkg>'` |
| `.C` buffer over-run / under-run | GNU R has no canaries (silently corrupts heap); Roxide enforces 64-byte guard canaries and raises `array over-run in .C("<name>") argument N` (ADR-0018) |
