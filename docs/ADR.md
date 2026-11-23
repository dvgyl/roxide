# Architectural Decision Records (ADRs)

All major architectural contracts and deviations from `docs/ARCHITECTURE.md` are recorded here by the primary planners (**Gemini 4 Neon** and **Opus 5.5**, informed by formal-semantics verification and live **GNU R 4.6.2** black-box oracle probes).

---

## ADR-0001: Black-Box GNU R 4.6.2 Binary Oracle & Strict Clean-Room Wall

- **Status:** Accepted
- **Context:** Roxide is dual-licensed `MIT OR Apache-2.0`, while GNU R 4.6.2 is `GPL-2.0-or-later` / `GPL-3.0-or-later`.
- **Decision:**
  - Install the signed, notarized CRAN macOS arm64 binary (`R-4.6.2-arm64.pkg`) solely into gitignored `env/oracle/` (`$ROXIDE_ORACLE_R`, `$ROXIDE_ORACLE_RSCRIPT`) for black-box differential verification in `roxide-harness` and `tools/`.
  - Never download, inspect, copy, or translate GNU R source trees, base `.R` scripts, `nmath` C code, or GNU R's test suite.
  - Enforce `#![forbid(unsafe_code)]` across every pure-Rust crate (`roxide-diag`, `roxide-host`, `roxide-gc`, `roxide-syntax`, `roxide-value`, `roxide-eval`, `roxide-base`, `roxide-math`, `roxide-stats`, `roxide-methods`, `roxide-utils`, `roxide-graphics`, `roxide-cli`, `roxide-harness`), permitting audited `unsafe` only in `roxide-capi` (Milestone 4).
  - Run `tools/measure/licence_scan.py` in every `scripts/gate.sh` run.

---

## ADR-0002: Unified `SexpId` Handle, Safepoint-Only Cycle-Safe GC & M4 C-API Layout

- **Status:** Accepted
- **Context:** R closures capture environments containing those closures, and promises form cycles with environments; meanwhile Milestone 4 requires `roxide-capi` to expose opaque `SEXP` pointers with `PROTECT`/`UNPROTECT` and stable `REAL()`/`INTEGER()`/`STRING_PTR_RO()` buffers without rewriting the engine.
- **Decision:**
  - Use a single canonical handle type `SexpId(NonZeroU64)` across all crates (no `GcRef` alias), bit layout `[63..36] generation (28 bits) | [35..4] slot_index + 1 (32 bits) | [3..0] zero`. Zero is never valid (`Option<SexpId>` = 8 bytes; C `NULL` != any `SEXP`). Permanent singletons occupy fixed indices at generation 0 (`NIL=0`, `UNBOUND=1`, `MISSING=2`, `NA_CHAR=3`, `BLANK_CHAR=4`, `EMPTY_ENV=5`, `BASE_ENV=6`, `GLOBAL_ENV=7`, `BASE_NAMESPACE=8`, `TRUE_SCALAR=9`, `FALSE_SCALAR=10`, `NA_LGL_SCALAR=11`).
  - **Safepoint-only collection:** `Heap::alloc` never triggers GC; collection runs only at explicit `Ctx::safepoint()` boundaries (`eval`, `apply`, `force`, `signal`, loop back-edges, M4 C allocators). Marking is iterative (explicit gray stack) and two-phase (`mark -> Marked::sweep`) so `RSession` purges the weak `CHARSXP` interning table between mark and sweep.
  - **Stable buffers:** Vector payloads use fixed-capacity `Box<[T]>` (`RVec<T>`) that never reallocate, keeping `SexpId` and payload addresses stable for M4 `roxide-capi`.
  - **GC Torture gate:** `GcPolicy::Torture` (`ROXIDE_GC_TORTURE=1`) collects at every safepoint to detect missing roots.

---

## ADR-0003: Saturating Copy-on-Modify Refcounting & R 4.6.2 Wrapper Duplication Guard

