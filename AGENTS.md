# Agent Guide for Roxide

**Roxide** is a strict clean-room, native Rust drop-in replacement for **GNU R 4.6.2** (`MIT OR Apache-2.0`), targeting drop-in parity for `R`, `Rscript`, `R CMD`, core base/stats/utils/methods/tools/graphics/grid packages, `.rds`/`.RData` serialization, numerical/PRNG reproducibility, and R 4.6.2's cleaned-up public C API for compiled CRAN packages.

## Communication & autonomy preferences

- **Keep talking to the user.** Regularly share what you are currently doing, what just landed, and what work remains — never go silent during long multi-step tasks. You do not need to narrate every single tool call; give clear, concise updates as you move through stages and waves.
- **Full autonomy — do NOT ask for routine permissions or confirmations:** The user has enabled **full access**. Do **not** pause to ask the user for approval, confirmation, or minor engineering choices — make sound decisions yourself and keep driving the project forward continuously. Only stop to ask if genuinely blocked by a missing external secret/credential or an irreversible destructive risk.
- **Monitor subagents for permission stalls:** Always launch delegated agents with inherited full-access permissions (never narrow `runtimeMode` such as `approval-required` unless explicitly requested), and keep an eye on running subagents so none stall waiting on a permission prompt or unanswered question.

## Model routing

See [`docs/MODEL_ROUTING.md`](docs/MODEL_ROUTING.md) for the full routing policy.

Summary for **Roxide**:
- **Primary orchestrator & planners:** **Gemini 4 Neon** (main thread) and **Opus 5.5** remain the **primary planners** (architecture, GC/`SEXP`/promise/C-API contracts, milestone plans), with **GPT 6.2 Astra** consulted for deep scientific/numerical/formal feedback. Final decisions always rest with **Gemini 4 Neon** and **Opus 5.5**.
- **Go really aggressive on OpenAI usage:** Default to **OpenAI agents** (`GPT 6.2 Sol`, `GPT 6.2 Astra`) for **everything that can be done on them where using Gemini or Anthropic wouldn't make a meaningful difference**:
  - **GPT 6.2 Sol (`codex`, `high`)** is the primary high-throughput builder, code/work reviewer, differential triage runner, and clean-room conformance author. Run it **extensively and aggressively** in parallel across disjoint crates and `roxide-base` / `roxide-stats` / `roxide-math` submodules.
  - **GPT 6.2 Astra** is the behind-the-scenes deep-reasoning & scientific/numerical feedback advisor — lean on it hard for formal R semantics and numerical math (IEEE 754 `NA_real_` 1954 payload propagation, lazy promise/NSE scoping, S3/S4 multiple dispatch, exact `d/p/q/r*` distribution tail accuracy, QR/SVD/GLM/optimization stability, PRNG bit-exactness, GC root-safety across conditions & C API). Never expose Astra directly to the user; the main agent evaluates its feedback and retains final decision authority.
- **Never use `max` effort on Anthropic or OpenAI agents:** Do **not** use Max effort (`effort: "max"` / `reasoningEffort: "max"`) on either Anthropic or OpenAI delegated agents; cap effort at `high` (or lower where appropriate).
- **Not a ban on Gemini 4 Neon / Opus 5.5 — Coordinator decides & always checks in:** Keep Anthropic usage **normal/measured** (drawing across Claude Ultra, Cursor Pro, and Gemini plan allowances). **Gemini 4 Neon** in the active chat decides how and when to invoke Gemini 4 Neon or Opus 5.5 when they genuinely add value, and **always performs check-ins on OpenAI models' work** (verifying diffs, architecture alignment, clean-room compliance, and GNU R 4.6.2 oracle behavior rather than blindly accepting outputs).
- **Disagreement resolution:** Never blindly agree with any model (Opus 5.5, GPT 6.2 Sol, or GPT 6.2 Astra). Verify disputes empirically against the codebase and live **GNU R 4.6.2** black-box oracle probes (`Rscript --vanilla`), reason disagreements out to reach agreement, and if deadlocked, **Gemini 4 Neon** and **Opus 5.5** retain the final decision.
- **Last resort only:** Never use **Fable 5.2** unless the user explicitly asks for it.
- **Exceptions:** Skip model routing when the user explicitly says not to, or when the task is menial (e.g., a tiny edit or one-time folder move).

