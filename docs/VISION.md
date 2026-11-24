# Working-backwards product vision

Roxide works backwards from the target announcement below. Every milestone in [`docs/ROADMAP.md`](ROADMAP.md), architectural contract in [`docs/ARCHITECTURE.md`](ARCHITECTURE.md), and decision in [`docs/ADR.md`](ADR.md) exists to make every claim in this announcement true and verifiable.

## Target Show HN announcement

**Show HN: Roxide, a clean-room MIT-licensed R 4.6 in Rust that runs CRAN**

I built a clean-room drop-in replacement for GNU R 4.6.2 in Rust, dual-licensed under MIT and Apache-2.0.

It installs and runs compiled C, C++, and Fortran CRAN packages unmodified, including `dplyr`, `ggplot2`, `data.table`, `Matrix`, `rlang`, `vctrs`, `cli`, `jsonlite`, `Rcpp`, and `htmlwidgets`, straight through `install.packages()`.

```sh
curl -fsSL https://roxide.dev/install.sh | sh

roxide -e '
  install.packages(c("dplyr", "ggplot2", "data.table", "Matrix"))
  library(dplyr); library(ggplot2)
  set.seed(42)
  df <- tibble(g = rep(c("a", "b"), each = 5000), x = rnorm(10000))
  png("out.png", 800, 500)
  print(ggplot(df, aes(x, fill = g)) + geom_density(alpha = 0.4))
  dev.off()
'
```

That runs from a single native binary on a bare machine with no system compilers, OpenBLAS, `libgfortran`, X11, Cairo, or Pango installed. Packages install in seconds from a prebuilt binary repo, and `out.png` writes in under 250 ms.

- **Instant installs and zero toolchain setup.** `install.packages()` pulls from a prebuilt binary package repository and writes `renv`-compatible lockfiles for reproducible environments. When a package needs to build from source, Roxide falls back to a bundled C, C++, and Fortran toolchain (built around `zig cc`) and a bundled, swappable pure-Rust BLAS and LAPACK backend (`faer`) with Fortran ABI symbols so `Matrix` and `stats` work out of the box.
- **CRAN and Bioconductor tested nightly.** Every package on CRAN and Bioconductor runs its own test suite against GNU R nightly, prioritized by reverse-dependency count so the packages others depend on most get fixed first. Roxide ships clean-room MIT headers written from *Writing R Extensions*, a compatibility layer for the most common private C API calls used by older packages, and install-time warnings for packages known to fail. Every change runs the GNU R oracle in CI, and new GNU R releases land within days.
- **Multi-session isolation that works with compiled packages.** `RSession` holds zero interpreter globals. Native packages run across concurrent sessions through per-session isolated library copies, a per-package share-safe manifest flag, or a transparent process-pool fallback behind the same API. Each session supports Deno-style permissions (directory read/write allowlists, network host allowlists, CPU, memory, and time limits), millisecond session snapshots that restore a warmed-up `tidyverse` environment, and an in-process `future` and `parallel` backend with no forking.
- **Real-world speed and zero-copy Arrow.** `Rscript -e "1+1"` boots in 4 ms and 9 MB RAM versus 185 ms and 55 MB on GNU R 4.6.2. Beyond startup, Roxide optimizes real `dplyr`, `data.table`, and `purrr` workloads with multithreaded base vector operations, zero-copy Arrow data frames over ALTREP for DuckDB, Polars, and Python, and a targeted native JIT for hot paths. We intentionally dropped `.Rc` bytecode roundtripping so the VM and JIT are not locked to GNU R's bytecode format.
- **Graphics, text, and interactive widgets.** Headless PNG, SVG, and PDF devices ship alongside a native desktop window device. Text shaping supports font fallback, CJK, and emoji matching `ragg` within a tight pixel tolerance, and `htmlwidgets` and `plotly` work out of the box.
- **Embed everywhere, including compiled CRAN in the browser.** Host applications embed isolated `RSession` instances from Python (`pip install roxide`, without `rpy2` global-state issues), Rust (`cargo add roxide-r`, since `roxide` is taken on crates.io), C, and JavaScript via a 4 MB WASM engine paired with a precompiled `wasm32` CRAN package repo.
- **Modern editor and notebook tooling.** One binary ships a Jupyter kernel, a Positron (Ark) backend, an LSP, a DAP debugger that replaces `browser()`, and error messages with the exact line and column, full call chain, and a suggested fix.
- **Numerical and clean-room guarantees.** Default Mersenne-Twister RNG and core samplers (`runif`, `rnorm`, `sample`) match GNU R bit for bit, and all 22 distribution families agree within `1e-14` relative error. `.rds` and `.RData` files roundtrip in both directions, including S4 objects and ALTREP vectors. Zero GNU R source files, base `.R` scripts, `nmath` files, or GPL tests were read or copied. Safety effort focuses on sound `Rf_error`/`longjmp` and C++ exception unwinding plus GC rooting across `libR`, verified continuously under `ROXIDE_GC_TORTURE=1`.

