# Roxide Parity & Milestone Progress

**Target Oracle:** Official GNU R 4.6.2 (`R-4.6.2-arm64.pkg`, macOS arm64 black-box binary)  
**Repository:** `https://github.com/dvgyl/roxide`

## Milestone Status

| Milestone | Status | Summary |
| --- | --- | --- |
| **M0 — Bootstrap, Oracle, Clean-Room Guardrails & Core Contracts** | **Completed** | Dual licences (`MIT OR Apache-2.0`), clean-room scanner, official GNU R 4.6.2 binary oracle (`env/oracle/`), ADRs 0001–0007 locked, 8-crate Rust workspace (`roxide-diag`, `roxide-gc`, `roxide-host`, `roxide-syntax`, `roxide-value`, `roxide-eval`, `roxide-cli`, `roxide-harness`), 112 clean-room Layer 1 cases, 12/12 smoke parity (including `ROXIDE_GC_TORTURE=1`) |
| **M1 — Complete R 4.6.2 Language Core & `base` Foundation** | In Progress | Parallel GPT 6.2 Sol waves across syntax/`deparse`, values/coercion/ALTREP, lazy promises/NSE/conditions/S3 evaluator, disjoint `roxide-base` submodules, `.rds`/`.RData` XDR v2/v3 |
| **M2 — Clean-Room `roxide-math`, `roxide-stats` & `roxide-utils`** | Planned | Reproducible PRNGs, `d/p/q/r*`, linear algebra, `lm`/`glm`/`optim`, `utils`/`tools` |
| **M3 — S4 (`methods`), Graphics & Pure-R Packages** | Planned | S4/RC, `NAMESPACE`/package loader, `grDevices`/`graphics`/`grid` SVG/PDF/PNG |
| **M4 — R 4.6.2 Public C API (`roxide-capi`), Compiled CRAN & Perf** | Planned | Isolated `unsafe` `libR` ABI, compiled CRAN corpus, bytecode VM, fuzzing |

## Differential Oracle Scoreboard

| Layer | Suite | Pass | Fail | Total | Notes |
| --- | --- | ---: | ---: | ---: | --- |
| Layer 1 | M0 Walking Skeleton (`00_smoke.rcase`) | 12 | 0 | 12 | Byte-for-byte vs GNU R 4.6.2 (incl. `ROXIDE_GC_TORTURE=1`) |
| Layer 1 | M1 Syntax & `deparse()` (`01_syntax_deparse.rcase`) | 0 | 25 | 25 | Starting M1 Wave 1 |
| Layer 1 | M1 Values, `NA` & Coercion (`02_values_na_coercion.rcase`) | 0 | 25 | 25 | Starting M1 Wave 1 |
| Layer 1 | M1 Eval, Promises, NSE, S3 (`03_eval_promises_nse.rcase`) | 0 | 25 | 25 | Starting M1 Wave 1 |
| Layer 1 | M1 Subsetting & `base` Core (`04_subset_base_core.rcase`) | 0 | 25 | 25 | Starting M1 Wave 2 |
| Layer 2 | Numerical & PRNG Parity | 0 | 0 | 0 | Planned M2 |
| Layer 3 | `.rds`/`.RData` Roundtrip | 0 | 0 | 0 | Planned M1 Wave 4 |