- **Status:** Accepted
- **Context:** GNU R 4.x replaced the 2-bit `NAMED` heuristic with saturating reference counting, and GNU R 4.6.2 fixed an ALTREP wrapper bug where read-only pointer access on a wrapper could alias a shared underlying vector.
- **Decision:**
  - Track a saturating `refcnt: u8` (`REFCNT_MAX = 7`, sticky once saturated; permanent objects always saturated) on every `Heap` slot. Increment when storing a handle into a heap field (vector element, cons cell, attribute, env binding, promise value, closure field); decrement on overwrite/removal. Rust locals and the protect stack do not alter `refcnt`.
  - Invariant: `refcnt` may conservatively overestimate sharing, never underestimate it (`maybe_shared` iff `refcnt > 1`).
  - For ALTREP wrapper objects, duplicate a possibly shared wrapped backing object before exposing either a writable slice OR a read-only raw pointer (R 4.6.2 safety guard).

---

## ADR-0004: Crate Dependency DAG & Module Placement Refinements

- **Status:** Accepted (approved refinements over initial `docs/ARCHITECTURE.md` draft)
- **Context:** Keeping `roxide-gc` generic and cleanly separating syntax/deparsing/printing avoids circular dependencies and unlocks parallel `roxide-base` / `roxide-stats` development.
- **Decision:**
  - Final acyclic dependency DAG:
    - `roxide-diag` (leaf)
    - `roxide-host` -> `roxide-diag`
    - `roxide-gc` -> `roxide-diag`
    - `roxide-syntax` -> `roxide-diag`
    - `roxide-value` -> `roxide-diag`, `roxide-gc`, `roxide-syntax`
    - `roxide-eval` -> `roxide-diag`, `roxide-host`, `roxide-gc`, `roxide-syntax`, `roxide-value`
    - `roxide-base`, `roxide-math`, `roxide-stats`, `roxide-methods`, `roxide-utils`, `roxide-graphics` -> `roxide-eval` (+ lower layers)
    - `roxide-harness` -> standalone (depends on zero engine crates)
  - `ALTREP` traits (`AltClass`, `AltVector<T>`, `AltString`, `CompactIntSeq`, `CompactRealSeq`, `AltRegistry`) live in `roxide-value::alt` so `roxide-gc` remains generic.
  - Pure number/string formatting (`roxide-value::format`) and `deparse()` (`roxide-value::deparse`, reusing `roxide-syntax::ops` precedence) live in `roxide-value`; S3-dispatched `print()` lives in `roxide-base::print` (with a default core printer available from M0).
  - M1 `roxide-eval` implements an AST/SEXP evaluator behind the stable `Ctx::eval` boundary; the M4 bytecode VM plugs behind `Ctx::eval` with zero worker API changes.

---

## ADR-0005: Exact IEEE 754 `NA_real_` (1954 Payload), `NA_integer_` & Kleene Logic

- **Status:** Accepted (empirically verified against live GNU R 4.6.2 arm64 oracle)
- **Context:** Live black-box probes on GNU R 4.6.2 arm64 confirmed fresh `NA_real_` is `0x7FF0_0000_0000_07A2` (low 32 bits `1954` = `0x07a2`, quiet bit clear) and quieted `NA_real_` after arithmetic is `0x7FF8_0000_0000_07A2`.
- **Decision:**
  - Define `NA_REAL_BITS: u64 = 0x7FF0_0000_0000_07A2` and classify via `is_na_real(x) = x.is_nan() && (x.to_bits() as u32) == 1954`, `is_nan_not_na(x) = x.is_nan() && (x.to_bits() as u32) != 1954`.
  - Preserve all 64 bits across storage, copying, and XDR serialization; default `identical()` (`single.NA = TRUE`) equates all `NA_real_` payloads and separates them from `NaN`, while `single.NA = FALSE` compares raw bits.
  - Apply power identities `x ^ 0 == 1` and `1 ^ y == 1` before missing-value propagation. On binary NaN slow paths, apply deterministic `nan_propagate(a, b)` matching arm64 GNU R 4.6.2.
  - `NA_INTEGER = i32::MIN`; integer `+`, `-`, `*` compute in `i64` and return `(NA_INTEGER, overflow: true)` when out of `[-2147483647, 2147483647]`, emitting `"NAs produced by integer overflow"`.
  - `RLogical(i32)` implements Kleene 3-valued logic (`FALSE & NA == FALSE`, `TRUE | NA == TRUE`). `CharId::NA` is tested by handle identity, never byte equality with `"NA"`.

---

## ADR-0006: Four-State Lazy Promises (`PROMSXP`), NSE & Frame Unwinding

