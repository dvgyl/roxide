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
  - Final acyclic dependency DAG (amended by ADR-0009 so `roxide-math` is a pure zero-dependency leaf crate):
    - `roxide-diag` (leaf)
    - `roxide-math` (leaf — zero Roxide deps, zero external deps, `#![forbid(unsafe_code)]`; amended per ADR-0009)
    - `roxide-host` -> `roxide-diag`
    - `roxide-gc` -> `roxide-diag`
    - `roxide-syntax` -> `roxide-diag`
    - `roxide-value` -> `roxide-diag`, `roxide-gc`, `roxide-syntax`
    - `roxide-eval` -> `roxide-diag`, `roxide-host`, `roxide-gc`, `roxide-syntax`, `roxide-value`
    - `roxide-base` -> `roxide-eval`, `roxide-math` (+ lower layers)
    - `roxide-stats` -> `roxide-eval`, `roxide-base`, `roxide-math` (+ lower layers)
    - `roxide-utils` -> `roxide-eval`, `roxide-host`, `roxide-base` (+ lower layers; no `roxide-math`/`roxide-stats` dep)
    - `roxide-methods`, `roxide-graphics` -> `roxide-eval` (+ lower layers)
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

---

## ADR-0009: Pure Leaf `roxide-math`, Arm64 FMA Parity, BSD Netlib LAPACK/BLAS & Paper-Only Licence Map

