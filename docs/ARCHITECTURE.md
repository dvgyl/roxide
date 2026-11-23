# Architecture

Roxide models GNU R 4.6.2's runtime cleanly in safe Rust, decomposed into **disjoint crates** so parallel **GPT 6.2 Sol** workers can build language primitives, statistical math, standard packages, serialization, and graphics concurrently without merge conflicts — while isolating any `unsafe` C-API / FFI boundary into a single audited crate.

## Execution Pipeline

1. **CLI & Session (`roxide-cli`, `roxide-host`):** Dispatches `R`, `Rscript`, and `R CMD` via `argv[0]` or subcommands, parses flags (`--vanilla`, `--slave`, `--no-save`, `-e`, `-f`), configures options/locale/search path, and initializes the `Host` and GC arena.
2. **Parse (`roxide-syntax`):** Tokenizes and parses R 4.6.2 source (including `|>`, `_`, `\(x)`, `r"(...)"`, `%op%`, `srcref` metadata) into R language expressions (`LANGSXP` / `SYMSXP` / literals) and supports exact `deparse()` formatting.
3. **Evaluate (`roxide-gc`, `roxide-value`, `roxide-eval`):** Runs lexical-scoped evaluation with lazy promises (`PROMSXP`), copy-on-modify vectors, `ALTREP`, condition handling (`tryCatch`, `withCallingHandlers`, `on.exit`), S3/S4 dispatch, and namespaces.
4. **Standard Libraries & Numerical Engine (`roxide-base`, `roxide-math`, `roxide-stats`, `roxide-methods`, `roxide-utils`, `roxide-graphics`):** Implements clean-room primitives and R-level wrappers for R 4.6.2's core packages.
5. **C API / CRAN Interop (`roxide-capi`, Milestone 4):** Exposes R 4.6.2's cleaned-up public C ABI (`Rinternals.h`, `R_ext/*.h`, `R_NO_REMAP`) for dynamically loading compiled packages (`.Call`, `.External`, `.C`, `.Fortran`, `ALTREP`).

## Workspace Crate Map

