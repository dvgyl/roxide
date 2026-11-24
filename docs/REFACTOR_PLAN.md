# Roxide pstack refactoring plan

This plan sequences behavior-preserving structural refactors across all 17 crates under the installed `pstack` skill suite (`poteto-mode`, `refactoring`, `thermo-nuclear-code-quality-review`, `deslop`, `no-comments`, `unslop`). Every lane keeps public contracts intact, subtracts before adding, removes unjustified lint suppressions and narration comments, and proves equivalence on existing unit, integration, and differential tests.

Execution runs in the background alongside Milestone 4, sharing the 2-Cargo-slot budget (`CARGO_BUILD_JOBS=2`, `RUST_TEST_THREADS=3`). Lanes below are grouped to avoid merge conflicts with active Milestone 4 waves.

## Wave R1 (safe now, disjoint from Milestone 4 W1-C and W2)

### Lane R1-A (`roxide-syntax`)
1. Delete the 10 narration comments in `crates/roxide-syntax/src/parser.rs` (lines 236, 984) and `crates/roxide-syntax/src/lexer.rs` (line 249 token banners).
2. Move braces, functions, and control forms from `parser.rs` into `parser/forms.rs`. Replace the newline and assignment flag pair with an `ExprContext` enum (`TopLevel`, `Block`, `Parens`, `Arg`). Share condition parsing for `if` and `while`, and share the unnamed-operand call builder while preserving delimiter errors, spans, and `for` trivia handling.
3. Move pipe validation and rewriting into `parser/pipe.rs`. Separate target classification from AST mutation, and move the argument copy after the extraction check.
- Pin: `cargo test -p roxide-syntax` and `conformance/layer1/01_syntax_deparse.rcase`.

### Lane R1-B (`roxide-graphics`)
1. Delete `par::parse_color` in `crates/roxide-graphics/src/graphics/par.rs` and call `grdevices::parse_rgba` directly while preserving color index 0 background resolution.
2. Extract stored `par` schema (`graphics/par/stored.rs`) and snapshot codec (`graphics/par/snapshot.rs`), removing the dummy `pch` templates while preserving list order and field names.
3. Add typed `Segments`, `Arrows`, and `Rect` endpoint command variants and move their renderer to `graphics/lowlevel/endpoints.rs`.
4. Share dash-pattern, line-end, and line-join decoding across base graphics (`lowlevel.rs`) and grid (`grid/draw.rs`), adding a pin for blank and numeric line types first.
5. Replace positional `Vec<Units>` in `grid/draw.rs` with named geometry payloads and extract `grid/draw/bounds.rs`.
6. Replace inverse offset calculations in `grid/measure.rs` with one unit-code table and a `SummaryOp` enum.
7. Replace the 11-argument palette helper in `grdevices/palette.rs` with a parameter struct and delete `#[allow(clippy::too_many_arguments)]`.
- Pin: `cargo test -p roxide-graphics` and `conformance/layer3/13..21`.

### Lane R1-C (`roxide-math` and `roxide-stats`)
1. In `roxide-math`: remove repeated Wilcoxon quantile checks in `dist/discrete/rank.rs`; consolidate identical scalar probability helpers (`invalid`, `on_scale`, `zero`, `step`, probability validation, endpoint selection) into `dist.rs`; move the standalone noncentral Student's t block out of `dist/continuous/noncentral.rs` into `dist/continuous/noncentral/student_t.rs`.
2. In `roxide-stats`: coerce optimizer bounds once in `optimize/multidim.rs`; classify formula operators once with an `Operator` enum in `model/formula.rs`; move correlation out of `htest/rank.rs` into `htest/rank/correlation.rs` and replace `sample_size: Option<usize>` with `SignedRank` and `RankSum { first_size }`; share zero-margin removal and move Fisher's test into `htest/tables/fisher.rs`; replace `interpolate` plus `Option<Tree>` in `smooth/loess.rs` with `Surface::Direct` and `Surface::Interpolated(Tree)`; replace integer stop codes in `smooth/nonlinear.rs` with a `StopReason` enum.
- Pin: `cargo test -p roxide-math -p roxide-stats` and `conformance/layer2/`.

### Lane R1-D (`roxide-methods`, after W1-V1b lands)
1. Remove the redundant slot uniqueness scan in `class/definitions.rs` and return `Option<String>` directly from `data_class`.
2. Move method-key encoding and signature-width rewriting into `dispatch/method_table.rs`, adding a shrinking-width removal pin first.
3. Share one breadth-first inheritance walk in `class/inheritance.rs` and distinguish custom-path outcomes in `convert`.
4. Move prototype decoding and slot default construction into `class/definitions/prototype.rs`.
- Pin: `cargo test -p roxide-methods` and `conformance/layer3/01..05b`.

