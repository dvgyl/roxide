# Roadmap and milestones

This roadmap works backwards from [`docs/VISION.md`](VISION.md). Milestones M0 through M3 are complete, M4 is in progress at Wave 5, and M5 through M8 sequence the remaining product work in strict priority order.

## Top 3 product priorities

1. **CRAN-wide and Bioconductor pass rate** (nightly full-ecosystem test runs against GNU R, reverse-dependency-first fixes, common private-API compatibility shims, clean-room MIT headers, install-time broken-package warnings, fast new-R-release turnaround).
2. **Bundled toolchain plus prebuilt binary package repo** (prebuilt native CRAN binaries for multi-second installs, bundled C/C++/Fortran toolchain via `zig cc`, bundled swappable pure-Rust `faer` BLAS/LAPACK with Fortran ABI symbols, `renv.lock` support).
3. **Multi-session execution working with compiled packages** (per-session isolated native copies, per-package share-safe manifest flag, transparent process-pool fallback behind `RSession`, Deno-style permissions, millisecond snapshots, in-process `future`/`parallel`).

## Explicit cut list and constraints

- Drop `.Rc` bytecode roundtripping (ADR-0022). Keep `BCODESXP` `consts[0]` unwrap on read and `compiler` API wrappers over Roxide's own IR and JIT.
- Narrow the forward bit-exact PRNG guarantee to default Mersenne-Twister plus `runif`, `rnorm`, and `sample`, targeting `< 1e-14` relative error across the remaining 22 distributions (ADR-0022).
- Prioritize safe `Rf_error`/`longjmp`, C++ exception unwinding, and GC rooting across `libR` over minimizing `unsafe` crate counts (ADR-0023).
- Publish the embeddable Rust library crate as `roxide-r` on crates.io because `roxide` is already taken (ADR-0021).

---

## Completed milestones (M0 through M3)

### Milestone 0: Bootstrap, oracle, clean-room guardrails and core contracts (Completed)

- Dual licences (`MIT OR Apache-2.0`), clean-room scanner (`tools/measure/licence_scan.py`), official GNU R 4.6.2 macOS arm64 binary oracle in `env/oracle/`, ADRs 0001 through 0007, core contracts (`SexpId`, safepoint GC, `Host` trait, 4-state promises, exact `NA_real_` 1954 semantics), and 12/12 smoke parity under default and `ROXIDE_GC_TORTURE=1`.

### Milestone 1: Complete R 4.6.2 language core and `base` foundation (Completed, 174/174)

- Full R 4.6.2 parser and `deparse()`, cycle-safe mark-sweep GC with saturating copy-on-modify refcounting and ALTREP, 4-state lazy promises and NSE, conditions, S3 dispatch, complete `roxide-base`, 174/174 Layer 1 byte-for-byte parity (normal and GC torture), and 100% `.rds`/`.RData` cross-engine roundtrip parity.

### Milestone 2: Clean-room `roxide-math`, `roxide-stats` and `roxide-utils` (Completed, 293/293)

- Pure-Rust leaf `roxide-math` (PRNGs, 22 distribution families, special/Bessel functions, LINPACK/LAPACK-parity linear algebra, mixed-radix FFT), `roxide-stats` (`formula`, `lm`, `glm`, 11 `htest` families, `optim`, `nlminb`, `uniroot`, `integrate`, `ts`, `multivar`, `density`, `loess`), `roxide-utils` (`read.table`, `str`, `head`, `tail`, `sessionInfo`, DCF, `Rd2txt`, SPDX licence analysis), and 293/293 Layer 2 parity.

### Milestone 3: S4 (`methods`), graphics and pure-R packages (Completed, 186/186 + 2,341/2,341 Tier-2)

- Full S4 object system and Reference Classes with Pareto-dominance multiple dispatch, ephemeron GC and finalizers, `roxide-pkg` (`NAMESPACE`, `.rdb`/`.rdx`, `library`/`use`, `R CMD INSTALL`/`BATCH`), `roxide-graphics` (`grDevices`, `graphics`, `grid`, headless PDF/SVG/PNG/BMP/JPEG/TIFF devices, 115 HCL palettes), 653/653 Layers 1 through 3 parity, and 0 failures across all 6 pinned MIT Tier-2 CRAN packages.

---

## Milestone 4: R 4.6.2 C API, unwind safety, private-API compat and Tier-3 CRAN (In progress, Wave 5)

