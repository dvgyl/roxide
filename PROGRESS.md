# Roxide Parity & Milestone Progress

**Target Oracle:** Official GNU R 4.6.2 (`R-4.6.2-arm64.pkg`, macOS arm64 black-box binary)  
**Repository:** `https://github.com/dvgyl/roxide`

## Milestone Status

| Milestone | Status | Summary |
| --- | --- | --- |
| **M0 — Bootstrap, Oracle, Clean-Room Guardrails & Core Contracts** | **Completed** | Dual licences (`MIT OR Apache-2.0`), clean-room scanner, official GNU R 4.6.2 binary oracle, ADRs 0001–0007, 8-crate Rust workspace, 12/12 smoke parity (incl. `ROXIDE_GC_TORTURE=1`) |
| **M1 — Complete R 4.6.2 Language Core & `base` Foundation** | **Completed (174 / 174 = 100%)** | Full R 4.6.2 syntax/`deparse`, cycle-safe GC + copy-on-modify + ALTREP, 4-state lazy promises/NSE/conditions/S3/active bindings, complete `roxide-base` (`ops`, `subset`, `strings`, `datetime`, `io_os`, `serialize` XDR v2/v3), **174/174 Layer 1 byte-for-byte parity** (normal & `ROXIDE_GC_TORTURE=1`), **100% cross-engine `.rds`/`.RData` roundtrip parity**, all cross-model review P0/P1/P2 findings closed |
| **M2 — Clean-Room `roxide-math`, `roxide-stats` & `roxide-utils`** | **In Progress** | ADRs 0009–0011 & empirical specs complete; scaffolding leaf `roxide-math`, `roxide-stats`, `roxide-utils`, package namespaces, and Layer 2 tolerance harness (W0) -> PRNG/AS241 FMA & special functions (W1) |
| **M3 — S4 (`methods`), Graphics & Pure-R Packages** | Planned | S4/RC, `NAMESPACE`/package loader, `grDevices`/`graphics`/`grid` SVG/PDF/PNG |
| **M4 — R 4.6.2 Public C API (`roxide-capi`), Compiled CRAN & Perf** | Planned | Isolated `unsafe` `libR` ABI, compiled CRAN corpus, bytecode VM, fuzzing |

## Differential Oracle Scoreboard (Live GNU R 4.6.2 Byte-for-Byte)

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
