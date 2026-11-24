# Architecture

Roxide is a clean-room GNU R 4.6.2 runtime in Rust, decomposed into disjoint crates so parallel workers can build language primitives, math, standard libraries, C API compatibility, multi-session isolation, graphics, embedding SDKs, and developer tools concurrently without merge conflicts.

Every architectural decision works backwards from [`docs/VISION.md`](VISION.md) and the Top 3 priorities: CRAN-wide/Bioconductor pass rate, bundled toolchain plus binary package repository, and multi-session execution with compiled packages.

## Execution pipeline

1. **CLI, Embedding SDKs and Interactive Backends (`roxide-cli`, `roxide-r`, `roxide-py`, `roxide-wasm`, `roxide-kernel`, `roxide-lsp`, `roxide-dap`):** Dispatches `roxide`, `R`, `Rscript`, `R CMD`, Jupyter/Positron kernels, LSP, DAP, and host-language embedding APIs, configuring per-session `Host` permissions, quotas, snapshots, and GC arenas.
2. **Parse and Diagnostics (`roxide-diag`, `roxide-syntax`):** Tokenizes and parses R 4.6.2 source (including `|>`, `_`, `\(x)`, `r"(...)"`, `%op%`, exact `srcref` line/column spans) into R expressions and formats both GNU R compatible stderr and rich interactive diagnostics with source snippets, call chains, and suggested fixes.
3. **Evaluate and Optimize (`roxide-gc`, `roxide-value`, `roxide-eval`):** Runs lexical-scoped evaluation with 4-state lazy promises (`PROMSXP`), copy-on-modify vectors, ALTREP (including zero-copy Arrow columns), condition handling, S3/S4 dispatch, namespaces, clean-room register IR, multithreaded vector ops, and profile-guided native JIT for hot paths.
4. **Standard Libraries and Numerical Engine (`roxide-base`, `roxide-math`, `roxide-blas`, `roxide-stats`, `roxide-methods`, `roxide-utils`, `roxide-graphics`, `roxide-pkg`):** Clean-room R 4.6.2 base/stats/utils/methods/tools/graphics/grid packages, bundled swappable pure-Rust BLAS/LAPACK (`faer`) with Fortran ABI exports, binary/source package installation, bundled C/C++/Fortran toolchain (`zig cc`), `renv.lock`, and compatibility advisories.
5. **Native Package Interop and Multi-Session Isolation (`roxide-capi`, `roxide-libr`):** Clean-room MIT headers from *Writing R Extensions*, R 4.6.2 public C API plus common private-API compatibility shims, sound `Rf_error`/`longjmp` and C++ exception unwinding, and three-tier multi-session isolation for compiled CRAN packages.

## Workspace crate map

### Existing crates (M0 through M4, 17 crates active)

