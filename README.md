# Roxide

**Repository:** [github.com/dvgyl/roxide](https://github.com/dvgyl/roxide)

**Roxide** is a strict clean-room, native Rust drop-in replacement for **GNU R 4.6.2** (`MIT OR Apache-2.0`), targeting drop-in compatibility for `R`, `Rscript`, `R CMD`, the R 4.6.2 language and standard libraries, `.rds`/`.RData` binary interchange, reproducible PRNG/numerical computing, and R 4.6.2's cleaned-up public C API for compiled CRAN packages.

## Target Compatibility

- **Reference oracle:** Official **GNU R 4.6.2** (released October 29, 2026; 4.6 series *"Because it was There"*)
- **Modern R 4.x – 4.6.2 language & library surface in scope:**
  - Full R syntax: native pipe `|>` with `_` placeholder (including extraction), shorthand lambdas `\(x) ...`, raw strings `r"(...)"`, `%||%`, `%op%`, integer/complex/hex literals, exact `srcref` & `deparse()`
  - Complete `SEXP` runtime: cycle-safe GC, copy-on-modify reference counting, `ALTREP` compact/deferred vectors, exact missing values (`NA_integer_`, `NA_real_` IEEE 754 payload `1954` distinct from `NaN`, `NA_character_`, Kleene `NA_logical_`), lazy promises & NSE (`substitute`, `eval`, `match.call`, `...`)
  - Object systems: S3 (`UseMethod`, `NextMethod`, group generics), S4 (`methods`, multiple dispatch), Reference Classes
  - R 4.5–4.6.2 additions & fixes:
    - `use()` selective imports, `grepv()`, `cumvar()`, `list.files(..., fixed = TRUE)`, `boxplot(..., warnN = FALSE)`, `rle()` `print()` forwarding `...`
    - UTF-8 `read.dcf()` / `write.dcf()` with `#` comment support, `tools::analyze_license()` SPDX identifiers, `zstd` in `extSoftVersion()`, default C++20 for package builds
    - R 4.6.2 correctness fixes (`rhyper()` large-population overflow guard, `seq.int()` non-vector `along.with`, `writeChar()` multibyte counting, `Rd2txt` comment-paragraph parsing, `dummy.coef()` non-syntactic names, wrapper read-only pointer duplication safety)
  - Binary interchange: XDR v2/v3 `.rds` and `.RData` serialization (with gzip/bzip2/xz/zstd compression)
  - Public C API boundary (R 4.6's cleaned-up header contract + R 4.5+ mandatory `R_NO_REMAP` for C++ + experimental resizable vectors API) for loading compiled CRAN packages

## Clean-Room Licensing

- **Roxide (`crates/*`, `tools/*`, `conformance/*`):** Dual-licensed under **`MIT OR Apache-2.0`**.
- **Strict clean-room boundary:** GNU R is `GPL-2.0-or-later` / `GPL-3.0-or-later`. Zero GNU R source files, base `.R` scripts, `nmath` C files, or GNU R test files may be copied or ported from GNU R source. All behavior is implemented cleanly from R manuals, published mathematical/statistical algorithms, permissive Rust crates, and black-box differential oracle testing against official GNU R 4.6.2.

## Planned Repository Layout

```text
Roxide/
├── AGENTS.md                 # Core agent operating instructions & clean-room guardrails
├── README.md                 # This file
├── PROGRESS.md               # Live parity scoreboard (created in Milestone 0)
├── THIRD_PARTY_NOTICES.md    # Permissive crate & algorithm attributions (Milestone 0)
├── Cargo.toml                # Rust workspace manifest (Milestone 0)
├── crates/                   # Disjoint Rust crates (see docs/ARCHITECTURE.md)
├── stdlib/                   # Clean-room Roxide R-level base/stats/utils/methods wrappers
├── conformance/              # Clean-room differential test suites & numerical golden specs
├── tools/                    # Oracle harness, licence scanner, numerical ULP checker, CRAN corpus runner
├── scripts/                  # Mechanical gates & git hooks
└── docs/
    ├── MODEL_ROUTING.md      # Multi-model orchestration & quota policy
    ├── ARCHITECTURE.md       # Workspace crates, contracts, R 4.6.2 traps & layered oracles
    ├── ROADMAP.md            # Milestones M0–M4 & real-world R/CRAN corpus ladder
    ├── KICKOFF_PROMPT.md     # Copy-paste starter prompt for the new thread
    ├── ADR.md                # Architectural Decision Records (created in M0)
    └── PRIOR_ART.md          # Dependency & licence log (created in M0)
```

## Documentation Index

- Start here for agent rules: [`AGENTS.md`](AGENTS.md)
- Model & subscription routing: [`docs/MODEL_ROUTING.md`](docs/MODEL_ROUTING.md)
- Technical architecture & crate map: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- Milestones & kickoff plan: [`docs/ROADMAP.md`](docs/ROADMAP.md)
- Ready-to-paste first prompt: [`docs/KICKOFF_PROMPT.md`](docs/KICKOFF_PROMPT.md)