RStudio's private C hooks and older packages reaching into unshimmed `SEXPREC` fields or platform-specific GUI toolkits do not work and trigger a warning at install time.

Try the live WASM REPL or grab the source at https://github.com/dvgyl/roxide.

---

## Top 3 product priorities

Every roadmap trade-off resolves in favor of these three priorities first:

1. **CRAN-wide and Bioconductor pass rate.** Nightly testing across all of CRAN and Bioconductor comparing each package's own tests against GNU R, fixing the highest reverse-dependency packages first, supported by MIT headers from *Writing R Extensions*, a compatibility layer for common private C API calls, and install-time broken-package advisories.
2. **Bundled toolchain plus prebuilt binary package repository.** A prebuilt binary repo and cached binary installer so `dplyr`, `data.table`, and `Matrix` install in seconds, paired with a bundled C/C++/Fortran toolchain (`zig cc` plus Fortran), a bundled swappable pure-Rust BLAS/LAPACK backend (`faer`) with Fortran ABI exports, and `renv`-compatible lockfiles.
3. **Multi-session execution working with compiled packages.** Concurrent isolated `RSession` instances supporting native CRAN packages behind one API via per-session native library copies, a per-package share-safe flag, and a transparent process-pool fallback, combined with Deno-style permissions, millisecond snapshots, and an in-process `future`/`parallel` backend.

---

## Explicit cut list and architectural realignments

To keep the product cohesive and avoid fighting our own architecture, these boundaries are locked (see ADRs 0021 through 0025 in [`docs/ADR.md`](ADR.md)):

- **Drop `.Rc` bytecode roundtripping.** Almost no workflow exchanges compiled bytecode across R implementations, and mirroring GNU R's bytecode format blocks better VM and JIT optimization. Roxide still unwraps `BCODESXP` `consts[0]` expressions when reading external `.rdb` or `.rds` files and keeps `compiler::cmpfun` and `enableJIT` as wrappers over Roxide's own IR and JIT.
- **Narrow the bit-exact PRNG guarantee.** Default Mersenne-Twister plus core samplers (`runif`, `rnorm`, `sample`) remain bit-exact with GNU R. The remaining 22 distributions and alternate RNG or tail paths target tight numerical agreement (under `1e-14` relative error) rather than bit-for-bit identity on obscure tails.
- **Prioritize unwind and GC soundness over minimizing `unsafe` crate counts.** Keep `#![forbid(unsafe_code)]` as the default for pure logic crates, while judging low-level FFI, `Rf_error`/`longjmp`, C++ exception walls, Fortran ABI shims, Arrow C Data Interface, and native multi-session isolation by actual memory safety (GC torture, ASAN/UBSAN, Miri, fuzzing) instead of crate counts.
- **Publish the embeddable Rust crate as `roxide-r`.** The name `roxide` is already taken on crates.io by an unrelated safe-`rm` CLI tool. Keep `roxide` for the CLI binary and PyPI package (`pip install roxide`), `@roxide/wasm` / `roxide-wasm` for JavaScript, and `roxide-r` for crates.io.
