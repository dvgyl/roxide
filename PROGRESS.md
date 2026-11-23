# Roxide Parity & Milestone Progress

**Target Oracle:** Official GNU R 4.6.2 (`R-4.6.2-arm64.pkg`, macOS arm64 black-box binary)  
**Repository:** `https://github.com/dvgyl/roxide`

## Milestone Status

| Milestone | Status | Summary |
| --- | --- | --- |
| **M0 — Bootstrap, Oracle, Clean-Room Guardrails & Core Contracts** | **Completed** | Dual licences (`MIT OR Apache-2.0`), clean-room scanner, official GNU R 4.6.2 binary oracle, ADRs 0001–0007, 8-crate Rust workspace, 12/12 smoke parity (incl. `ROXIDE_GC_TORTURE=1`) |
| **M1 — Complete R 4.6.2 Language Core & `base` Foundation** | **Completed (174 / 174 = 100%)** | Full R 4.6.2 syntax/`deparse`, cycle-safe GC + copy-on-modify + ALTREP, 4-state lazy promises/NSE/conditions/S3/active bindings, complete `roxide-base` (`ops`, `subset`, `strings`, `datetime`, `io_os`, `serialize` XDR v2/v3), **174/174 Layer 1 byte-for-byte parity** (normal & `ROXIDE_GC_TORTURE=1`), **100% cross-engine `.rds`/`.RData` roundtrip parity**, all cross-model review P0/P1/P2 findings closed |
| **M2 — Clean-Room `roxide-math`, `roxide-stats` & `roxide-utils`** | **Completed (293 / 293 = 100%)** | 12-crate `#![forbid(unsafe_code)]` workspace (0 new deps), ADRs 0009–0011, 100% bit-exact PRNG across all 7 uniform generators + all 22 distribution RNGs (incl. R 4.6.2 large-$N$ `rhyper`), full `d/p/q/r*` continuous/discrete/noncentral/Tukey & special/Bessel functions, LINPACK/LAPACK-parity `qr`/`svd`/`eigen`/`chol`/`solve`/complex/FFT, `stats` (`formula`/`terms`/`model.matrix`/`lm`/`glm`/11 `htest`/`optim`/`nlminb`/`nlm`/`uniroot`/`integrate`/`ts`/`multivar`/`density`/`loess`), `utils` (`read.table`/`str`/`head`/`tail`/`capture.output`/`sessionInfo`/`dump`/`dput`), `base::dcf`, `tools` (`parse_Rd`/`Rd2txt`/`analyze_license`), **293/293 Layer 2 parity** + all W10A–W10E peer-review hardening waves closed |
| **M3 — S4 (`methods`), Graphics & Pure-R Packages** | **In Progress** | Clean-room empirical oracle spec & zero-new-dependency architecture blueprint complete (`roxide-methods`, `roxide-pkg`, `roxide-graphics`); starting W0 coordinator contracts |
| **M4 — R 4.6.2 Public C API (`roxide-capi`), Compiled CRAN & Perf** | Planned | Isolated `unsafe` `libR` ABI, compiled CRAN corpus, bytecode VM, fuzzing |

## Differential Oracle Scoreboard (Live GNU R 4.6.2)

### Layer 1 — Core Language & `base` (Byte-for-Byte Exact)

| Layer | Suite | Pass | Fail | Total | Notes |
| --- | --- | ---: | ---: | ---: | --- |
| Layer 1 | `00_smoke.rcase` | 12 | 0 | 12 | 100% (normal & `ROXIDE_GC_TORTURE=1`) |
| Layer 1 | `01_syntax_deparse.rcase` | 25 | 0 | 25 | 100% (normal & `ROXIDE_GC_TORTURE=1`) |
| Layer 1 | `02_values_na_coercion.rcase` | 25 | 0 | 25 | 100% (normal & `ROXIDE_GC_TORTURE=1`) |
| Layer 1 | `03_eval_promises_nse.rcase` | 25 | 0 | 25 | 100% (normal & `ROXIDE_GC_TORTURE=1`) |
| Layer 1 | `04_subset_base_core.rcase` | 25 | 0 | 25 | 100% (normal & `ROXIDE_GC_TORTURE=1`) |
| Layer 1 | `05_strings_regex.rcase` | 25 | 0 | 25 | 100% (normal & `ROXIDE_GC_TORTURE=1`) |
| Layer 1 | `06_files_io_datetime.rcase` | 22 | 0 | 22 | 100% (normal & `ROXIDE_GC_TORTURE=1`) |
| Layer 1 | `07_rds_rdata_roundtrip.rcase` | 15 | 0 | 15 | 100% (normal & `ROXIDE_GC_TORTURE=1`) + 20/20 cross-engine roundtrips |
| **Total** | **Layer 1 (Suites 00–07)** | **174** | **0** | **174** | **100.0% — Milestone 1 Closed!** |

