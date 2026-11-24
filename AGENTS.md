# Agent Guide for Roxide

Roxide is a strict clean-room, native Rust drop-in replacement for GNU R 4.6.2 (`MIT OR Apache-2.0`). The project works backwards from [`docs/VISION.md`](docs/VISION.md), targeting compiled CRAN and Bioconductor parity, instant binary package installs with a bundled toolchain and pure-Rust BLAS/LAPACK, multi-session compiled-package isolation, first-class Rust (`roxide-r`), Python (`roxide`), C, and WASM embedding, and built-in Jupyter/Positron/LSP/DAP tooling.

## Top 3 priorities and explicit cut list

Always prioritize work in this order (see [`docs/VISION.md`](docs/VISION.md) and ADRs 0021 through 0025 in [`docs/ADR.md`](docs/ADR.md)):

1. **CRAN-wide and Bioconductor pass rate:** Nightly tests against GNU R, reverse-dependency-first fixes, MIT headers from *Writing R Extensions*, common private-API compatibility shims, install-time broken-package warnings, and fast turnaround on new GNU R releases.
2. **Bundled toolchain plus prebuilt binary package repo:** Prebuilt native and `wasm32` CRAN binary repos, `renv.lock` support, bundled C/C++/Fortran toolchain (`zig cc` plus Fortran), and bundled swappable pure-Rust `faer` BLAS/LAPACK with Fortran ABI exports.
3. **Multi-session `RSession` with compiled packages:** Per-session isolated native library copies, per-package `share_safe` flags, and transparent process-pool fallback behind one API, plus Deno-style permissions/quotas, millisecond snapshots, and an in-process `future`/`parallel` backend.

Respect these cuts and architectural realignments:

- **No `.Rc` bytecode roundtripping (ADR-0022):** Unwrap `BCODESXP` `consts[0]` on read for existing `.rdb`/`.rds` compatibility, and keep `compiler` wrappers over Roxide's own IR/JIT. Do not invest in bidirectional `.Rc` bytecode parity.
- **Narrowed PRNG bit-exactness (ADR-0022):** Keep default Mersenne-Twister plus `runif`, `rnorm`, and `sample` bit-exact with GNU R. Target `< 1e-14` relative error across the remaining 22 distributions and alternate RNG/tail paths.
- **Unwind and GC soundness over `unsafe` crate counts (ADR-0023):** Keep `#![forbid(unsafe_code)]` as the default for pure logic crates, and permit audited, ADR-governed `unsafe` in explicit low-level boundary crates (`roxide-capi`, `roxide-libr`, `roxide-blas`, `roxide-arrow`, `roxide-py`, `roxide-wasm`, native JIT). Prioritize rock-solid `Rf_error`/`longjmp`, C++ exception walls, and GC rooting verified under `ROXIDE_GC_TORTURE=1`, ASAN/UBSAN, and fuzzing.
- **Crates.io name (ADR-0021):** `roxide` is already taken on crates.io. Name the public embeddable Rust crate `roxide-r` (CLI binary and PyPI stay `roxide`, JS/WASM uses `@roxide/wasm` / `roxide-wasm`).

## Communication and autonomy preferences

- **Keep talking to the user.** Share what you are doing, what just landed, and what remains as you move through stages and waves. Keep updates concise.
- **Full autonomy:** The user has enabled full access. Do not pause to ask for routine permissions, confirmations, or minor engineering choices. Only stop to ask if blocked by a missing external secret/credential or an irreversible destructive risk.
- **Monitor subagents for permission stalls:** Always launch delegated agents with inherited full-access permissions (never narrow `runtimeMode` such as `approval-required` unless explicitly requested), and keep an eye on running subagents so none stall waiting on a prompt or unanswered question.

## Model routing

See [`docs/MODEL_ROUTING.md`](docs/MODEL_ROUTING.md) for the full routing policy.

Summary for Roxide:
- **Primary orchestrator and planners:** **Gemini 4 Neon** (main thread) and **Opus 5.5** remain the primary planners (architecture, GC/`SEXP`/promise/C-API/multi-session contracts, milestone plans), with **GPT 6.2 Astra** consulted for deep scientific, numerical, and formal feedback. Final decisions always rest with Gemini 4 Neon and Opus 5.5.
- **Go aggressive on OpenAI usage:** Default to OpenAI agents (**GPT 6.2 Sol**, **GPT 6.2 Astra**) for everything they can handle where using Gemini or Anthropic would not make a meaningful difference:
  - **GPT 6.2 Sol (`codex`, `high`)** is the primary high-throughput builder, code/work reviewer, differential triage runner, and clean-room conformance author. Run it aggressively in parallel across disjoint crates and submodules.
  - **GPT 6.2 Astra** is the behind-the-scenes deep-reasoning and scientific/numerical advisor (IEEE 754 `NA_real_` 1954 propagation, lazy promise/NSE scoping, S3/S4 dispatch, distribution accuracy `< 1e-14`, `faer`/LAPACK/GLM stability, GC root safety across unwind and C API). Never expose Astra directly to the user; the main agent evaluates its feedback and retains final decision authority.
