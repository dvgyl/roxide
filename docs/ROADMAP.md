# Roadmap & Milestones

## Storage & Environment Setup Note

- **Best option (recommended):** Move this folder to the internal **APFS** SSD (e.g., `~/dev/Roxide`) before running Milestone 0. With ~96 GB free on internal APFS, the entire project + `target/` + GNU R 4.6.2 oracle + test corpus will use only **~10–25 GB**, with 3–5× faster builds, working incremental compilation, and zero exFAT 1 MB-per-file or `._*` overhead.
- **Alternative (keep on external SSD):** If you prefer keeping everything on `/Volumes/Extreme Pro`, either place the repo inside an APFS `.sparsebundle` on `/Volumes/Extreme Pro`, or enforce the exFAT guardrails in [`AGENTS.md`](../AGENTS.md) (`CARGO_INCREMENTAL=0`, `COPYFILE_DISABLE=1`, temp dirs on `/tmp`).

---

## Milestone 0 — Bootstrap, Oracle, Clean-Room Guardrails & Core Contracts

**Goal:** Walking skeleton, official **GNU R 4.6.2** oracle installed, automated clean-room licence scanner active, and core `SEXP` / GC / Promise contracts locked by **Gemini 4 Neon + Opus 5.5** (with **GPT 6.2 Astra** consulted on GC rooting, `NA_real_` 1954 semantics, and promise/NSE evaluation rules).

- [ ] Initialize git repo, `.gitignore`, `LICENSE-MIT`, `LICENSE-APACHE`, `THIRD_PARTY_NOTICES.md`, `third_party/licences/`, `docs/ADR.md`, `docs/PRIOR_ART.md`, and `PROGRESS.md`.
- [ ] Install/pin official **GNU R 4.6.2** macOS arm64 binary (`R` + `Rscript`) into `env/oracle/` (gitignored) — **never** download or unpack GNU R C/R source trees into the workspace (strict clean-room rule!).
- [ ] Create `env/env.sh`, `init.sh` (with `commit-msg` hook rejecting AI trailers), `scripts/gate.sh`, and `tools/measure/licence_scan.py`.
- [ ] Create root `Cargo.toml` workspace with strict lints (`unsafe_code = "forbid"` default across pure crates, clippy `-D warnings`, rustdoc `-D warnings`).
- [ ] Scaffold core contract crates (`roxide-diag`, `roxide-host`, `roxide-syntax`, `roxide-gc`, `roxide-value`, `roxide-cli`, `roxide-harness`) so `roxide --version` and `roxide -e "1 + 1"` pass Layer 1 differential verification against GNU R 4.6.2.

---

## Milestone 1 — Complete R 4.6.2 Language Core & `base` Foundation

**Goal:** High-velocity parallel **GPT 6.2 Sol** waves delivering a faithful R 4.6.2 interpreter verified byte-for-byte against `Rscript --vanilla`.