| Crate | Responsibility | `unsafe` Policy | Depends on |
| --- | --- | --- | --- |
| `roxide-diag` | Source spans, `srcref`, R-exact error/warning/message/traceback formatting, condition hierarchy | `forbid` | none |
| `roxide-host` | `Host` trait (FS, env, stdin/stdout/stderr, connections, process/`system2`, clock, locale/TZ, entropy), `NativeHost`, deterministic `PureHost`, resource limits | `forbid` | `roxide-diag` |
| `roxide-syntax` | Zero-copy R 4.6.2 lexer & parser (newline rules inside `()` vs `{}`, `|>` + `_`, `\(x)`, raw strings `r"(...)"`, hex/complex literals, `%op%`), AST & `deparse()` | `forbid` | `roxide-diag` |
| `roxide-gc` | Cycle-safe mark-sweep/generational GC arena, `GcRef` / `SEXP` handles, copy-on-modify refcounts (`NAMED`/shared tracking), `ProtectStack`, `ALTREP` trait & wrapper duplication safety (4.6.2 fix) | `forbid` (or ADR-gated) | `roxide-diag` |
| `roxide-value` | Core R data model: atomic vectors (`LGL`, `INT`, `REAL`, `CPLX`, `STR`, `RAW`), `VECSXP`, `LISTSXP`, `LANGSXP`, `ENVSXP`, `PROMSXP`, `CLOSXP`, `S4SXP`, attributes, exact `NA_real_` (`1954`) / `NA_integer_` / Kleene logic, coercion, recycling, `print`/`format` | `forbid` | `roxide-diag`, `roxide-gc` |
| `roxide-eval` | Evaluator & bytecode VM, lexical environments, lazy promises & NSE (`substitute`, `match.call`, `missing`, `eval`, `...`), call argument matching, conditions (`tryCatch`, `withCallingHandlers`, `on.exit`), S3 dispatch (`UseMethod`, `NextMethod`, group generics), namespaces & `use()` | `forbid` | `roxide-syntax`, `roxide-value`, `roxide-host` |
| `roxide-math` | Clean-room numerical core: special functions (`gamma`, `lgamma`, `beta`, `bessel*`), PRNGs (`MT19937`, `L'Ecuyer-CMRG`, `Wichmann-Hill`, etc.), `d/p/q/r*` distributions (including 4.6.2 `rhyper` large-$m+n$ fix), `fft`, clean-room linear algebra (`qr`, `svd`, `eigen`, `chol`, `solve`, `det`) | `forbid` | `roxide-diag`, `roxide-value` |
| `roxide-base` | `base` package primitives & R wrappers split into disjoint submodules (`ops`, `subset`, `apply`, `string_regex`, `datetime`, `io_conn`, `files_os`, `serialize_rds`, `env_meta`, `matrix`, `cumvar_seq`, `grepv`) | `forbid` | `roxide-eval`, `roxide-math`, `roxide-host` |
| `roxide-stats` | `stats` package split into disjoint submodules (`formula_model_matrix`, `lm_glm`, `tests`, `descriptive`, `optim_nlminb`, `timeseries`, `cluster`, `dist_wrappers`, `dummy_coef`) | `forbid` | `roxide-eval`, `roxide-base`, `roxide-math` |
| `roxide-methods` | S4 class/generic/method system (`setClass`, `setGeneric`, `setMethod`, multiple dispatch, `@` slots, Reference Classes) | `forbid` | `roxide-eval`, `roxide-base` |
| `roxide-utils` | `utils` & `tools` packages: `read.table`/`read.csv`, `write.table`, `str`, `head`/`tail`, `sessionInfo`, `extSoftVersion` (`zstd`), UTF-8 `read.dcf`/`write.dcf` with `#` comments, `Rd` parser/`Rd2txt` (4.6.2 fix), `analyze_license` (SPDX), `R CMD` build/install helpers | `forbid` | `roxide-eval`, `roxide-base`, `roxide-host` |
| `roxide-graphics` | `grDevices`, `graphics`, and `grid` engine + clean-room headless `svg`, `pdf`, and `png` devices (`boxplot(..., warnN = FALSE)`, base pipe plotting) | `forbid` | `roxide-eval`, `roxide-base`, `roxide-stats`, `roxide-host` |
| `roxide-capi` | **Isolated FFI crate (Milestone 4):** R 4.6.2 public C API (`Rinternals.h`, `R_ext/*.h`, `R_NO_REMAP`), `dlopen` package loading, `.Call`/`.External`/`.C`/`.Fortran`, `R_registerRoutines`, `ALTREP` C callbacks, experimental resizable vectors API | audited `unsafe` (ADR-governed) | `roxide-eval`, `roxide-gc`, `roxide-value`, `roxide-math` |
| `roxide-cli` | Multi-call binary (`roxide`, `R`, `Rscript` via `argv[0]`), interactive REPL, `R CMD` dispatcher | `forbid` | all engine crates |
| `roxide-harness` | Isolated black-box differential oracle runner comparing `roxide` against official GNU R 4.6.2 (never links core crates) | `forbid` | none of the core crates |

## Coordinator-Owned Shared Contracts

Only the **coordinator (main thread)** edits:
- Root `Cargo.toml`, `Cargo.lock` dependency additions, `scripts/`, `PROGRESS.md`, `docs/ADR.md`
- Public traits and core representations in `roxide-diag`, `roxide-host`, `roxide-gc`, `roxide-value`, and `roxide-syntax`
- Top-level primitive/builtin registration wiring in `roxide-base/src/lib.rs` and `roxide-stats/src/lib.rs`
- `roxide-harness` test format and conformance baselines

Workers implement disjoint crates or disjoint submodules (`roxide-base/src/<family>/`, `roxide-stats/src/<family>/`, `roxide-math/src/<family>/`) and request shared contract changes from the coordinator.

## Critical GNU R 4.6.2 Invariants & Known Traps

Lock these invariants with **Gemini 4 Neon + Opus 5.5 + GPT 6.2 Astra** before writing Milestone 1 code:

1. **Cycle-Safe GC + Copy-on-Modify + `ALTREP`:**
   - R closures capture environments that contain those very closures; promises reference environments that reference promises. Simple `Rc`/`Arc` leaks memory immediately. Build a real cycle-safe GC arena from Day 1 with an explicit `ProtectStack` so `roxide-capi` slots in cleanly later without a rewrite.
   - Reference-count sharing (`NAMED` / refcount) enables in-place mutation when unshared while preserving R's value semantics when shared. Implement R 4.6.2's wrapper duplication guard before exposing read-only pointers.
2. **Exact Missing Values (`NA` vs `NaN`):**
   - `NA_integer_ == i32::MIN`; integer arithmetic overflow returns `NA_integer_` with the exact R warning (`NAs produced by integer overflow`).
   - `NA_real_` is an IEEE 754 quiet NaN carrying the `1954` payload in the low 32 bits (`is.na(NaN) == TRUE`, `is.nan(NA_real_) == FALSE`). Preserve `NA_real_` vs `NaN` distinction carefully in arithmetic, comparisons, sorting, hashing, serialization, and printing.
   - Three-valued Kleene logic (`TRUE | NA == TRUE`, `FALSE & NA == FALSE`, `NA | FALSE == NA`) and `NA_character_` distinct from `"NA"`.