- **Never use `max` effort on Anthropic or OpenAI agents:** Cap effort at `high` (or lower where appropriate).
- **Normal/measured Gemini 4 Neon and Opus 5.5 usage with mandatory check-ins:** Keep Anthropic usage normal and measured across Claude Ultra, Cursor Pro, and Gemini plan allowances. Always perform check-ins on OpenAI models' work (verifying diffs, architecture alignment, clean-room compliance, and GNU R 4.6.2 oracle results).
- **Disagreement resolution:** Never blindly agree with any model. Verify disputes empirically against the codebase and live GNU R 4.6.2 black-box oracle probes (`Rscript --vanilla`), reason disagreements out to reach agreement, and if deadlocked, Gemini 4 Neon and Opus 5.5 retain the final decision.
- **Last resort only:** Never use **Fable 5.2** unless the user explicitly asks for it.
- **Exceptions:** Skip model routing when the user explicitly says not to, or when the task is menial.

## Hardware, storage and build discipline

This workstation is a **16 GB RAM macOS arm64** machine.

- **16 GB RAM concurrency limits (always enforce):**
  - **At most 2 concurrent Cargo build/test slots** across the entire agent fleet (`CARGO_BUILD_JOBS=2`, `RUST_TEST_THREADS=3`). Use `scripts/cargo_slot.sh`.
  - Keep read-only workers (architecture blueprints, oracle diff triage, numerical analysis, spec audits, review passes, Python tooling) running in parallel without holding a Cargo slot.
  - Cap runaway scripts, infinite R recursion, or huge vector allocations with explicit timeouts and memory limits.
- Always source `env/env.sh` before running `cargo`, tests, or oracle commands.

## Strict clean-room and licence rules

- **Roxide is licensed under `MIT OR Apache-2.0`. GNU R 4.6.2 is `GPL-2.0-or-later` / `GPL-3.0-or-later`** (with public C headers under `LGPL-2.1-or-later`).
- **STRICT CLEAN-ROOM WALL:**
  - **Never** read, translate, or copy GNU R source code (`src/main/*.c`, `src/nmath/*.c`, `src/library/*/R/*.R`, `src/library/*/src/*`, `src/include/*`), GNU R's test suite (`tests/`), or GPL-licensed CRAN package source code into this repository.
  - Reimplement all language behavior, libraries, headers, compatibility shims, and numerical routines cleanly from:
    1. Official R manuals (*R Language Definition*, *Writing R Extensions*, *R Internals*, *R Data Import/Export*, official R `NEWS`).
    2. Published mathematical, statistical, and numerical-analysis literature.
    3. Permissively licensed (`MIT` / `Apache-2.0` / `BSD` / `Zlib` / `ISC`) Rust crates and libraries (e.g., `faer`).
    4. Black-box differential oracle output from the installed official GNU R 4.6.2 binary (`$ROXIDE_ORACLE_R`, `$ROXIDE_ORACLE_RSCRIPT`).
- **Roxide never invokes GNU R at runtime.** Only `crates/roxide-harness`, `tools/`, and differential tests invoke the oracle (`$ROXIDE_ORACLE_RSCRIPT`).
- Before adopting any external crate or third-party algorithm/data: verify its licence is compatible with `MIT OR Apache-2.0`, record it in `docs/PRIOR_ART.md`, add a short ADR in `docs/ADR.md`, and add attribution in `THIRD_PARTY_NOTICES.md` + `third_party/licences/`.
- Run `tools/measure/licence_scan.py` (via `scripts/gate.sh`) before committing new code or data.

## Pstack and poteto mode

Apply the full installed `pstack` skill suite (`poteto-mode`, `deslop`, `no-comments`, `unslop`, `architect`, `how`, `why`, `interrogate`, `thermo-nuclear-code-quality-review`, `figure-it-out`, and the `principle-*` skills) across Antigravity, Codex, and Claude Code for every code change, refactor, review, and reply.

