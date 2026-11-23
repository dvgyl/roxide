# Roxide Parity & Milestone Progress

**Target Oracle:** Official GNU R 4.6.2 (`R-4.6.2-arm64.pkg`, macOS arm64 black-box binary)  
**Repository:** `https://github.com/dvgyl/roxide`

## Milestone Status

| Milestone | Status | Summary |
| --- | --- | --- |
| **M0 — Bootstrap, Oracle, Clean-Room Guardrails & Core Contracts** | **Completed** | Dual licences (`MIT OR Apache-2.0`), clean-room scanner, official GNU R 4.6.2 binary oracle, ADRs 0001–0007, 8-crate Rust workspace, 12/12 smoke parity (incl. `ROXIDE_GC_TORTURE=1`) |
| **M1 — Complete R 4.6.2 Language Core & `base` Foundation** | **In Progress (~61% of 174 Layer 1 cases; 95.5% of core suites 00–04)** | Wave 1 (`roxide-syntax` + `roxide-value`) & Wave 2 (`roxide-eval` + `roxide-base` `ops`/`subset`) landed; Wave 3 (`datetime`, `strings`, `io_os`, `serialize`) in flight |
| **M2 — Clean-Room `roxide-math`, `roxide-stats` & `roxide-utils`** | Planned | Reproducible PRNGs, `d/p/q/r*`, linear algebra, `lm`/`glm`/`optim`, `utils`/`tools` |
| **M3 — S4 (`methods`), Graphics & Pure-R Packages** | Planned | S4/RC, `NAMESPACE`/package loader, `grDevices`/`graphics`/`grid` SVG/PDF/PNG |
| **M4 — R 4.6.2 Public C API (`roxide-capi`), Compiled CRAN & Perf** | Planned | Isolated `unsafe` `libR` ABI, compiled CRAN corpus, bytecode VM, fuzzing |

## Differential Oracle Scoreboard (Live GNU R 4.6.2 Byte-for-Byte)

| Layer | Suite | Pass | Fail | Total | Notes |
| --- | --- | ---: | ---: | ---: | --- |
| Layer 1 | `00_smoke.rcase` | 12 | 0 | 12 | 100% (incl. `ROXIDE_GC_TORTURE=1`) |
| Layer 1 | `01_syntax_deparse.rcase` | 24 | 1 | 25 | 96% (1 pipe error stderr wording) |
| Layer 1 | `02_values_na_coercion.rcase` | 25 | 0 | 25 | 100% |
| Layer 1 | `03_eval_promises_nse.rcase` | 25 | 0 | 25 | 100% (incl. `ROXIDE_GC_TORTURE=1`) |
| Layer 1 | `04_subset_base_core.rcase` | 21 | 4 | 25 | 84% (`grepv` in Wave 3B + 3 stderr `Calls:` polish) |
| Layer 1 | `05_strings_regex.rcase` | 0 | 25 | 25 | Wave 3B launching |
| Layer 1 | `06_files_io_datetime.rcase` | 0 | 22 | 22 | Wave 3A (`datetime`) in flight; Wave 3C (`io_os`) next |
| Layer 1 | `07_rds_rdata_roundtrip.rcase` | 0 | 15 | 15 | Wave 3D (`serialize`) next |
| **Total** | **Layer 1 (Suites 00–07)** | **107** | **67** | **174** | **Core 00–04: 107 / 112 (95.5%)** |