- **Status:** Accepted (empirically verified against live GNU R 4.6.2 oracle)
- **Context:** Live GNU R 4.6.2 probes confirmed both recursive default promise detection and interrupted promise restart warnings.
- **Decision:**
  - `PromiseData` carries `expr: SexpId`, `env: SexpId`, `value: SexpId`, and a 4-state `PromiseState { Unforced, Evaluating, Interrupted, Forced }`:
    - `Unforced -> Evaluating`: evaluate `expr` in `env`.
    - `Evaluating` re-entry: error `"promise already under evaluation: recursive default argument reference or earlier problems?"`.
    - Normal completion -> `Forced` (`env = NIL`, `value` cached, `expr` kept for `substitute()`).
    - Unwind/error during evaluation -> `Interrupted` (retain `expr` and `env`); a subsequent force emits warning `"restarting interrupted promise evaluation"` and retries.
  - Track missingness explicitly on formal bindings (`Binding::missing`) separate from `SexpId::MISSING`, `SexpId::UNBOUND`, and `SexpId::NIL`.
  - All non-local transfers (`return`, `break`, `next`, exiting condition handlers, `invokeRestart`, `q()`) use typed `Result<T, Unwind>` in safe Rust; Rust never `longjmp`s (M4 C shim isolates any foreign `longjmp` at the `.Call` trampoline).

---

## ADR-0007: Parallel Worker Boundary (`Ctx`, `ValueApi`, `Module`)

- **Status:** Accepted
- **Context:** Multiple **GPT 6.2 Sol** workers will build disjoint `roxide-base`, `roxide-math`, and `roxide-stats` submodules concurrently.
- **Decision:**
  - `RObject` internals stay `pub(crate)` inside `roxide-value`; workers access objects solely through `ValueApi`, `VecView<T>`, and `Ctx` methods.
  - Each standard library submodule exports a `pub const MODULE: Module` (`prims: &'static [PrimDef]`, `prelude: &'static str`) and uses `ctx.ext::<MyState>()` for per-module GC-traced session state without editing coordinator-owned `Ctx` or `roxide-value` structs.
  - `roxide-eval` owns S3 dispatch (`UseMethod`, `NextMethod`, group generics) and plugs S4 dispatch in via `S4Hooks` in Milestone 3.

---

## ADR-0008: Permissive Regex & Compression Dependencies for `roxide-base`

- **Status:** Accepted
- **Context:** Milestone 1 requires POSIX ERE / PCRE2-compatible regex matching (`grep`/`grepv`/`sub`/`gsub`/`regexpr`/`gregexpr`/`strsplit`) and gzip/bzip2/xz/zstd stream support (`gzfile`/`bzfile`/`xzfile`, `readRDS`/`saveRDS`, `load`/`save`, `extSoftVersion()[["zstd"]]`), while strictly avoiding any GPL/LGPL/AGPL dependencies and keeping `#![forbid(unsafe_code)]` in `roxide-base`.
- **Decision:**
  - Adopt `regex` (`MIT OR Apache-2.0`) and `fancy-regex` (`MIT`) for pure-Rust regex matching, lookaround (`perl = TRUE`), and backreferences (`\1`, `\2`).
  - Adopt `flate2` with `default-features = false, features = ["miniz_oxide"]` (`MIT OR Apache-2.0` / `Zlib`) for pure-Rust gzip (`GzEncoder` / `MultiGzDecoder`).
  - Adopt `bzip2` (`MIT OR Apache-2.0`, default pure-Rust `libbz2-rs-sys` under permissive `bzip2-1.0.6`) for bzip2 (`BzEncoder` / `MultiBzDecoder`).
  - Adopt `lzma-rust2` (`Apache-2.0`, `default-features = false, features = ["std", "encoder", "xz"]`) for pure-Rust `.xz` streams — explicitly rejecting `xz2`/`lzma-sys` because its bundled XZ source archive contains GPL/LGPL components.
  - Adopt `zstd` (`BSD-3-Clause`, `default-features = false, features = ["vendored"]`, `zstd-sys` `BSD-3-Clause AND BSD-2-Clause` bundling libzstd 1.5.7 under BSD terms) for Zstandard streams and exact GNU R 4.6.2 `extSoftVersion()[["zstd"]] == "1.5.7"` parity.
  - Enforce zero copyleft dependencies in `Cargo.lock` and preserve `#![forbid(unsafe_code)]` across all `roxide-base` code.