**Goal:** Close Tier-3a/3b/3c compiled CRAN packages (`rlang`, `vctrs`, `cli`, `purrr`, `tibble`, `cpp11`, `Rcpp`, `data.table`, `dplyr`, `ggplot2`) with hardened `libR` unwinding, GC rooting, and common private-API shims.

- [x] `485/485` `api,data` exports, `164/164` ABI probe, C/C++ setjmp and `catch(...)` trampoline, `R_UnwindProtect`, full foreign ALTREP (Seams C1-C7 + 62 exports), clean-room register VM, Tier-3a `yaml`/`magrittr`/`glue` 100% pass.
- [ ] **Wave 5 Tier-3a/3b closure:** finish `fastmap`, `jsonlite`, `ps`, `processx`, `cpp11`, `rlang`, `cli`, `prettyunits`, `lifecycle`, `lobstr`, `vctrs`, `purrr`, `tibble`.
- [ ] **Unwind and GC rooting hardening (ADR-0023):** stress-test `Rf_error`/`longjmp`, `R_UnwindProtect`, C++ RAII unwinding (`cpp11` and `Rcpp`), and C-API GC rooting under `ROXIDE_GC_TORTURE=1`, ASAN/UBSAN, and random C-API fuzzing.
- [ ] **Common private-API compatibility layer:** add an audited compatibility shim (driven by CRAN symbol census, strictly clean-room) for the highest-frequency private C API symbols used by older CRAN packages so legacy compiled packages load and pass tests.
- [ ] **Compiled CRAN flagship tier:** compile, load, and pass test suites for `Rcpp`, `data.table`, `dplyr`, and `ggplot2` (and drop `.Rc` bytecode roundtrip maintenance per ADR-0022).
- [ ] **CI oracle gate:** run the GNU R oracle comparison in CI on every change.

---

## Milestone 5: Bundled `faer` BLAS/LAPACK + Fortran, bundled toolchain, binary repo and nightly CRAN/Bioconductor (Priorities #1 and #2)

**Goal:** Deliver zero-dependency, multi-second `install.packages()` on a bare machine and establish systematic CRAN-wide and Bioconductor pass-rate burn-down.

- [ ] **Bundled swappable pure-Rust BLAS/LAPACK (`faer`) + Fortran ABI (ADR-0024):**
  - Wrap pure-Rust `faer` (MIT) behind an explicit Fortran-ABI export layer (`dgemm_`, `dgesdd_`, `dpotrf_`, etc.) so `Matrix` and base `stats` run out of the box with zero system OpenBLAS or `libgfortran`.
  - Support runtime/build-time swapping to Accelerate, OpenBLAS, or MKL when desired.
- [ ] **Bundled C/C++/Fortran toolchain:**
  - Integrate a redistributable zero-setup C/C++/Fortran toolchain (`zig cc` / `zig c++` plus Fortran compilation support) into `R CMD SHLIB` and `R CMD INSTALL` so source builds succeed on a bare machine with no system compilers or OpenMP setup.
- [ ] **Prebuilt binary package repository and `renv.lock`:**
  - Define the Roxide binary package ABI metadata and archive layout, build a cached binary installer inside `install.packages()` (falling back to source builds via the bundled toolchain), and publish prebuilt binaries for top CRAN packages so `install.packages(c("dplyr", "ggplot2", "data.table", "Matrix"))` completes in seconds.
  - Add native `renv.lock` reading, resolution, and generation for reproducible installs.
- [ ] **Nightly full CRAN and Bioconductor test matrix + install-time advisories:**
  - Stand up automated nightly builds and test runs across all of CRAN and Bioconductor, comparing each package's own tests against GNU R.
  - Rank failures by reverse-dependency count (`revdep` priority queue) and burn down the top dependency graph first.
  - Publish a compact compatibility advisory index consumed by `install.packages()` to warn users at install time when a package is known to be broken or uses unsupported private hooks.
- [ ] **Fast GNU R release cadence:**
  - Automate oracle upgrade probes (`tools:::funAPI()`, ABI probe, Layer 1-4 diff) so each new GNU R release is supported within days.

---

## Milestone 6: Multi-session compiled packages, Deno-style permissions, snapshots and in-process parallelism (Priority #3)

**Goal:** Make concurrent `RSession` execution work seamlessly even when compiled C/C++/Fortran CRAN packages are loaded.