- **Status:** Accepted (amends ADR-0004 and `docs/MATH_SPEC.md` §3)
- **Context:** Milestone 2 requires bit-exact PRNG reproducibility, high-accuracy `d/p/q/r*` distributions, reference BLAS/LAPACK sign and rounding parity (empirically verified against GNU R 4.6.2 arm64 using reference `libRblas`/`libRlapack` 3.12.1 with FMA contraction), mixed-radix FFT, and numerical optimization/quadrature without introducing copyleft code or slow crate rebuild loops.
- **Decision:**
  - **`roxide-math` is a pure leaf crate** (`#![forbid(unsafe_code)]`, zero Roxide dependencies, zero external crates). Kernels operate strictly on `f64`/`i32`/`u32` and slices (`MatRef`/`MatMut`/`Cplx`/`Tail`/`Flags`), keeping math builds fast, eliminating GC rooting hazards inside math kernels, and enabling direct `Rmath.h` reuse by `roxide-capi` in Milestone 4.
  - **Zero new external crates for M2:** Use Rust `std` floating-point math (`exp`, `ln`, `powf`, `ln_1p`, `exp_m1`, `hypot`) rather than `libm` so macOS calls the same Apple libm as GNU R 4.6.2. Implement `sinpi`, `cospi`, `tanpi`, `log1pmx`, and `lgamma1p` directly.
  - **Arm64 FMA parity:** Use `f64::mul_add` in BLAS, LINPACK, LAPACK, and empirically verified polynomial loops where GNU R 4.6.2 contracts multiply-add on arm64.
  - **Clean-room algorithm sourcing & licence map:**
    - Reference Netlib BLAS & LAPACK 3.12.1 (`BSD-3-Clause`, upstream Netlib only — never GNU R's bundled copy; attributed in `docs/PRIOR_ART.md` & `THIRD_PARTY_NOTICES.md`) for faithful decomposition and `svd`/`eigen` sign conventions.
    - QUADPACK (`Public Domain`) and Knuth `ran_array` (`Public Domain`) are clean-room permissible.
    - Pure-Rust Singleton (1969) mixed-radix FFT (radices 2, 3, 4, 5 + odd-prime butterfly, optional Bluestein fallback for large primes), amending `docs/MATH_SPEC.md` §3.
    - **Paper-only rule:** ACM-licensed or Applied Statistics algorithms (`TOMS 708` / DiDonato & Morris 1992 `pbeta`, `TOMS 643` / Mehta & Patel `fisher.test`, `TOMS 778` / Byrd-Lu-Nocedal-Zhu 1995 `L-BFGS-B`, `AS 241` / Wichura 1988 `qnorm`, `AS 136` / Hartigan-Wong 1979 `kmeans`, `AS R94` / Royston 1995 `shapiro.test`) MUST be implemented strictly from the published mathematical papers, never from ACM/GPL code.

---

## ADR-0010: `.GlobalEnv$.Random.seed` Single-Source-of-Truth Contract

- **Status:** Accepted (empirically verified against live GNU R 4.6.2 oracle)
- **Context:** R user code inspects, saves (`old <- .Random.seed`), mutates, restores (`.Random.seed <<- old`), and deletes (`rm(.Random.seed)`) `.Random.seed` directly in `.GlobalEnv`, while local `.Random.seed` bindings inside functions have no effect.
- **Decision:**
  - `.GlobalEnv$.Random.seed` is the sole authoritative RNG state, following R's documented `GetRNGstate`/`PutRNGstate` model: read from `env_get(GLOBAL_ENV, ".Random.seed")` at RNG entry, operate on a pure-Rust `roxide_math::rng::Rng` with no GC safepoints during draws, and write back to `GLOBAL_ENV` on exit.
  - `RngExt` (`CtxExt` in `roxide-base::random`) stores only the non-serialized Box–Muller spare (`bm_saved: Option<f64>`) and holds **zero `SexpId`s**.
  - **Copy-on-modify invariant:** `put_rng` overwrites an existing `INTSXP` binding in place **only** when it is unshared (`!maybe_shared`) and matches the exact encoded length; otherwise it allocates a fresh `INTSXP` so saved copies (`old <- .Random.seed`) are never mutated.
  - Missing, non-integer, NA-kind, or invalid-kind `.Random.seed` values emit GNU R 4.6.2's exact warnings and reseed via `Host` (`now`/`pid`, deterministic on `PureHost`); valid-kind wrong-length vectors raise an R error (`'.Random.seed' has wrong length`), while `set.seed()` reads only kind codes and ignores wrong length.

---

## ADR-0011: Package Namespaces (`Ctx::register_package`), `C_` Internal Naming & Duplicate Checks

- **Status:** Accepted
- **Context:** Milestone 2 adds `stats`, `utils`, and `tools` packages alongside `base`, split across parallel disjoint submodules. GNU R exposes package functions inside `namespace:<pkg>` and attaches default packages onto `search()` (`.GlobalEnv -> package:stats -> package:utils -> ... -> package:base`).
- **Decision:**
  - Introduce `Package { name, attach, modules }` and `Ctx::register_package(&mut self, pkg: &Package)` in `roxide-eval`. Each package gets a dedicated GC-traced namespace environment (`namespace:<name>`, enclosing `BASE_ENV`) where its primitives and preludes execute; when `attach` is `true`, an attached `package:<name>` environment is spliced onto the search path below `GLOBAL_ENV`.
  - Resolve `::` and `:::` dynamically against registered package namespaces (keeping `"base"` mapped to `BASE_ENV`).
  - Enforce duplicate-name rejection in `register_module` / `register_package` for non-fallback primitive and `.Internal` registrations so parallel workers cannot silently shadow each other.
  - Name `stats`/`utils`/`tools` `.Internal` entry points with R's visible native symbol names (`C_fft`, `C_lmfit`, `C_cor`, etc.) so they cannot collide with `base` internals and bridge cleanly to `.Call` in Milestone 4.

---

## ADR-0012: S4 Object Model (`OBJSXP` + `flags.s4`), `SEXP_S4_NULL`, and Multiple Dispatch

- **Status:** Accepted (empirically verified against live GNU R 4.6.2 oracle; see `docs/M3_SPEC.md`)
- **Context:** Milestone 3 implements S4 classes, slots, inheritance, multiple dispatch, `callNextMethod()`, `show()`, and Reference Classes in `roxide-methods` without adding a new `SEXPTYPE` or coupling `roxide-eval` directly to `roxide-methods`.
- **Decision:**
  - Represent an S4 instance as any `SEXPTYPE` with `ObjFlags::s4 == true`: `Body::Obj` (`SexpType::Obj`) for slot-only classes and underlying vector/environment types for `.Data` / `.xData` classes.
  - Add permanent singleton `SEXP_S4_NULL = SexpId::permanent(12)` — an interned `SYMSXP` with bytes `b"\x01NULL\x01"` — distinguishing present `NULL` slots from absent slots while matching GNU R's XDR serialization and `attributes()` layout.
  - Preserve `flags.s4` in `duplicate`/`unshare`, compare it in `identical()`, and store package-qualified S4 `class` attributes as `STRSXP` vectors carrying their own `package` attribute.
  - Keep R metadata objects (`__C__<Class>` `classRepresentation` and `.__T__<generic>:<pkg>` method-table environments) canonical; derive epoch-invalidated Rust class/method caches in `MethodsExt: CtxExt`.
  - Multiple-dispatch acceptance contract (reconciled empirically across 35 two- and three-argument GNU R 4.6.2 probes): filter applicable candidates (`"missing"` matches only `missing` or `ANY`), score `ANY` at `1 + max_superclass_depth(across ALL target arguments)`, select minimum total inheritance distance, break ties by (a) boolean `has_exact` (`any(d_i == 0)`), (b) lexicographic per-argument distance in generic `signature` order, and (c) declared `contains` parent order; emit GNU R's ambiguity note whenever any other applicable candidate strictly beats the selected method in `(distance, parent_rank)` on at least one argument.
  - Extend `S4Hooks` in `roxide-eval` (`dispatch`, `standard_generic`, `show`, `class_chain`, `check_slot`) as the sole boundary between `roxide-eval` and `roxide-methods`.

---

## ADR-0013: Dynamic Namespace Registry, `BASE_NAMESPACE` Alias, and `roxide-pkg` Crate

- **Status:** Accepted (amends ADR-0004 DAG and ADR-0011)
- **Context:** Built-in and installed pure-R packages require a unified dynamic namespace registry, exact lexical scoping (`namespace:pkg -> imports:pkg -> baseNamespace -> .GlobalEnv -> search`), `NAMESPACE` directives (`export`, `import`, `S3method` incl. deferred cross-package registration), and `library()`/`require()`/`use()`/`loadNamespace()`/`unloadNamespace()`.
- **Decision:**
  - Make `BASE_NAMESPACE` alias the frame of `BASE_ENV` while setting `BASE_NAMESPACE`'s enclosing environment to `GLOBAL_ENV` (matching GNU R's `identical(parent.env(asNamespace("base")), .GlobalEnv)` without duplicating `base` bindings).
  - Replace static `RegisteredPackage` with dynamic `NsRecord { name, ns, imports, attached, path, builtin }` and GC-traced `pending_s3` deferred S3 registrations in `roxide-eval::namespace`, storing R's canonical `.__NAMESPACE__.` and `.__S3MethodsTable__.` environments.
  - Resolve S3 methods strictly through the calling environment chain, the generic's defining namespace `.__S3MethodsTable__.`, and `BASE_NAMESPACE`'s table (never scanning arbitrary loaded namespaces).
  - Add pure-Rust crate `roxide-pkg` (`#![forbid(unsafe_code)]`, depending on `roxide-eval`, `roxide-base`, `roxide-host`, `flate2`, `bzip2`, `lzma-rust2`) registered via `NamespaceLoader` in `roxide-eval`; `roxide-methods` integrates at R level (`.rx_cacheMetaData`) with zero Rust dependency between `roxide-pkg` and `roxide-methods`.