- [ ] **Parser & `deparse()` (`roxide-syntax`):** Complete R 4.6.2 grammar (`|>`, `_`, `\(x)`, `r"(...)"`, `%op%`, `%||%`, raw/hex/complex literals, newline rules, `srcref`, `parse()`, `deparse()`).
- [ ] **GC, Values & Exact `NA` Semantics (`roxide-gc`, `roxide-value`):** Cycle-safe mark-sweep GC, copy-on-modify refcounting, `ALTREP` compact sequences + R 4.6.2 wrapper duplication fix, exact `NA_real_` (`1954`) vs `NaN`, `NA_integer_` overflow warnings, Kleene logic, coercion, vector recycling, `print()`/`format()`.
- [ ] **Evaluator & NSE (`roxide-eval`):** Lexical scoping, lazy promises (`PROMSXP`), `substitute()`, `eval()`, `match.call()`, `missing()`, `force()`, `...`, call argument matching, control flow (`if`, `for`, `while`, `repeat`, `switch`, `return`), conditions (`stop`, `warning`, `message`, `tryCatch`, `withCallingHandlers`, `on.exit`), S3 dispatch (`UseMethod`, `NextMethod`, group generics), namespaces & R 4.5 `use()`.
- [ ] **Parallel `roxide-base` Submodules:**
  - Arithmetic/math/comparison/logic ops, `seq`/`seq.int` (4.6.2 fix), `rep`, `c`, `cumvar` (4.6.0), `cumsum`/`cumprod`
  - Subsetting (`[`, `[[`, `$`) & replacement functions, `data.frame`, `factor`, `matrix`, `array`, `rle` (4.6.0 `print` fix)
  - Higher-order (`lapply`, `sapply`, `vapply`, `apply`, `mapply`, `Map`, `Reduce`, `Filter`, `do.call`)
  - Strings & Regex (`paste`/`paste0`, `sprintf`, `substr`, `strsplit`, `grep`, `grepv` (4.5), `grepl`, `sub`/`gsub`, `regexpr`/`gregexpr`, `fixed`/TRE/PCRE2)
  - Files, OS & Connections (`list.files(..., fixed = TRUE)`, `readLines`/`writeLines`, `readBin`/`writeBin`, `readChar`/`writeChar` (4.6.2 multibyte fix), `file`/`gzfile`/`bzfile`/`xzfile`/`textConnection`, `system2`)
  - Dates/Times (`Date`, `POSIXct`, `POSIXlt`, `strptime`/`strftime`, `difftime`)
  - Binary `.rds` / `.RData` XDR v2/v3 serialization (`readRDS`/`saveRDS`/`load`/`save`) with gzip/bzip2/xz/zstd
- [ ] **Exit criteria:** >= 95% pass rate on clean-room Layer 1 language/`base` suite + 100% `.rds`/`.RData` cross-engine roundtrip parity with GNU R 4.6.2.

---

## Milestone 2 — Clean-Room `roxide-math`, `roxide-stats` & `roxide-utils`

**Goal:** Roxide becomes a genuine statistical computing engine — **GPT 6.2 Astra** designs/verifies numerical precision & stability while parallel **GPT 6.2 Sol** fleets implement & test against GNU R 4.6.2.