- [ ] **Three-tier multi-session native package isolation (ADR-0025):**
  - Replace the single-session-per-process `dyn.load` ceiling (ADR-0019) with TLS session routing and three transparent isolation modes behind `RSession`:
    1. Per-session isolated copies of native libraries (private OS namespace / copied DLL image per session where supported).
    2. Per-package `share_safe` manifest flag for stateless, reentrant native libraries that can safely share one loaded image across threads.
    3. Transparent worker process-pool fallback behind the same `RSession` API for legacy compiled packages with unfixable process-global mutable C state.
- [ ] **Deno-style per-session permissions and quotas (`roxide-host`):**
  - Enforce declarative allowlists per `RSession` for filesystem read/write directories, outbound network hosts, and subprocess spawning.
  - Enforce hard CPU time, heap/RSS memory, and wall-clock execution limits with clean cancellation.
- [ ] **Millisecond session snapshots:**
  - Snapshot and restore warmed-up `RSession` state (heap, namespaces, loaded package metadata, e.g., preloaded `tidyverse`) in milliseconds.
- [ ] **In-process `future` and `parallel` backend:**
  - Ship a clean-room backend for `parallel` and `future` backed by isolated in-process `RSession` workers, providing true multi-core parallelism with zero `fork()` hazards and low memory footprint.

---

## Milestone 7: Real-world performance, zero-copy Arrow, production graphics and embedding SDKs + WASM repo

**Goal:** Deliver real-world data-frame throughput, publication-quality graphics and widgets, and first-class embedding across Rust, Python, C, and browser WASM.

- [ ] **Real-world workload optimization and targeted JIT:**
  - Build a real-world benchmark suite covering `dplyr`, `data.table`, and `purrr` pipelines alongside startup and tight loops (`docs/BENCH.md`).
  - Add multithreaded base vector operations (deterministic parallel execution over large numeric, logical, and character vectors).
  - Add a profile-guided native JIT targeting the hot code paths where real workloads spend the most time.
- [ ] **Zero-copy Arrow data frames via ALTREP (`roxide-arrow`):**
  - Implement Arrow C Data Interface import/export backed by ALTREP columns so DuckDB, Polars, and Python exchange data frames with Roxide with zero copies.
- [ ] **Production graphics, `ragg`-parity text, window device and widgets:**
  - Upgrade text layout and rasterization with real font discovery, fallback, shaping, CJK, and emoji matching `ragg` within a tight pixel tolerance.
  - Add a native cross-platform interactive window device (no X11/XQuartz dependency) alongside hardened PNG, SVG, and PDF devices.
  - Support `htmlwidgets` and `plotly` rendering pipelines.
- [ ] **First-class embedding SDKs (`RSession`):**
  - **Rust (`crates/roxide-r`, published as `roxide-r` on crates.io):** ergonomic multi-session API with zero global state.
  - **Python (`crates/roxide-py`, published as `pip install roxide`):** PyO3/abi3 wheels with multi-session support and zero-copy Arrow/Polars/DuckDB exchange.
  - **C (`roxide_embed.h`):** explicit `roxide_session_t*` embedding header alongside `libR`.
  - **JavaScript/WASM (`crates/roxide-wasm`, 4 MB target):** offline browser engine paired with a **precompiled `wasm32` CRAN package repository** so compiled CRAN packages run directly in browser tabs without an in-browser C compiler.

---

## Milestone 8: Jupyter kernel, Positron backend, LSP, DAP debugger and rich diagnostics

**Goal:** Make Roxide a complete drop-in interactive environment without relying on RStudio's private C hooks.

- [ ] **Built-in Jupyter kernel and Positron (Ark) backend:**
  - Ship `roxide kernel` supporting Jupyter wire protocol, Positron/Ark comm messages, variables pane, help viewer, inline plots, and `htmlwidgets`.
- [ ] **Built-in LSP (`roxide lsp`):**
  - Diagnostics, completion, hover, go-to-definition, workspace symbols, and formatting driven directly by `roxide-syntax` and package metadata.
- [ ] **Built-in DAP debugger (`roxide dap`):**
  - Source-mapped breakpoints, conditional breakpoints, step in/over/out, full stack frames, and live frame evaluation replacing `browser()`.
- [ ] **Rich error diagnostics:**
  - Render Rust-style errors with exact line and column (`srcref`), source snippet, call chain, and a suggested fix by default in interactive sessions, while preserving exact GNU R stderr formatting under oracle/compatibility mode.