### Lane R1-E (`roxide-base` and `roxide-utils`, after W1-V1b lands and before W2-F)
1. In `roxide-base`: register `text::transcode` directly for both `enc2utf8` and `enc2native` (adding an `enc2native` pin first); extract shared factor-code decoding into `ops/common.rs`; unify connection restoration across `operate` and `write_sink` and move sink policy to `io_os/connection/sink.rs`; generalize `Reader::numeric` for list and expression vectors and move `CodecSession` to `serialize/session.rs`; write `rbind` and `cbind` directly in column-major order in `subset/containers.rs`; replace `(allow_extend, strict)` booleans in `subset/index.rs` and `subset/replacement.rs` with an `IndexPolicy` enum.
2. In `roxide-utils`: share Rd comment whitespace handling in `tools_rd/render.rs` and delete the redundant `index != 0` check; consolidate per-tag Rd grammar definitions into `tools_rd/parser/grammar.rs`.
- Pin: `cargo test -p roxide-base -p roxide-utils` and `conformance/layer1/` plus `conformance/layer2/19_tools_rd_license.rcase`.

## Wave R2 (core runtime leaf crates, safe after W1-C lands)

### Lane R2-A (`roxide-gc`)
1. Unify `Finalizer` and `WeakRefEntry` into one `Ephemeron` entry in `Heap`, eliminating the meaningless `onexit` field on non-finalizer weakrefs while keeping public `register_*` and `remove_weakref` signatures unchanged.
2. Add `FinalizerKind::closure() -> Option<SexpId>` to replace duplicate `RClosure` checks.
3. Split `crates/roxide-gc/src/heap.rs` (1,099 lines) into `heap/mod.rs`, `heap/protect.rs`, and `heap/ephemeron.rs`, extracting `seed_roots`, `ephemeron_fixpoint`, `partition_dead`, `resurrect`, and `retire_slot` and deleting numbered step comments.
- Pin: `cargo test -p roxide-gc` and `roxide-eval` finalizer GC parity tests.

### Lane R2-B (`roxide-value`)
1. Add `RObject::type_mismatch(expected)` to replace 62 hand-built `TypeMismatch` sites.
2. Add a crate-private `replace_edge` helper to collapse the 8 refcounted edge setters in `capi_support.rs`.
3. Add a crate-private `trait AtomicBody: Elem` to collapse repeated typed view and `alloc_*_from` boilerplate in `api.rs`.
4. Split `crates/roxide-value/src/capi_support.rs` (1,060 lines) into `capi_support/{mod,dataptr,extptr,weakref,mutate,strings}.rs` with zero public signature changes.
5. Remove stale `#[allow(dead_code)]` attributes in `object.rs` and `alt.rs` (deleting genuinely dead items), replace `clippy::too_many_arguments` in `deparse/atomic.rs` with a parameter struct, and drive permanent slot creation in `session.rs` from one ordered table so index order is structural rather than prose comments.
- Pin: `cargo test -p roxide-value -p roxide-capi`.

### Lane R2-C (`roxide-host` and `roxide-diag`)
1. Add a `forward_host!` macro in `roxide-host` and migrate `testing.rs` and `FailingHost` (deferring `ctx.rs` until Wave R3).
2. In `roxide-diag`, delete unused `format::calls_trailer`, `msg::EMBEDDED_NUL`, and `msg::NO_LOOP_FOR_BREAK_NEXT`, and introduce a named `PendingWarning` struct replacing `(Option<Vec<String>>, String)`.
- Pin: `cargo test -p roxide-host -p roxide-diag -p roxide-eval`.

## Wave R3 (core evaluator and cross-crate polish, after Milestone 4 W2 lands)

### Lane R3-A (`roxide-eval` typed dispatch and module splits)
1. Update `primitive!` to accept `$fun:expr` and eliminate the 28 primitive self-name string switches across `language.rs`, `conditions.rs`, `environment.rs`, `fallback.rs`, and `control.rs` in favor of typed enums (`Predicate`, `FramePart`, `FnPart`).
2. Adopt `roxide_diag::ConditionKind` throughout `conditions.rs` and replace `Handler.target: Option<FrameId>` plus dummy `SEXP_NIL` with `enum HandlerKind { Calling(SexpId), Exiting(FrameId) }`.
3. Extract `options.rs` (`struct Options` with one cached-key table) from `conditions.rs`, and split `environment.rs` (1,283 lines) into `environment.rs`, `frames.rs`, `gc.rs`, and `namespace.rs` with a single `run_finalizer_batch`.
4. Extract `PRIMITIVE_FORMALS` const table in `language.rs`.

### Lane R3-B (`Ctx` decomposition, shadow primitive removal, and cross-crate cleanup)
1. Group `Ctx`'s 33 fields into `PrimTable`, `FrameStack`, `EvalDepth`, `NsState`, `Hooks`, and `streams.rs` (`StreamSet` plus `Scoped | Persistent`).
2. Audit and delete overridden bootstrap primitives in `builtins.rs` and `fallback.rs` (about 1,200 lines), migrating standalone tests to `tests/` with a `roxide-base` dev-dependency.
3. Replace pass-through aliases (`warning_at`, `signal_error_at`, `protect_eval_result`, `flush_stream_sinks`), simplify `dispatch.rs` (`MethodFrame::origin`, `GroupGeneric::as_str()`), replace repeated `intern("names" | "class" | ...)` calls with `SYM_*` constants, and share canonical argument helpers where error contracts match.
- Pin: full `scripts/gate.sh`, Layers 1 to 3 under `ROXIDE_JIT=0` and `ROXIDE_JIT=3`, and GC-torture suites.