## Hardware, storage & build discipline

This workstation is a **16 GB RAM macOS arm64** machine.

- **Recommended location:** Running this project on the internal **APFS** SSD (typically **10–25 GB** total including Rust `target/`, GNU R 4.6.2 oracle, and CRAN test corpus) gives fast incremental builds, CoW copies, and zero small-file cluster waste.
- **If kept on `/Volumes/Extreme Pro` (exFAT):** Every file consumes at least **1 MB** (2 MB with macOS `._*` AppleDouble files). Keep `CARGO_INCREMENTAL=0`, `COPYFILE_DISABLE=1`, group test fixtures into compact files, ignore `._*` in all directory walks, clean `.git` companion files after `git fetch`/`git gc` (`find .git -name '._*' -delete`), and stage temporary package builds/tests on `/tmp` or inside an APFS `.sparsebundle`.
- **16 GB RAM concurrency limits (always enforce):**
  - **At most 2 concurrent Cargo build/test slots** across the entire agent fleet (`CARGO_BUILD_JOBS=2`, `RUST_TEST_THREADS=3`).
  - Keep read-only workers (architecture blueprints, oracle diff triage, numerical analysis, spec audits, review passes, Python tooling) running in parallel without holding a Cargo slot so the fleet stays saturated without swapping or OOMing.
  - Always cap runaway scripts, infinite R recursion, or huge vector allocations with explicit timeouts and memory limits.
- Once `env/env.sh` is created in Milestone 0, always source it before running `cargo`, tests, or oracle commands.

## Strict clean-room & licence rules

- **Roxide is licensed under `MIT OR Apache-2.0`. GNU R 4.6.2 is `GPL-2.0-or-later` / `GPL-3.0-or-later`** (with public C headers under `LGPL-2.1-or-later`).
- **STRICT CLEAN-ROOM WALL — NEVER VIOLATE:**
  - **Never** read, translate, or copy GNU R source code (`src/main/*.c`, `src/nmath/*.c`, `src/library/*/R/*.R`, `src/library/*/src/*`), GNU R's test suite (`tests/`), or GPL-licensed CRAN package source code into this repository.
  - Reimplement all language behavior, base/stats/utils/methods/graphics libraries, and numerical routines cleanly from:
    1. Official R manuals (*R Language Definition*, *Writing R Extensions*, *R Internals*, *R Data Import/Export*, official R 4.6.2 `NEWS`)
    2. Published mathematical, statistical, and numerical-analysis literature (IEEE 754, MT19937, Kinderman-Ramage, Ahrens-Dieter, Householder QR, BFGS/L-BFGS-B, Nelder-Mead, Brent, etc.)
    3. Permissively licensed (`MIT` / `Apache-2.0` / `BSD` / `Zlib` / `ISC`) Rust crates and libraries
    4. **Black-box differential oracle output** from the installed official **GNU R 4.6.2** binary (`$ROXIDE_ORACLE_R`, `$ROXIDE_ORACLE_RSCRIPT`)
- **Roxide never invokes GNU R at runtime.** Only `crates/roxide-harness`, `tools/`, and differential tests invoke the oracle (`$ROXIDE_ORACLE_RSCRIPT`).
- Before adopting any external crate or third-party algorithm/data: verify its licence is compatible with `MIT OR Apache-2.0`, record it in `docs/PRIOR_ART.md`, add a short ADR in `docs/ADR.md`, and add attribution in `THIRD_PARTY_NOTICES.md` + `third_party/licences/`.
- Maintain an automated licence scanner (`tools/measure/licence_scan.py`) and run it before committing new code or data.