3. **Lazy Promises, NSE & Call Argument Matching:**
   - Arguments to R closures are wrapped in promises (`expr`, `env`, `value`) evaluated lazily on first lookup and memoized thereafter.
   - `substitute()`, `missing()`, `force()`, `match.call()`, `sys.calls()`, `parent.frame()`, `eval()`, and `...` (`DOTSXP`) forwarding must follow exact R rules — otherwise `lm()`, `subset()`, `with()`, `data.frame()`, and modern R packages fail.
   - Call matching order: exact tag match → unique partial tag match (before `...`) → positional match → remaining args captured into `...`.
4. **Vector Subsetting, Subassignment & Recycling:**
   - `[`, `[[`, `$`, `@` with positive/negative/logical/character/matrix indices, `NA` indices, zero-length indices, out-of-bounds `NA` fill, `drop = TRUE` matrix/array reduction, partial `$` matching, and nested replacement calls (`names(x)[2] <- "b"` desugaring via `*tmp*`).
5. **Regex Dialects (TRE vs PCRE2 vs Fixed):**
   - R supports three regex engines via `fixed = TRUE`, default POSIX ERE (TRE semantics), and `perl = TRUE` (PCRE2 semantics — BSD-3-Clause!). Never conflate them; handle bytes vs UTF-8 encodings and `writeChar()` multibyte counting (R 4.6.2 fix) accurately.
6. **XDR v2/v3 `.rds` & `.RData` Serialization:**
   - Exact big-endian XDR binary encoding of `SEXP`s, `CHARSXP` encoding flags, reference tables for shared environments/symbols, `ALTREP` state, and transparent `gzip`/`bzip2`/`xz`/`zstd` streams so files written by GNU R 4.6.2 load cleanly in Roxide and vice versa.
7. **Reproducible PRNG & Numerical Accuracy (`nmath` clean-room):**
   - Exact `set.seed()` / `.Random.seed` state representation for `Mersenne-Twister` + `Inversion` + `Rejection` (default in R 4.x) so `set.seed(42); rnorm(10)` and `sample()` match GNU R 4.6.2!
   - Clean-room implementations of `d/p/q/r*` distributions, `lm`/`glm`, `optim`/`nlminb` verified against GNU R 4.6.2 with explicit ULP / relative-error tolerances (consult **GPT 6.2 Astra** for tail stability and edge cases like `rhyper` overflow when $m + n > \text{INT\_MAX}$).
8. **R 4.6.0–4.6.2 Public C API Clean Boundary:**
   - R 4.6.0 separated private internals out of installed C headers and R 4.5+ enforces `R_NO_REMAP` for C++ — making the public `libR` ABI smaller, cleaner, and far more tractable than legacy R 3.x.

## Layered Differential Oracle Strategy

All verification runs black-box against the official **GNU R 4.6.2** binary (`Rscript --vanilla` / `R --vanilla`):

| Layer | What it tests | Pass criterion |
| --- | --- | --- |
| **Layer 1: Exact Language & Output Parity** | Syntax/parse/deparse, evaluation, promises/NSE, subsetting, S3/S4, `print`/`format`/`str`, conditions/warnings, `base` & `utils` text output | **Byte-for-byte** stdout, stderr, exit code, and `identical()`-equivalent `dput()` output vs. GNU R 4.6.2 |
| **Layer 2: Numerical & PRNG Parity** | `set.seed()` sequences, `d/p/q/r*` distributions (body + extreme tails + `NA`/`NaN`/`Inf`), linear algebra (`qr`/`svd`/`eigen`/`chol`), `lm`/`glm`/`optim`/`nlminb` | Bit-exact PRNG integer/uniform streams; tight ULP / relative-tolerance bounds (e.g., $\le 10^{-13}$ to $10^{-15}$) verified against GNU R 4.6.2 |
| **Layer 3: Binary `.rds`/`.RData` & File Interchange** | Cross-roundtrip: R 4.6.2 writes `.rds`/`.RData` → Roxide reads → Roxide writes → R 4.6.2 verifies `identical()` | 100% cross-engine `identical()` roundtrip pass |
| **Layer 4: Real-World Pure-R & Compiled CRAN Corpus** | Real-world statistical workflows, pure-R CRAN packages (M2–M3), and compiled C/C++ CRAN packages via `roxide-capi` (M4) | Matching outputs, test-suite pass rates, and zero crashes |