| Crate | Responsibility | `unsafe` policy | Key dependencies |
| --- | --- | --- | --- |
| `roxide-diag` | Source spans, `srcref`, GNU R and rich interactive error/warning/traceback formatting | `forbid` | none |
| `roxide-math` | Pure-Rust leaf numerical kernels: PRNGs (bit-exact MT19937 + `runif`/`rnorm`/`sample`), 22 distributions (`< 1e-14` rel error), special/Bessel functions, core linalg, FFT | `forbid` | none |
| `roxide-host` | `Host` trait (FS, env, streams, connections, process, clock, locale/TZ, entropy), Deno-style permission/quota policies, `NativeHost`, `PureHost` | `forbid` | `roxide-diag` |
| `roxide-gc` | Non-moving cycle-safe mark-sweep GC arena, `SexpId` handles, saturating copy-on-modify refcounts, `ProtectStack`, ephemerons and finalizers | `forbid` | `roxide-diag` |
| `roxide-syntax` | Zero-copy R 4.6.2 lexer, parser, `srcref` metadata, and operator tables | `forbid` | `roxide-diag` |
| `roxide-value` | `RObject` model, atomic/list/env/promise/closure/S4 types, exact `NA_real_` (`1954`) / `NA_integer_` / Kleene logic, `ALTREP` traits, `deparse`/`format`, snapshots | `forbid` | `roxide-diag`, `roxide-gc`, `roxide-syntax` |
| `roxide-eval` | Evaluator, clean-room register IR VM, targeted JIT hooks, 4-state promises and NSE, conditions, S3 dispatch, dynamic namespaces, `NativeHooks`, `S4Hooks` | `forbid` | `roxide-syntax`, `roxide-value`, `roxide-host` |
| `roxide-base` | `base` and `compiler` primitives and preludes in disjoint submodules (`ops`, `subset`, `strings`, `datetime`, `io_os`, `serialize`, `native`, `compiler`, etc.) | `forbid` | `roxide-eval`, `roxide-math`, `roxide-host` |
| `roxide-stats` | `stats` package (`formula`, `lm`, `glm`, `htest`, `optim`/`nlminb`, `ts`, `multivar`, `density`, `loess`, `distn`) | `forbid` | `roxide-eval`, `roxide-base`, `roxide-math` |
| `roxide-methods` | S4 classes, generics, Pareto-dominance multiple dispatch, `@` slots, Reference Classes | `forbid` | `roxide-eval`, `roxide-base` |
| `roxide-pkg` | `NAMESPACE`, `DESCRIPTION`, `.rdb`/`.rdx` lazy-load DB, `library`/`use`, `R CMD INSTALL`/`SHLIB`/`BATCH`, binary repo installer, `renv.lock`, bundled toolchain driver, install advisories | `forbid` | `roxide-eval`, `roxide-base`, `roxide-host` |
| `roxide-utils` | `utils`, `tools`, and `datasets` packages (`read.table`, `str`, `head`, `tail`, `sessionInfo`, DCF, `Rd2txt`, SPDX licence analyzer) | `forbid` | `roxide-eval`, `roxide-base`, `roxide-host` |
| `roxide-graphics` | `grDevices`, `graphics`, `grid`, PDF/SVG/PNG/BMP/JPEG/TIFF devices, interactive window device, `ragg`-parity text shaping (fallback/CJK/emoji), `htmlwidgets`/`plotly` hooks | `forbid` | `roxide-eval`, `roxide-base`, `roxide-host` |
| `roxide-capi` | Clean-room MIT C headers, R 4.6.2 public C API + common private-API compat shims, C/C++ setjmp/`catch(...)` unwind wall, `dlopen`/registration, foreign ALTREP, multi-session native router | audited `unsafe` | `roxide-eval`, `roxide-value`, `roxide-gc`, `roxide-math`, `roxide-base`, `roxide-pkg` |
| `roxide-libr` | `staticlib` re-exporting `roxide-capi`, `roxide-blas`, and `roxide_main` for manifest-driven `libR` linking | audited `unsafe` | `roxide-capi`, `roxide-cli` |
| `roxide-cli` | Multi-call binary (`roxide`, `R`, `Rscript`, `R CMD`), REPL, session bootstrapper | `forbid` | engine crates |
| `roxide-harness` | Standalone black-box differential oracle runner comparing Roxide against official GNU R 4.6.2 | `forbid` | none of the engine crates |

### Planned crates (M5 through M8)

| Crate | Target milestone | Responsibility | `unsafe` policy |
| --- | --- | --- | --- |
| `roxide-blas` | M5 | Bundled, swappable pure-Rust `faer` BLAS/LAPACK backend exporting Fortran-ABI symbols (`libRblas`/`libRlapack`) plus runtime swap to Accelerate/OpenBLAS/MKL | audited `unsafe` (C/Fortran ABI shims only) |
| `roxide-arrow` | M7 | Zero-copy Arrow data frames through ALTREP and Arrow C Data Interface for DuckDB, Polars, and Python | audited `unsafe` (C Data Interface boundary) |
| `roxide-r` | M7 | Public embeddable Rust SDK published on crates.io as `roxide-r` (`roxide` is taken on crates.io), exposing multi-session `RSession`, permissions, and snapshots | `forbid` (or thin safe facade over `roxide-capi`) |
| `roxide-py` | M7 | Python embedding extension (`pip install roxide`) using PyO3/abi3 with zero global state and zero-copy Arrow interop | audited `unsafe` (PyO3 FFI boundary) |
| `roxide-wasm` | M7 | 4 MB `wasm32` engine build + loader for precompiled `wasm32` CRAN package binaries | audited `unsafe` (wasm-bindgen/ABI boundary) |
| `roxide-ide` (or `roxide-kernel`, `roxide-lsp`, `roxide-dap`) | M8 | Built-in Jupyter kernel, Positron (Ark) backend, LSP server, and DAP debugger | `forbid` |