## Engineering standards

- **Isolated `unsafe` discipline:** Every pure-Rust crate (`roxide-diag`, `roxide-syntax`, `roxide-host`, `roxide-value`, `roxide-eval`, `roxide-base`, `roxide-math`, `roxide-stats`, `roxide-methods`, `roxide-utils`, `roxide-graphics`, `roxide-cli`) MUST enforce `#![forbid(unsafe_code)]`. Audited `unsafe` is permitted **only** in the explicitly isolated low-level boundary crates (`roxide-capi` for the `libR` C ABI / dynamic library loading, and optionally a tiny audited GC/BLAS FFI module governed by ADRs).
- **Treat all inputs as untrusted:** No `unwrap()`, `expect()`, `panic!()`, `todo!()`, or `unimplemented!()` on reachable input paths (R source, `.rds`/`.RData` streams, CSV/DCF files, regexes, CLI args). Enforce expression/call-stack depth (`expressions` option), vector-size, and archive/decompression limits.
- **Abstract OS access via `Host` / `Context`:** Core evaluation, I/O connections, filesystem helpers, clock, and RNG seeding must go through `roxide-host::Host` rather than calling `std::fs` / `std::env` / `std::process` directly, enabling deterministic `PureHost` tests alongside `NativeHost`.
- **General rules over test hacks:** Implement the true R 4.6.2 semantic or numerical rule — never hardcode single-test special cases.
- **Quality gates:** Every public item documented; `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`, full Rust tests, licence scan, and layered differential oracle suites must pass before landing waves.
- **GitHub & commit hygiene:** Remote repository is **`https://github.com/dvgyl/roxide`** (`origin`). **Commit and push to GitHub (`origin/main`) regularly** after every verified wave, passing milestone slice, or meaningful checkpoint — do not accumulate hours of unpushed work. Use conventional commits (e.g., `feat(eval): implement lazy promise forcing and substitute`). Never include `Co-Authored-By:`, `Claude-Session:`, or `"Generated with"` lines (install a `commit-msg` hook in `init.sh`). Never force-push or rewrite published history.

## Parallel wave coordination

- The **coordinator** (main thread) owns root `Cargo.toml`, shared contract crates (`roxide-diag`, `roxide-syntax`, `roxide-host`, `roxide-gc`, `roxide-value` core traits/types), `scripts/`, `PROGRESS.md`, and `docs/ADR.md`. Workers must not edit shared contracts unasked.
- Split implementation waves across **disjoint crates or disjoint submodules** (`roxide-base/src/<family>/`, `roxide-math/src/<family>/`, `roxide-stats/src/<family>/`) so 3–5 GPT 6.2 Sol workers run concurrently with zero merge conflicts.
- **Cross-model review discipline:** Review major GPT 6.2 Sol architectural/core slices with Opus 5.5 (and review every Opus 5.5 slice with GPT 6.2 Sol). For routine GPT 6.2 Sol build waves, pair a fresh GPT 6.2 Sol reviewer + differential oracle gate, escalating subtle language semantics or numerical stability questions to GPT 6.2 Astra / Opus 5.5.

## Navigation

- [`README.md`](README.md) — Project overview, GNU R 4.6.2 target features, and repository layout
- [`docs/MODEL_ROUTING.md`](docs/MODEL_ROUTING.md) — Full model routing and subscription policy
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — Crate breakdown, shared contracts, R 4.6.2 invariants, and layered oracle design
- [`docs/ROADMAP.md`](docs/ROADMAP.md) — Phased milestones (M0–M4), numerical/CRAN corpus ladder, and Day-1 setup
- [`docs/KICKOFF_PROMPT.md`](docs/KICKOFF_PROMPT.md) — Ready-to-use kickoff prompt for starting the new project thread