- [ ] **Reproducible PRNG & Distributions (`roxide-math`):** `set.seed()` / `.Random.seed` (`Mersenne-Twister`, `L'Ecuyer-CMRG`, `Wichmann-Hill`, etc.), `sample()`, special functions (`gamma`, `lgamma`, `beta`, `bessel*`), full `d/p/q/r*` families (`norm`, `unif`, `exp`, `gamma`, `beta`, `chisq`, `t`, `f`, `binom`, `pois`, `geom`, `nbinom`, `hyper` with 4.6.2 large-$m+n$ fix, `cauchy`, `logis`, `lnorm`, `weibull`, `wilcox`, `signrank`, `multinom`).
- [ ] **Linear Algebra & Signal Processing (`roxide-math`):** `%*%`, `crossprod`, `tcrossprod`, `qr` (Householder with column pivoting matching R's rank-deficient handling), `svd`, `eigen`, `chol`, `solve`, `det`, `backsolve`, `fft`.
- [ ] **Statistical Modeling (`roxide-stats`):** Formula parsing (`~`, `terms`, `model.frame`, `model.matrix`, contrasts, `I()`, `:`/`*`/`^`), `lm()`, `glm()` (IRLS + families: `gaussian`, `binomial`, `poisson`, `Gamma`, `inverse.gaussian`, `quasi*`), `anova`, `predict`, `residuals`, `summary.lm`/`summary.glm`, `dummy.coef` (4.6.2 fix), `t.test`, `wilcox.test`, `chisq.test`, `fisher.test`, `ks.test`, `cor`/`cov`, `quantile`, `density`, `loess`, `optim` (Nelder-Mead, BFGS, CG, L-BFGS-B, Brent), `nlminb`, `uniroot`, `integrate`, `hclust`, `kmeans`, `prcomp`.
- [ ] **`utils` & `tools` (`roxide-utils`):** `read.table`/`read.csv`/`write.csv`, `str()`, `head`/`tail`, `dput`/`dump`, `sessionInfo()`, `extSoftVersion()` (`zstd`), UTF-8 `read.dcf`/`write.dcf` with `#` comments, `Rd` parser/`Rd2txt` (4.6.2 fix), `tools::analyze_license()` (SPDX).
- [ ] **Exit criteria:** Layer 2 numerical tolerances green across all distributions & linear models; Tier-1 real-world data-analysis & pure-R package scripts match GNU R 4.6.2.

---

## Milestone 3 — S4 (`methods`), Graphics (`grDevices`/`graphics`/`grid`) & Pure-R Packages

**Goal:** Support full R object systems, plotting pipelines, and pure-R CRAN packages (`library()`, `NAMESPACE`, `DESCRIPTION`).

- [ ] **S4 & Reference Classes (`roxide-methods`):** `setClass`, `setGeneric`, `setMethod`, inheritance & multiple dispatch, `@` slot access, validity checks, `methods` package bootstrap.
- [ ] **Package Loader & `R CMD`:** Full `NAMESPACE` (`export`, `import`, `importFrom`, `S3method`, `useDynLib`), lazy-load `.rdb`/`.rdx` database support, `library()`/`require()`/`loadNamespace()`/`use()`, `R CMD BATCH`, `R CMD INSTALL` (pure-R packages).
- [ ] **Graphics & `grid` (`roxide-graphics`):** Display list, `par()`, base plotting (`plot`, `hist`, `boxplot(..., warnN = FALSE)`, `lines`, `points`, `abline`, `legend`, `axis`, `layout`), `grid` viewport/grob tree engine, and clean-room `svg()`, `pdf()`, and `png()` output devices.
- [ ] **Exit criteria:** Tier-2 pure-R CRAN packages (e.g., `jsonlite`-pure/R6/testthat-core/magrittr/crayon/glue-pure/etc.) install, load, and pass their test suites under Roxide.

---

## Milestone 4 — R 4.6.2 Public C API (`roxide-capi`), Compiled CRAN Packages & Performance

**Goal:** Drop-in compiled package compatibility using R 4.6.2's cleaned-up C API + bytecode JIT/VM optimization.

- [ ] **`roxide-capi` (isolated audited `unsafe`):** Implement R 4.6.2's public C API (`Rinternals.h`, `R_ext/*.h`, `R_NO_REMAP` default for C++, C++20 default compilation flags in `R CMD SHLIB` / `R CMD INSTALL`, `PROTECT`/`UNPROTECT`, `R_registerRoutines`, `.Call`/`.External`/`.C`/`.Fortran`, `ALTREP` C API, experimental resizable vectors API).
- [ ] **Compiled CRAN Package Ladder:** Build and run real compiled C/C++ CRAN packages against `libR` provided by Roxide (`digest`, `jsonlite`, `rlang`, `vctrs`, `cli`, `glue`, `Matrix`, `Rcpp`, `data.table` stretch).
- [ ] **Bytecode Compiler & Performance:** Register-based bytecode VM (`compiler::cmpfun` parity), SIMD vectorized numeric ops, benchmark suite (`roxide` vs GNU R 4.6.2 startup latency, vector ops, `lm`, and peak RSS).
- [ ] **Fuzzing & Hardening:** Fuzz `roxide-syntax`, `.rds`/`.RData` deserializer, `read.table`/`read.dcf`, and regex engine (zero panics / bounded memory).

---

## Real-World Corpus Ladder (North Star)

- **Tier 1 (Language, Math & Data Workflows):** Real-world statistical scripts (linear/logistic regression, Monte Carlo simulations with fixed `set.seed()`, CSV/RDS pipelines, time-series & clustering workflows) compared directly against GNU R 4.6.2.
- **Tier 2 (Pure-R Ecosystem Packages):** Installing, loading, and running tests for pure-R packages (`R6`, `magrittr`, `withr`, `crayon`, `praise`, `testthat` pure subsets, `yaml`/DSL scripts).
- **Tier 3 (Compiled CRAN Packages via `roxide-capi`):** `digest` → `glue` → `jsonlite` → `cli` → `rlang` → `vctrs` → `Rcpp` → `data.table` / `ggplot2` (via `grid`) stretch!