### Layer 2 — `roxide-math`, `roxide-stats`, `roxide-utils` & `tools`

| Layer | Suite | Pass | Fail | Total | Notes |
| --- | --- | ---: | ---: | ---: | --- |
| Layer 2 | `00_rng_streams.rcase` | 8 | 0 | 8 | Bit-exact across all 7 uniform PRNGs |
| Layer 2 | `01_random_seed_sync.rcase` | 10 | 0 | 10 | Bit-exact `.Random.seed` & `RNGkind()` transitions |
| Layer 2 | `02_rng_variates.rcase` | 8 | 0 | 8 | Bit-exact across all 22 distribution RNGs |
| Layer 2 | `03_special_functions.rcase` | 23 | 0 | 23 | Gamma, Beta, Digamma/Psigamma, Bessel `I/J/K/Y` |
| Layer 2 | `04_dist_continuous.rcase` | 8 | 0 | 8 | All continuous, noncentral & Tukey `d/p/q/r*` |
| Layer 2 | `05_dist_discrete.rcase` | 4 | 0 | 4 | All discrete `d/p/q/r*` + R 4.6.2 large-$N$ `rhyper` |
| Layer 2 | `06_linalg_core.rcase` | 14 | 0 | 14 | `%*%`, `qr` (LINPACK/LAPACK), `chol`, `solve`, `rcond`, `det` |
| Layer 2 | `07_linalg_spectral.rcase` | 14 | 0 | 14 | Real & complex `svd`, `La.svd`, `eigen`, `norm`, `kappa` |
| Layer 2 | `08_fft_ts.rcase` | 20 | 0 | 20 | `fft`, `mvfft`, `convolve`, `ts`, `acf`/`pacf`, `decompose`, `Box.test` |
| Layer 2 | `09_model_frame_matrix.rcase` | 5 | 0 | 5 | `formula`, `terms`, `model.frame`, `model.matrix`, `contr.*`, `poly` |
| Layer 2 | `10_lm_anova.rcase` | 27 | 0 | 27 | `lm`, `mlm`, `summary.lm`, `anova.lm`, `predict.lm`, `dummy.coef`, influence |
| Layer 2 | `11_glm.rcase` | 33 | 0 | 33 | `glm`, `glm.fit`, all families/links, `summary`/`anova`/`predict.glm` |
| Layer 2 | `12_htest.rcase` | 18 | 0 | 18 | 11 `htest` families, `p.adjust`, `print.htest` |
| Layer 2 | `13_optim_roots_integrate.rcase` | 17 | 0 | 17 | All 6 `optim` methods, `optimHess`, `optimize`, `uniroot`, `integrate` |
| Layer 2 | `14_descriptive.rcase` | 7 | 0 | 7 | `var`, `sd`, `cov`, `cor`, `median`, `quantile` (types 1–9), `IQR`, `mad` |
| Layer 2 | `15a_multivar.rcase` | 17 | 0 | 17 | `dist`, `hclust`, `cutree`, `kmeans`, `prcomp`, `princomp`, `cmdscale` |
| Layer 2 | `15b_smooth_nlminb.rcase` | 23 | 0 | 23 | `density`, `approx`, `spline`, `ecdf`, `lowess`, `loess`, `nlminb`, `nlm` |
| Layer 2 | `16_utils_read_write.rcase` | 9 | 0 | 9 | `scan`, `type.convert`, `count.fields`, `read.table`/`read.csv`/`write.csv` |
| Layer 2 | `17_utils_str_head.rcase` | 8 | 0 | 8 | `str`, `head`, `tail`, `capture.output`, `object.size`, `sessionInfo` |
| Layer 2 | `18_base_dcf.rcase` | 5 | 0 | 5 | UTF-8 `read.dcf` / `write.dcf` with `#` comments |
| Layer 2 | `19_tools_rd_license.rcase` | 15 | 0 | 15 | `tools::parse_Rd`, `tools::Rd2txt` (R 4.6.2), `tools::analyze_license` |
| **Total** | **Layer 2 (Suites 00–19)** | **293** | **0** | **293** | **100.0% — Milestone 2 Closed!** |
