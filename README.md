# Roxide

**Repository:** [github.com/dvgyl/roxide](https://github.com/dvgyl/roxide)

Roxide is a strict clean-room, native Rust drop-in replacement for GNU R 4.6.2 (`MIT OR Apache-2.0`). It targets drop-in parity for `R`, `Rscript`, `R CMD`, compiled CRAN and Bioconductor packages, multi-session sandboxing, zero-copy Arrow interop, first-class host embedding, and browser WASM.

The project works backwards from [`docs/VISION.md`](docs/VISION.md).

## Top 3 priorities

1. **CRAN-wide and Bioconductor pass rate.** Nightly testing across all of CRAN and Bioconductor against GNU R, fixing the highest reverse-dependency packages first, supported by clean-room MIT headers written from *Writing R Extensions*, a compatibility layer for common private C API calls, install-time advisories for known-broken packages, and multi-day turnaround on new GNU R releases.
2. **Bundled toolchain plus prebuilt binary package repo.** Prebuilt native and `wasm32` binary CRAN repos so `install.packages()` finishes in seconds, `renv`-compatible lockfiles, a bundled C/C++/Fortran toolchain (`zig cc` plus Fortran) for source builds on bare machines, and a bundled, swappable pure-Rust BLAS/LAPACK backend (`faer`) with Fortran ABI exports so `Matrix` and `stats` work without system OpenBLAS or `libgfortran`.
3. **Multi-session execution with compiled packages.** Concurrent zero-global `RSession` instances supporting native CRAN packages behind one API (per-session isolated library copies, per-package share-safe flags, or a transparent process-pool fallback), paired with Deno-style permissions and resource limits, millisecond session snapshots, and an in-process `future`/`parallel` backend with no forking.

## Target product capabilities

- **R 4.6.2 language and standard libraries:** Full syntax (`|>`, `_`, `\(x)`, `r"(...)"`, `%||%`, `%op%`, exact `srcref`), cycle-safe non-moving GC, copy-on-modify refcounting, ALTREP, exact IEEE 754 `NA_real_` (`1954` payload) and Kleene logic, 4-state lazy promises and NSE, S3, S4 multiple dispatch, Reference Classes, and `.rds`/`.RData` binary interchange (gzip, bzip2, xz, zstd).
- **Real-world performance and Arrow:** Benchmarked optimization of real `dplyr`, `data.table`, and `purrr` workloads, multithreaded base vector operations, zero-copy Arrow data frames over ALTREP for DuckDB, Polars, and Python, and a targeted native JIT for hot paths.
- **Graphics and widgets:** Headless PNG, SVG, and PDF devices, an interactive native window device (zero X11/Cairo/Pango), `ragg`-parity text shaping with font fallback, CJK, and emoji, plus working `htmlwidgets` and `plotly`.
- **Embedding SDKs and WASM:** First-class multi-session `RSession` embedding in Rust (`cargo add roxide-r`, since `roxide` is taken on crates.io), Python (`pip install roxide`, without `rpy2` global-state issues), C (`roxide_embed.h`), and JavaScript via a 4 MB WASM engine paired with a precompiled `wasm32` CRAN package repo.
- **Interactive tooling and diagnostics:** Built-in Jupyter kernel, Positron (Ark) backend, LSP, DAP debugger replacing `browser()`, and Rust-grade error messages with exact line and column, call chain, and suggested fixes.

## Explicit cut list and contracts

- **No `.Rc` bytecode roundtripping:** Roxide unwraps `BCODESXP` `consts[0]` expressions when reading external `.rdb`/`.rds` files and keeps `compiler::cmpfun`/`enableJIT` over its own IR and JIT, without mirroring GNU R's bytecode format (ADR-0022).
- **Focused PRNG bit-exactness:** Default Mersenne-Twister plus `runif`, `rnorm`, and `sample` match GNU R bit for bit; the remaining 22 distributions target `< 1e-14` relative error (ADR-0022).
- **Unwind and GC soundness first:** Safe `Rf_error`/`longjmp`, C++ exception unwinding, and GC rooting across `libR` take precedence over minimizing `unsafe` crate counts (ADR-0023).

## Clean-room licensing

- **Roxide (`crates/*`, `tools/*`, `conformance/*`):** Dual-licensed under `MIT OR Apache-2.0`.
- **Strict clean-room wall:** GNU R is `GPL-2.0-or-later` / `GPL-3.0-or-later`. Zero GNU R source files, base `.R` scripts, `nmath` C files, or GNU R test files may be read, copied, or ported into this repository. All behavior is implemented from R manuals, published mathematical algorithms, permissive crates, and black-box differential oracle testing against official GNU R 4.6.2 binaries.

## Repository layout

```text
Roxide/
├── AGENTS.md                 # Agent operating rules, model routing, and clean-room wall
├── README.md                 # Project overview and documentation index
├── PROGRESS.md               # Live parity scoreboard (M0-M3 complete, M4 Wave 5 active)
├── THIRD_PARTY_NOTICES.md    # Permissive crate and algorithm attributions
├── Cargo.toml                # Rust workspace manifest (17 active crates)
├── crates/                   # Disjoint Rust engine, C API, CLI, and oracle harness crates
├── conformance/              # Clean-room differential test suites (Layers 1-4)
├── tests/                    # C/C++ C-API differential fixtures
├── fuzz/                     # Out-of-workspace cargo-fuzz harnesses
├── tools/                    # C-API manifest/ABI probes, CRAN corpus runner, benchmarks, licence scan
├── scripts/                  # Gate runner, R_HOME staging, and Cargo slot limiter
└── docs/
    ├── VISION.md             # Working-backwards target Show HN post and product priorities
    ├── HANDOFF_PROMPT.md     # Short continuation handoff prompt for new threads
    ├── ROADMAP.md            # Milestones M0-M8 working backwards from VISION.md
    ├── ARCHITECTURE.md       # Workspace crates, contracts, invariants, and 5-layer oracle
    ├── ADR.md                # Architectural Decision Records (ADR-0001 through ADR-0025)
    ├── MODEL_ROUTING.md      # Multi-model orchestration and quota policy
    └── PRIOR_ART.md          # Vetted permissive dependency and algorithm log
```

## Documentation index

- Target product vision and announcement: [`docs/VISION.md`](docs/VISION.md)
- Short handoff prompt: [`docs/HANDOFF_PROMPT.md`](docs/HANDOFF_PROMPT.md)
- Agent rules and clean-room guardrails: [`AGENTS.md`](AGENTS.md)
- Roadmap (M0 through M8): [`docs/ROADMAP.md`](docs/ROADMAP.md)
- Technical architecture and crate map: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- Architectural decisions (ADRs): [`docs/ADR.md`](docs/ADR.md)
- Model and subscription routing: [`docs/MODEL_ROUTING.md`](docs/MODEL_ROUTING.md)
- Live milestone scoreboard: [`PROGRESS.md`](PROGRESS.md)