- **Model routing takes precedence over `pstack`'s default Models table.** Never route to Fable 5.2, never use `max` effort, and never enable Codex fast mode.
- **Required discipline on every code change:** name the data shape first (`principle-model-the-domain`), validate at boundaries and trust internal types (`principle-boundary-discipline`, `principle-type-system-discipline`), subtract dead weight before adding (`principle-subtract-before-you-add`, `principle-laziness-protocol`), minimize reader load (`principle-minimize-reader-load`), pin behavior before refactoring and verify on real artifacts (`principle-prove-it-works`, `principle-test-behavior-not-implementation`, `principle-sequence-verifiable-units`), and run `deslop` and `no-comments` checks before committing.
- **Writing discipline:** follow `unslop` on all replies, docs, and commit messages (short declarative sentences, no em-dash characters, no mid-sentence colons, concrete evidence on every claim, comments only for non-obvious why).

## Engineering standards

- **Isolated `unsafe` discipline (ADR-0023):** Every pure-Rust logic crate (`roxide-diag`, `roxide-syntax`, `roxide-host`, `roxide-gc`, `roxide-value`, `roxide-eval`, `roxide-base`, `roxide-math`, `roxide-stats`, `roxide-methods`, `roxide-utils`, `roxide-pkg`, `roxide-graphics`, `roxide-cli`, `roxide-harness`) enforces `#![forbid(unsafe_code)]`. Audited, ADR-governed `unsafe` is permitted in explicit low-level boundary crates (`roxide-capi`, `roxide-libr`, and future `roxide-blas`, `roxide-arrow`, `roxide-py`, `roxide-wasm`, or native JIT boundaries), prioritized for rock-solid C/C++ unwinding and GC rooting.
- **Treat all inputs as untrusted:** No `unwrap()`, `expect()`, `panic!()`, `todo!()`, or `unimplemented!()` on reachable input paths. Enforce expression/call-stack depth, vector-size, archive/decompression, and Deno-style `Host` permission/quota limits.
- **Abstract OS access via `Host`:** Core evaluation, connections, filesystem helpers, networking, process spawning, clock, and RNG seeding go through `roxide-host::Host`, enabling deterministic `PureHost` tests, WASM builds, and per-session sandboxing.
- **General rules over test hacks:** Implement the true R semantic or numerical rule. Never hardcode single-test special cases.
- **Quality gates:** Every public item documented; `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`, full Rust tests, licence scan, and layered differential oracle suites must pass before landing waves.
- **GitHub and commit hygiene:** Remote repository is `https://github.com/dvgyl/roxide` (`origin`). Commit and push to `origin/main` regularly after every verified wave, passing milestone slice, or meaningful checkpoint. Use conventional commits. Never include `Co-Authored-By:`, `Claude-Session:`, or `"Generated with"` lines. Never force-push or rewrite published history.

## Parallel wave coordination

- The coordinator (main thread) owns root `Cargo.toml`, shared contract crates (`roxide-diag`, `roxide-syntax`, `roxide-host`, `roxide-gc`, `roxide-value` core traits/types, `roxide-eval` hook traits), `scripts/`, `PROGRESS.md`, `docs/VISION.md`, and `docs/ADR.md`. Workers must not edit shared contracts unasked.
- Split implementation waves across disjoint crates or disjoint submodules so 3 to 5 GPT 6.2 Sol workers run concurrently with zero merge conflicts.
- **Cross-model review discipline:** Review major GPT 6.2 Sol architectural, FFI, or unwinding slices with Opus 5.5 (and review every Opus 5.5 slice with GPT 6.2 Sol). For routine GPT 6.2 Sol build waves, pair a fresh GPT 6.2 Sol reviewer with the differential oracle gate, escalating subtle semantics or numerical stability questions to GPT 6.2 Astra and Opus 5.5.

## Navigation

- [`docs/VISION.md`](docs/VISION.md). Working-backwards target Show HN announcement, Top 3 priorities, and cut list.
- [`docs/HANDOFF_PROMPT.md`](docs/HANDOFF_PROMPT.md). Short continuation handoff prompt for new threads.
- [`README.md`](README.md). Project overview, SDK names, and repository layout.
- [`docs/MODEL_ROUTING.md`](docs/MODEL_ROUTING.md). Full model routing and subscription policy.
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md). Crate map, shared contracts, invariants, and 5-layer oracle.
- [`docs/ROADMAP.md`](docs/ROADMAP.md). Milestones M0 through M8 working backwards from `docs/VISION.md`.
- [`docs/ADR.md`](docs/ADR.md). Architectural Decision Records (ADR-0001 through ADR-0025).
- [`PROGRESS.md`](PROGRESS.md). Live parity scoreboard and active wave checkpoint.