---

## ADR-0014: Lazy-Load `.rdb`/`.rdx` Database, `BCODESXP` Unwrap, and `R CMD INSTALL` / `BATCH`

- **Status:** Accepted
- **Context:** Installed R packages (including packages installed by GNU R 4.6.2 and pure-R packages installed via `R CMD INSTALL`) store namespace objects in `.rdb`/`.rdx` lazy-load databases with `PERSISTSXP` (`env::N`) environment references and optional `BCODESXP` byte-compiled bodies.
- **Decision:**
  - Expose `PersistHook`, `serialize_bytes`, and `unserialize_bytes` in `roxide-base::serialize`, adding `PERSISTSXP` (247) hook support and `BCODESXP` (21) decoding (returning `consts[0]`, the original source expression, as closure body until Milestone 4's bytecode VM).
  - Implement clean-room POSIX `ustar` extraction over existing `flate2`, `.rdb`/`.rdx` reader/writer over `flate2`/`bzip2`/`lzma-rust2`, in-process staged `R CMD INSTALL` (build `Ctx` + test-load `Ctx`), and `R CMD BATCH` (with `ScriptOpts { auto_print, echo }`). Reject packages containing `src/` cleanly until Milestone 4.

---

## ADR-0015: Graphics Engine (`roxide-graphics`) and Zero-New-Dependency PDF/SVG/PNG Devices

- **Status:** Accepted (amends ADR-0004 DAG: removes `roxide-stats` dependency from `roxide-graphics`)
- **Context:** Milestone 3 requires `grDevices`, `graphics`, and `grid` along with headless `pdf()`, `svg()`, and `png()` devices while keeping `#![forbid(unsafe_code)]`, zero new external crates, and fast parallel builds.
- **Decision:**
  - Drop the Rust dependency from `roxide-graphics` to `roxide-stats`; R preludes invoke `stats::fivenum` / `stats::IQR` dynamically at runtime.
  - Define a single `Device` trait in 1/72-inch, bottom-left origin, y-up device coordinates; manage device slots `1..=63`, per-device `par`/grid state, and GC-traced display lists inside `GraphicsExt: CtxExt`.
  - Implement PDF 1.4 (`FlateDecode` via `flate2`), SVG 1.1 (`<pageSet>`/`<page>` for `onefile = TRUE`), and PNG (clean-room scanline coverage rasterizer + PNG encoder using existing `flate2` zlib & CRC32) with **zero new Cargo dependencies**.
  - Use Adobe Core-14 Helvetica AFM advance widths (permissive Adobe notice) for text metrics and public-domain Hershey Simplex vector strokes for PNG glyph rendering. Numerical and geometry outputs (`par`, `usr`, `hist`, `boxplot`, `barplot`, `legend(plot=FALSE)`, `grid` units/viewports) target exact GNU R 4.6.2 parity; rendered files are verified structurally.

---

## ADR-0016: Analytical HCL Palettes from BSD-3-Clause `colorspace` Parameters

- **Status:** Accepted
- **Context:** `grDevices::hcl.colors()` and `grDevices::hcl.pals()` provide 115 perceptual HCL palettes (9 qualitative, 70 sequential, 18 diverging, 18 flexible diverging). Sampling fixed color tables would bloat the repository with binary blobs and lose accuracy at arbitrary lengths `n`.
- **Decision:**
  - Adopt the published analytical HCL trajectory equations (linear/power hue, luminance, and piecewise-triangular chroma trajectories) and the 115-row compact plain-text parameter table (`hcl_palettes.tsv`, ~5 KB) from the permissively licensed upstream `colorspace` project (`BSD-3-Clause`, Zeileis et al. 2020 *JSS* 96(1), never consulting GNU R GPL source files).
  - Evaluate every color analytically on demand through `roxide-graphics::grdevices::colors::hcl_rgb`, supporting exact arbitrary `n`, `fixup`, `alpha`, and `rev` semantics with zero binary lookup tables.
  - Record BSD-3-Clause attribution in `THIRD_PARTY_NOTICES.md`, `docs/PRIOR_ART.md`, and `third_party/licences/colorspace-BSD-3-Clause.txt`.

---

## ADR-0017: Public R 4.6.2 C API Handle ABI & Clean-Room Headers (`roxide-capi`)

- **Status:** Accepted
- **Context:** Compiled CRAN packages interact with R through `SEXP` handles, data-pointer accessors (`INTEGER`, `REAL`, `RAW`, `R_CHAR`), and public headers (`R.h`, `Rinternals.h`, `Rmath.h`, `R_ext/*.h`). Roxide's `MIT OR Apache-2.0` licence strictly prohibits reading or copying GNU R's LGPL/GPL headers or source files.
- **Decision:**
  - Isolate all C-ABI `unsafe` code into `crates/roxide-capi` and `crates/roxide-libr` (amending ADR-0001 only for these two explicit boundary crates; all 15 pure-Rust crates keep `#![forbid(unsafe_code)]`).
  - Represent `SEXP` as an opaque 64-bit pointer whose bit pattern is `SexpId::to_bits()` (never dereferenced by C code; 64-bit targets only). C `NULL` (`0`) is rejected with a deterministic R error (or converted with GNU R's warning on `.Call` return).
  - Allocate all public `R_*` `SEXP` globals (sentinels, environments, `R_BlankScalarString`, `R_NamespaceRegistry`, `R_MethodsNamespace`, and all standard `R_*Symbol` symbols) in fixed permanent generation-0 slots in `roxide-value::RSession::new` so C data exports are link-time constants.
  - Keep `roxide-gc` non-moving forever so `INTEGER()`/`REAL()`/`RAW()`/`DATAPTR_RO()`/`R_CHAR()` return direct zero-copy pointers into pinned `RVec<T>` / `RChar` backing buffers. Writable pointers (`data_ptr_rw`) are restricted strictly to atomic vector buffers; `STRSXP`/`VECSXP`/`EXPRSXP` elements are mutated exclusively via `SET_STRING_ELT`/`SET_VECTOR_ELT` to preserve GC refcounts and type invariants. Because FFI calls are opaque to the Rust optimizer, pinned `Box<[T]>` buffers remain valid across `Mode::Released` callbacks (moving to `UnsafeCell`-backed storage remains a local drop-in option if formal Miri/Tree-Borrows verification is later enabled).
  - Author all headers under `crates/roxide-capi/include/` strictly clean-room from *Writing R Extensions* and verify symbol coverage via `tools/capi/api_manifest.py` (`tools:::funAPI()` + `nm -gU`) and numeric ABI parity via `tools/capi/abi_probe.py` (which suppresses all oracle compiler diagnostics so zero oracle header text can leak).

---

## ADR-0018: Foreign Control Transfer & C++ Exception Wall (`roxide-capi`)

- **Status:** Accepted
- **Context:** R C API functions such as `Rf_error()` and `R_ContinueUnwind()` perform non-local jumps (`longjmp`), whereas Rust `Result<T, Unwind>` relies on structured unwinding and destructors. C++ CRAN packages (`fastmap`, `cpp11`, `vctrs`) also throw C++ exceptions across C frames.
- **Decision:**
  - Every Rust→C call (`.Call`, `.External`, `.C`, `.Fortran`, package `R_init_*`, C finalizers, foreign ALTREP callbacks) enters through a small clean-room C++ trampoline (`crates/roxide-capi/csrc/shim.cc`) combining `sigsetjmp` with a `try { ... } catch (...)` wall so C++ exceptions never cross into Rust and are translated into R errors.
  - Every C→Rust entry point uses a panic-free `boundary()` helper that catches panics via `std::panic::catch_unwind`, records any `Unwind` in a depth-indexed pending-unwind slot, drops all Rust locals cleanly, and invokes a plain-old-frame `longjmp` tail (never using `extern "C-unwind"`).
  - Support `R_UnwindProtect`, `R_MakeUnwindCont`, and `R_ContinueUnwind` cleanly via a `LISTSXP` (`CONS(R_NilValue, R_NilValue)`) continuation token backed by a GC-traced Rust `UnwindConts` stack per token (allowing nested reuse of static tokens and matching `SETCAR(token, R_NilValue)` as used by `cpp11`), preserving C++ RAII cleanup semantics.
  - Divergence note: `Rf_unprotect(n)` underflowing the active entry frame's protect floor raises an R error rather than silently popping Rust-owned outer roots, preventing heap corruption while matching GNU R's stack-imbalance reporting on normal `.Call`/`.External`/`.C` exit.

---

## ADR-0019: C-API Reentrancy Modes, Unified GC Finalizers & Foreign ALTREP

- **Status:** Accepted
- **Context:** Re-entrant C→Rust→R→C calls (`Rf_eval`, `R_tryCatchError`), worker threads in packages like `cli`, GC finalizers (`reg.finalizer` + `R_RegisterCFinalizerEx` + `WEAKREFSXP`), and foreign C ALTREP classes require strict borrow-safety and GC rooting discipline.
- **Decision:**
  - Track active C-API access in thread-local `Mode { Full, Session, Heap, Released }` with single-owner reborrowing per C entry point and immediate abort on main-thread double-borrow or unauthorized non-main-thread API calls (allowing read-only worker access only while the main thread is `Released`).
  - Move the unified ephemeron finalizer & `WEAKREFSXP` registry into `roxide-gc` (supporting both `FinalizerKind::RClosure(SexpId)` and `FinalizerKind::Native(usize)` while keeping `roxide-gc` `#![forbid(unsafe_code)]`). Finalizers drain strictly at evaluator safepoints (`Ctx::run_pending_finalizers`), never inside `collect_gc`.
  - Enforce GNU R's protect-stack limit (`50_000`), `unprotect_ptr`, and trampoline protect-mark restoration with exact stack-imbalance warnings.
  - Enforce the single-authority rule for foreign ALTREP objects: `roxide-value` stores opaque `ForeignAltObj` (`class_id`, `base_type`, `data1`, `data2`) traced by GC while `roxide-capi` dispatches foreign method tables exclusively.

---

## ADR-0020: Staged `libR` Build/Link, Compiled Packages, New Dependencies & Native IR VM

- **Status:** Accepted
- **Context:** Compiled CRAN packages dynamically link against `libR` (`R CMD SHLIB` / `R CMD INSTALL`), invoke `R CMD config`, link system BLAS/LAPACK, and expect JIT bytecode compilation (`compiler::enableJIT`).
- **Decision:**
  - Build `libR` via `crates/roxide-libr` (`staticlib`) finalized by the system linker with an explicit manifest export list and staged into `target/rhome/{bin,include,lib,etc/Makeconf,share/make/shlib.mk}` via `scripts/stage_rhome.sh`.
  - Link compiled CRAN packages against system BLAS/LAPACK (Accelerate on macOS, OpenBLAS on Linux) while preserving Roxide's bit-exact `roxide-math` engine path (ADR-0009).
  - Stamp Roxide-installed compiled packages with `Meta/Roxide.rds` and refuse foreign-runtime shared libraries via pure-Rust Mach-O/ELF dependency inspection so GNU R is never loaded into a Roxide process.
  - Adopt two vetted permissive dependencies: `cc` (`MIT OR Apache-2.0`) for building `roxide-capi`'s C/C++ trampoline shim and `libloading` (`ISC`) for dynamic library loading (`dyn.load`), plus optional out-of-workspace nightly `cargo-fuzz` (`MIT OR Apache-2.0`).
  - Never execute GNU R `BCODESXP` instruction streams; lower `consts[0]` ASTs to Roxide's own clean-room register IR (`roxide-eval::vm`) with `ENVSXP` `shape_version` inline caches (bumped on binding insert/remove, `env_set_enclos`, lock/active flag mutations, and function/promise binding overwrites; call-site cache hits also confirm the target function `SexpId` in the defining frame) and guard-based fallback to the AST semantic reference.



