# Prior Art & Dependency Licence Log

Every external Rust crate, C library, published algorithm, or specification adopted in Roxide is logged here **before** being added to `Cargo.toml`, with verified licence compatibility (`MIT OR Apache-2.0`).

| Name | Version / Reference | Licence | Purpose | Clean-Room / ADR |
| --- | --- | --- | --- | --- |
| Rust Standard Library (`std`) | Rust 1.99.1+ (edition 2024) | `MIT OR Apache-2.0` | Core runtime & standard collections | Baseline |
| GNU R 4.6.2 macOS arm64 binary (oracle only) | `R-4.6.2-arm64.pkg` (CRAN `sonoma-arm64`) | `GPL-2.0-or-later` (external black-box binary only; gitignored in `env/oracle/`, **never** linked or distributed) | Black-box differential oracle for `roxide-harness` | ADR-0001 |
| `regex` | `1.13.x` | `MIT OR Apache-2.0` | Pure-Rust regular expression engine (`roxide-base::strings`) | ADR-0008 |
| `fancy-regex` | `0.20.x` | `MIT` | Pure-Rust lookaround & backreference engine (`perl = TRUE`, POSIX backrefs) | ADR-0008 |
| `flate2` (+ `miniz_oxide`) | `1.1.x` (`miniz_oxide` backend) | `MIT OR Apache-2.0` (`MIT OR Zlib OR Apache-2.0`) | Pure-Rust gzip streams (`gzfile`, `.rds`/`.RData` gzip) | ADR-0008 |
| `bzip2` (+ `libbz2-rs-sys`) | `0.6.x` (Rust backend) | `MIT OR Apache-2.0` (`bzip2-1.0.6`) | Pure-Rust bzip2 streams (`bzfile`, `.rds`/`.RData` bzip2) | ADR-0008 |
| `lzma-rust2` | `0.25.x` (`std`, `encoder`, `xz`) | `Apache-2.0` | Pure-Rust XZ/LZMA2 streams (`xzfile`, `.rds`/`.RData` xz; avoids GPL/LGPL `xz2`/`lzma-sys` bundle) | ADR-0008 |
| `zstd` (+ `zstd-safe`, `zstd-sys` vendored libzstd 1.5.7) | `0.14.x` (`vendored`) | `BSD-3-Clause` (`BSD-3-Clause AND BSD-2-Clause`) | Zstandard streams (`saveRDS`/`readRDS`, `extSoftVersion()[["zstd"]] == "1.5.7"`) | ADR-0008 |
| Netlib Reference BLAS & LAPACK | `3.12.1` (upstream Netlib reference, never GNU R's bundled copy) | `BSD-3-Clause` | Clean-room Rust port of reference BLAS/LAPACK algorithms (`roxide-math::linalg`) for sign & rounding parity | ADR-0009 |
| QUADPACK | Piessens et al. (1983) | `Public Domain` | Adaptive Gauss-Kronrod quadrature (`dqags`/`dqagi` for `stats::integrate`) | ADR-0009 |
| Knuth TAOCP `ran_array` | Knuth (1997 / 2002) | `Public Domain` | `Knuth-TAOCP` & `Knuth-TAOCP-2002` PRNG engines (`roxide-math::rng`) | ADR-0009 |
| Published numerical & statistical papers (paper-only clean-room implementations) | Singleton 1969 (FFT); Wichura 1988 AS 241 (`qnorm`); DiDonato & Morris 1992 (`pbeta`); Mehta & Patel (`fisher.test`); Byrd/Lu/Nocedal/Zhu 1995 (`L-BFGS-B`); Hartigan & Wong 1979 AS 136 (`kmeans`); Royston 1995 AS R94 (`shapiro.test`); Marsaglia / L'Ecuyer / Kinderman-Ramage / Ahrens-Dieter / Kachitvichyanukul & Schmeiser | Clean-room mathematical equations (never ACM/GPL code) | Pure-Rust `roxide-math` & `roxide-stats` numerical kernels | ADR-0009 |
| Adobe Core-14 Helvetica AFM Font Metrics | Adobe AFM Specification 4.1 (Core-14 Helvetica metric tables) | Permissive Adobe AFM Notice | Clean-room text advance widths (`strwidth`, legend geometry, PDF/SVG metrics) in `roxide-graphics` | ADR-0015 |
| Hershey Simplex Vector Font | Allen Vincent Hershey / US NBS / NTIS (1967) | `Public Domain` | Clean-room stroke glyphs for headless PNG rasterizer in `roxide-graphics` | ADR-0015 |
| PDF 1.4, SVG 1.1, PNG (ISO/IEC 15948), POSIX.1-1988 `ustar` Specifications | Adobe PDF 1.4; W3C SVG 1.1; W3C PNG; IEEE Std 1003.1 | Public Specifications | Clean-room file writers (`roxide-graphics`) and pure-Rust tar archive reader (`roxide-pkg`) | ADR-0014, ADR-0015 |
| `colorspace` HCL palette parameters & trajectory equations | Zeileis et al. (2020) *JSS* 96(1) / CRAN `colorspace` 2.1-4 | `BSD-3-Clause` | Analytical HCL palette trajectories (`hcl.colors`, `hcl.pals`) in `roxide-graphics::grdevices` | ADR-0016 |
| X11 `rgb.txt` named color catalog & public qualitative palettes (Okabe-Ito, ColorBrewer, Tableau) | X.Org `rgb.txt` (MIT/X11); Okabe & Ito (2002); Brewer (Apache-2.0); Tableau | Permissive / Public | `colors()` 657-entry catalog and `palette.colors()` / `palette.pals()` in `roxide-graphics::grdevices` | ADR-0016 |

## Forbidden Sources (Strict Clean-Room Wall)

The following must **never** be read, copied, translated, or vendored into Roxide:
- GNU R source code (`src/main/*.c`, `src/nmath/*.c`, `src/library/*/R/*.R`, `src/library/*/src/*`, `src/include/*` internals)
- GNU R test suite (`tests/*`)
- Any GPL/AGPL/LGPL-only crate or CRAN package source code (including `xz2`/`lzma-sys` bundled XZ source trees)