## Coordinator-owned shared contracts

Only the coordinator (main thread) edits:

- Root `Cargo.toml`, `Cargo.lock` dependency additions, `scripts/`, `PROGRESS.md`, `docs/ADR.md`, and `docs/VISION.md`.
- Public traits and core representations in `roxide-diag`, `roxide-host`, `roxide-gc`, `roxide-value`, `roxide-syntax`, and `roxide-eval` hook interfaces (`NativeHooks`, `S4Hooks`, `NamespaceLoader`).
- Top-level primitive and package wiring in `roxide-base/src/lib.rs`, `roxide-stats/src/lib.rs`, and `roxide-cli/src/lib.rs`.
- `roxide-harness` test formats and conformance baselines.

Workers implement disjoint crates or disjoint submodules and request shared contract changes from the coordinator.

## Critical Roxide invariants and design contracts

1. **Cycle-safe non-moving GC + copy-on-modify + ALTREP (ADRs 0002, 0003, 0017, 0019):**
   - `SexpId(NonZeroU64)` is the opaque 64-bit `SEXP` value. `Heap::alloc` never collects; collection happens strictly at explicit safepoints. `RVec<T>` backing buffers stay pinned for zero-copy C pointers.
   - Saturating copy-on-modify refcounts (`REFCNT_MAX = 7`) preserve value semantics and enforce R 4.6.2's wrapper duplication guard.
   - ALTREP supports compact sequences, wrappers, foreign C ALTREP classes (Seams C1-C7), and zero-copy Arrow arrays (M7).
2. **Exact missing values (`NA` vs `NaN`) and 4-state lazy promises (ADRs 0005, 0006):**
   - `NA_integer_ == i32::MIN` with overflow warnings; `NA_real_` preserves IEEE 754 payload `1954` distinct from `NaN`; Kleene three-valued logic for `logical`; handle-identity `NA_character_`.
   - 4-state lazy promises (`Unforced`, `Evaluating`, `Interrupted`, `Forced`) preserve `substitute()`, `match.call()`, `missing()`, and exact argument matching.
3. **Unwind safety and GC rooting across `libR` over `unsafe` crate counts (ADRs 0018, 0023):**
   - Every Rust-to-C call enters a setjmp + `try { ... } catch (...)` trampoline; every C-to-Rust entry uses a panic-free plain-old-frame `boundary()` helper so Rust never unwinds into C and C++ exceptions never cross into Rust.
   - `#![forbid(unsafe_code)]` stays the default in pure logic crates, while audited, ADR-governed `unsafe` is permitted in explicit FFI/ABI/JIT boundary crates (`roxide-capi`, `roxide-libr`, `roxide-blas`, `roxide-arrow`, `roxide-py`, `roxide-wasm`) and verified via `ROXIDE_GC_TORTURE=1`, ASAN/UBSAN, Miri, and fuzzing.
4. **Clean-room MIT headers + common private-API compatibility shim (ADRs 0017, 0023):**
   - All headers under `crates/roxide-capi/include/` are authored clean-room from *Writing R Extensions* with `MIT OR Apache-2.0` SPDX headers and verified numerically by `abi_probe.py`.
   - To maximize CRAN pass rates on older packages, `roxide-capi` includes an audited compatibility layer for the most frequent private C API entry points identified by CRAN symbol census, while install-time advisories warn on known-broken packages.
5. **Bundled `faer` BLAS/LAPACK + Fortran ABI + bundled toolchain + binary repos (ADR-0024):**
   - `roxide-blas` wraps `faer` (MIT) behind explicit Fortran-ABI symbols so `Matrix` and `stats` work on bare machines without system OpenBLAS or `libgfortran`, while remaining swappable for Accelerate/OpenBLAS/MKL.
   - `roxide-pkg` drives prebuilt binary installs (native and `wasm32`), `renv.lock`, and a bundled C/C++/Fortran toolchain (`zig cc` plus Fortran) for zero-setup source installs.
6. **Three-tier multi-session isolation for compiled packages (ADR-0025):**
   - `RSession` holds zero interpreter globals. When compiled packages are loaded across concurrent sessions, `roxide-capi` routes native state per session via: (1) isolated per-session native library copies/namespaces, (2) a per-package `share_safe` manifest flag for verified reentrant libraries, or (3) a transparent process-pool fallback behind the same `RSession` API.
   - `Host` enforces Deno-style per-session permissions (paths, network hosts, subprocesses) and resource quotas (CPU, memory, wall time). Session snapshots persist and restore warmed-up sessions in milliseconds.
7. **Realigned numerical, serialization, and VM/JIT contracts (ADRs 0020, 0022):**
   - Bidirectional XDR v2/v3 `.rds` and `.RData` interchange (gzip, bzip2, xz, zstd) including S4 and ALTREP.
   - **No `.Rc` bytecode roundtripping:** read-only `BCODESXP` `consts[0]` unwrap preserves compatibility with existing `.rdb`/`.rds` files, while Roxide's register IR and targeted native JIT optimize freely for real `dplyr`, `data.table`, and `purrr` workloads.
   - **PRNG and distributions:** bit-exact default Mersenne-Twister plus `runif`, `rnorm`, and `sample`; tight numerical agreement (`< 1e-14` relative error) across the remaining 22 distributions.

## Layered differential oracle strategy

All verification runs black-box against the official GNU R 4.6.2 binary (`Rscript --vanilla` / `R --vanilla`) and runs in CI on every change:

| Layer | What it tests | Pass criterion |
| --- | --- | --- |
| **Layer 1: Exact language and `base` parity** | Syntax, `deparse`, evaluation, promises/NSE, subsetting, S3, `print`/`format`, conditions, `base` text output, `.rds`/`.RData` | Byte-for-byte stdout, stderr, exit code, and cross-engine `.rds`/`.RData` roundtrip vs. GNU R 4.6.2 |
| **Layer 2: Numerical and PRNG parity** | Default MT19937 + `runif`/`rnorm`/`sample`, 22 distributions, linalg (`qr`/`svd`/`eigen`/`chol`), `lm`/`glm`/`optim`/`nlminb`, `utils`/`tools` | Bit-exact MT19937 + core samplers; `< 1e-14` relative error on distributions and tight linalg/model tolerances |
| **Layer 3: S4, packages and graphics** | S4/RefClass dispatch, `NAMESPACE`/`.rdb`/`.rdx`/`R CMD`, `grDevices`/`graphics`/`grid`, `ragg`-parity text/CJK/emoji | Exact numeric/geometry match, pixel tolerance vs. `ragg`, and `ROXIDE_GC_TORTURE=1` pass |
| **Layer 4: Curated pure-R and compiled CRAN tiers** | Tier-2 pure-R (`R6`, `withr`, `crayon`, `praise`, `magrittr`, `zeallot`) and Tier-3 compiled (`jsonlite`, `rlang`, `vctrs`, `cli`, `cpp11`, `Rcpp`, `data.table`, `dplyr`, `ggplot2`, `Matrix`) | Test-suite parity with GNU R, zero crashes, GC-torture and multi-session isolation pass |
| **Layer 5: Nightly CRAN/Bioconductor matrix and workload benchmarks** | Full CRAN and Bioconductor package test suites ranked by reverse-dependency count, plus real `dplyr`/`data.table`/`purrr` and Arrow interop benchmarks | Tracked ecosystem pass rate, zero regressions on pinned reverse-dep leaders, and advisory table updates |
