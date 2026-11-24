# Milestone 4 architecture blueprint: C API, compiled packages, bytecode VM, and hardening

## 0. Current state and probe results

### 0.1 What the code already gives M4

| Fact | Location | M4 consequence |
|---|---|---|
| `SexpId(NonZeroU64)` is `repr(transparent)`, bits `[63..36]` generation, `[35..4]` slot+1, `[3..0]` zero. | `roxide-gc/src/id.rs` | The C `SEXP` value **is** `SexpId::to_bits()`. A reused slot gets a new generation, so a stale `SEXP` fails the lookup (`Heap::get` returns an error). It does not corrupt memory. |
| `Heap::alloc` never collects. Collection runs only at `Ctx::safepoint()`. | `heap.rs:222`, `ctx.rs:912` | C allocators are explicit safepoints. GC torture (`GcPolicy::Torture`) gives `gctorture` behaviour for C code at no extra cost. |
| The protect stack (`protect`, `reprotect`, `unprotect`, `protect_mark`, `unprotect_to`) and the precious multiset (`preserve`, `release`) exist. | `heap.rs:377–437` | `PROTECT`, `REPROTECT`, `R_PreserveObject` and `R_ReleaseObject` map 1:1. `UNPROTECT_PTR` and a depth limit are missing. |
| `RVec<T> { buf: Box<[T]>, len }`. Slots live in `Vec<Slot<T>>`, so the `RObject` moves when the heap grows, but the `Box` payload does not. | `object.rs:236` | `REAL()` and similar return `buf.as_ptr()`. The pointer stays valid until the object is freed. `buf.len()` is the capacity, so the R 4.6 resizable-vector API (`R_allocResizableVector`, `R_resizeVector`, `R_maxLength`) needs no new type. |
| `RLogical` is `repr(transparent) i32`. `RComplex` is `repr(C) {re, im}`. `CharId` and `SexpId` are `repr(transparent)` u64. | `na.rs:126,209`, `object.rs:106,120` | `LOGICAL`, `INTEGER`, `REAL`, `COMPLEX`, `RAW`, `STRING_PTR_RO` and `VECTOR_PTR_RO` share memory with C directly. No data is copied. This needs a 64-bit target. |
| `RChar.bytes_nul` already stores a trailing NUL byte. | `object.rs:166` | `R_CHAR()` returns `bytes_nul.as_ptr()` directly. |
| `ExtPtrData { addr, rust, tag, prot }` and `Body::Alt` with a `materialized` cache exist. | `object.rs:329`, `alt.rs:272` | External pointers and ALTREP `DATAPTR` work with small accessor additions. |
| `S4Hooks` and `NamespaceLoader` use the `&'static dyn` hook pattern. | `api.rs:284,329` | Add `NativeHooks` with the same pattern. Then `roxide-base` never depends on `roxide-capi`. |
| `useDynLib` (including `.fixes`) is already parsed. `R CMD INSTALL` rejects `src/`. | `nsfile.rs:218,493–514`, `install.rs:168` | The loader and install code only need to call the hooks. |
| `BCODESXP` decoding returns `consts[0]`. | `serialize/read.rs:723–764` | The VM never needs to run GNU R bytecode (§5). |
| `WeakRefData` traces `value` strongly. No finalizer registry exists. | `object.rs:514–516` | **W0 gap:** C finalizers (`R_RegisterCFinalizerEx`) and ephemeron weak references need new support in `roxide-gc`. |

### 0.2 Black-box oracle findings

1. **API classification:** `tools:::funAPI()` lists 1470 entry points: 1058 `api`, 347 `eapi` (experimental), 52 `emb` (embedding), and 13 `for` (Fortran). This is the authoritative function manifest source. It lists functions only. Data symbols (`R_NilValue`, `R_NamesSymbol`, …) do not appear, so they must come from `nm -gU libR.dylib` filtered to the names documented in *Writing R Extensions* (WRE).
2. **Macros and remapping:** 613 `api` names have no export with the same name. The causes are macros (`PROTECT`→`Rf_protect`, `CHAR`→`R_CHAR`, `ISNA`→`R_IsNA`), the `Rf_` remap (36 names), and the BLAS/LAPACK symbols, which live in `libRblas` and `libRlapack`, not in `libR`. The manifest tool must resolve these aliases.
3. **Removed accessors:** `DATAPTR`, `CLOENV`, `BODY`, `FORMALS`, `ENCLOS`, `FRAME` and `PRVALUE` are **not** `api` in 4.6.2. The replacements are `DATAPTR_RO`, `DATAPTR_RW`, `DATAPTR_OR_NULL`, `R_ClosureFormals`, `R_ClosureBody`, `R_ClosureEnv`, `R_ParentEnv`, `R_getVar`, `R_getVarEx` and `R_existsVarInFrame`. All of these are in the `api` set.
4. **Toolchain (oracle `etc/Makeconf`):**
   - `CC = clang -arch arm64`
   - `CXX = clang++ -arch arm64 -std=gnu++20` (C++20 is confirmed)
   - `SHLIB_LDFLAGS = -dynamiclib -Wl,-headerpad_max_install_names -undefined dynamic_lookup`
   - `LIBR = -framework R`
   - `BLAS_LIBS = -lRblas`, `SHLIB_EXT = .so`
   - `SHLIB_OPENMP_CFLAGS` is empty.
   - `gfortran` is **not installed** on this workstation.
5. **Hard-linked `libR`:** CRAN-built `.so` files hard-link `/Library/Frameworks/R.framework/Versions/4.6/Resources/lib/libR.dylib` by absolute install name (`otool -L stats.so`). Roxide must never load a GNU R-built `.so`. It would load GNU R's GPL runtime next to ours. See §3.6.
6. **Oracle blocker:** `R CMD config` and `R CMD SHLIB` fail on the relocated oracle. The `bin/R` script sources a hard-coded `/Library/Frameworks/R.framework/Resources/etc/ldpaths`. **Workaround (no oracle change):** compile oracle-side fixtures with `clang -dynamiclib -undefined dynamic_lookup -I$ORACLE/include` and `dyn.load()` them into oracle `Rscript`. Flat lookup then binds to the `libR` that is already loaded.
7. **Symbol census:** `nm -u` over the base and recommended package `.so` files found 521 distinct symbols. The most frequent are:
   - `Rf_error` (23), `R_registerRoutines` (23), `R_useDynamicSymbols` (23), `R_forceSymbols` (18)
   - `Rf_asInteger`, `Rf_protect`, `Rf_unprotect`, `Rf_install`, `Rf_allocVector`, `R_alloc`, `INTEGER` (16 each)
   - `SET_STRING_ELT`, `Rf_setAttrib`, `Rf_mkChar`, `Rf_length` (15 each)

   **Bias warning:** base packages compile with internal headers. Their imports of `R_PPStack`, `R_BadLongVector` and `ALTREP_LENGTH` do **not** predict CRAN needs. Re-run the census on the CRAN corpus built against public headers (W0 tool).

## 1. Public C API surface and clean-room boundary

### 1.1 New crates

```
crates/roxide-capi/             # rlib; the ONLY crate with audited unsafe (with roxide-libr)
  build.rs                      # cc: csrc/shim.cc (C++) + csrc/shim_va.c (C variadics)
  include/                      # clean-room headers (§1.2), shipped to R_HOME/include
  csrc/shim.cc                  # setjmp trampolines + catch(...) wall
  csrc/shim_va.c                # Rf_error, Rf_errorcall, Rf_warning*, Rprintf, REprintf, Rvprintf, REvprintf
  csrc/launcher.c               # R_HOME/bin/exec/R: int main → roxide_main
  src/abi.rs                    # Sexp(u64), Rboolean, R_xlen_t, Rcomplex, method-def structs, static asserts
  src/cx.rs                     # TLS ApiCx modes, with_cx(), call_foreign() (§2.4)
  src/jump.rs                   # pending-unwind slots, jump targets, boundary() (§2.2)
  src/globals.rs                # #[no_mangle] data symbols (§1.4)
  src/api/{vector,attrib,coerce,pairlist,string,sym,env,eval,error,print,unwind,
           memory,extptr,weakref,rmath,random,arith,utils,altrep,resizable,s4,parse}.rs
  src/dll.rs  src/register.rs  src/dotcall.rs  src/hooks.rs   # dlopen, registration, marshalling, NativeHooks impl
crates/roxide-libr/             # crate-type = ["staticlib"]; re-exports capi + roxide_main
```

Dependency order: `roxide-capi` depends on `roxide-eval`, `roxide-value`, `roxide-gc`, `roxide-math`, `roxide-base` and `roxide-pkg`. `roxide-libr` depends on `roxide-capi` and `roxide-cli` (add a library target to `roxide-cli`).

New dependencies:
- `cc` (build only, MIT/Apache-2.0)
- `libloading` (ISC) for `dlopen`, `dlsym`, `dlclose` and `dlerror` with `RTLD_LOCAL`, `RTLD_GLOBAL` and `RTLD_NOW`

Do not use `bindgen`. Write the Rust and C sides by hand and let the ABI probe check them.

### 1.2 Header set (write from WRE and *R Internals* only)

| Header | Contents | Wave |
|---|---|---|
| `R.h` | Includes `R_ext/{Arith,Boolean,Complex,Constants,Error,Memory,Print,Random,Utils,RS}.h` | W1 |
| `Rinternals.h` | Opaque `typedef struct SEXPREC *SEXP;`, `SEXPTYPE` constants, accessors, the full `Rf_` remap block guarded by `#ifndef R_NO_REMAP`, and `R_NO_REMAP` forced on for C++ (R 4.5+ rule). No `USE_RINTERNALS`. | W1 |
| `Rdefines.h` | Legacy macros written only in terms of API functions. | W1 |
| `Rmath.h` | `d/p/q/r*` with the `Rf_` remap and `MATHLIB_STANDALONE` guard. Backed by `roxide-math`. | W1 |
| `Rversion.h`, `Rconfig.h` | `R_VERSION`, `R_Version()`, `R_MAJOR` "4", `R_MINOR` "6.2", platform macros. | W1 |
| `R_ext/Rdynload.h` | `DllInfo` (opaque), the 4 method-def structs, `R_registerRoutines`, `R_useDynamicSymbols`, `R_forceSymbols`, `R_RegisterCCallable`, `R_GetCCallable`, `R_FindSymbol` | W1 |
| `R_ext/{Error,Memory,Print,Utils,Arith,Boolean,Complex,Constants,RS,Visibility,Random,Rallocators,Itermacros,Parse,Riconv,libextern}.h` | As documented in WRE | W1 |
| `R_ext/{BLAS,Lapack,Linpack,Applic}.h` | Prototypes only. BLAS/LAPACK resolve to the system libraries (§3.5). `Applic.h` (`vmmin`, `nmmin`, `lbfgsb`) maps to `roxide-stats` optim. | W1 (protos), W3 (Applic) |
| `R_ext/Altrep.h` | Class constructors and method setters | W3 |
| `Rembedded.h`, `Rinterface.h` | `Rf_initEmbeddedR` and `Rf_endEmbeddedR` only | W4 stretch |
| `R_ext/GraphicsEngine.h`, `GraphicsDevice.h` | 154 symbols | **Out of M4** (move to M5) |

Clean-room rules for headers:
- Put an `SPDX-License-Identifier: MIT OR Apache-2.0` line at the top of each header.
- Do not copy comments, macro bodies or layout text from GNU R.
- `licence_scan.py` must reject GPL/LGPL banner text under `include/`.
- Agents must never open the oracle `include/` files. Only tools use them (§1.3).

### 1.3 Manifest-driven ABI (single source of truth)

`tools/capi/api_manifest.py` writes `crates/roxide-capi/api_manifest.tsv` with the columns: `name`, `export_symbol`, `kind{fn,data,macro}`, `apitype{api,eapi,emb,compat}`, `header`, `alloc{0,1}`, `min_mode{heap,session,full}`, `wave`, `status`.

Sources:
- function names and `apitype` from oracle `tools:::funAPI()`
- data names from `nm -gU` on oracle `libR` (names only)
- the `compat` rows from the CRAN census (`tools/capi/symbol_census.py`: build packages against oracle headers in a `tempfile` directory, then run `nm -u`)

Tools that use the manifest:
- **`check_exports.py`** (gate): `nm -gU R_HOME/lib/libR.{dylib,so}` must equal the manifest `export_symbol` set for every row with `status=done`.
- **`abi_probe.py`** (gate): generates `probe.c` from the manifest and compiles it twice, once with `-I$ORACLE/include` and once with our `include/`. The program prints the following, and the tool diffs the two outputs:
  - `sizeof` and `alignof` for `SEXP`, `Rboolean`, `R_xlen_t`, `R_len_t`, `Rcomplex`, `Rbyte`, `PROTECT_INDEX`, the 4 method-def structs, `R_altrep_class_t` and `cetype_t`
  - `offsetof` for each documented struct field
  - every `SEXPTYPE`, `CE_*`, `NA_INTEGER`, `R_XLEN_T_MAX` and `R_SHORT_LEN_MAX` value

  The probe never links `libR`. It also compiles a C++ unit with `-std=gnu++20 -Wall -Werror`, with and without `R_NO_REMAP`.
- **`abi_sigdiff.py`** (coordinator decision): extracts canonical function types from both header sets with `clang -Xclang -ast-dump=json` and reports only mismatched signatures. Agents never see header text.

### 1.4 `SEXP` ABI decision

- **Representation.** `SEXP` is an opaque pointer-width value with bits equal to `SexpId::to_bits()`. C never dereferences it, because `SEXPREC` is an incomplete type in our headers. Pointer equality equals object identity. The low 4 bits are zero, as for a 16-byte-aligned pointer. C `NULL` is never valid. An API function that receives `NULL` raises the R error `"invalid NULL SEXP passed to <fn>"`. GNU R crashes in this case.
- **64-bit only.** Add `_Static_assert(sizeof(void*) == 8)` in `Rinternals.h` and a matching Rust `const` assert.
- **Data globals are link-time constants.** Extend the ADR-0002 permanent table:
  - indices 13…N: every `R_*Symbol` in the manifest (`R_NamesSymbol`, `R_DimSymbol`, `R_DimNamesSymbol`, `R_ClassSymbol`, `R_LevelsSymbol`, `R_RowNamesSymbol`, `R_TspSymbol`, `R_DotsSymbol`, `R_DollarSymbol`, `R_BracketSymbol`, `R_Bracket2Symbol`, `R_BraceSymbol`, `R_DoubleColonSymbol`, `R_TripleColonSymbol`, `R_ModeSymbol`, `R_NameSymbol`, `R_NaRmSymbol`, `R_QuoteSymbol`, `R_SeedsSymbol`, `R_SortListSymbol`, `R_SourceSymbol`, `R_SpecSymbol`, `R_LastvalueSymbol`, `R_DropSymbol`, `R_EvalSymbol`, `R_FunctionSymbol`, `R_PackageSymbol`, `R_PreviousSymbol`, `R_SrcrefSymbol`, `R_SrcfileSymbol`, `R_AsCharacterSymbol`, `R_AtsignSymbol`, `R_BaseSymbol`, `R_NamespaceEnvSymbol`, `R_dot_Generic`, `R_dot_Method`, `R_dot_defined`, `R_dot_target`, `R_dot_packageName`, …)
  - `R_BlankScalarString`, `R_TrueValue`, `R_FalseValue`, `R_LogicalNAValue`, `R_NamespaceRegistry`, `R_MethodsNamespace`

  `globals.rs` then holds `#[no_mangle] pub static R_NilValue: Sexp = Sexp(0x10);` and similar constants. Numeric globals (`R_NaReal`, `R_NaInt`, `R_NaN`, `R_PosInf`, `R_NegInf`) are constants too. Only `R_Interactive` is mutable.

  **Why:** ADR-0014 runs several `RSession` objects in one process (build ctx and test-load ctx). A non-permanent symbol has a different `SexpId` in each session, so a single global variable cannot be correct. Permanent symbols are never collected, so C code that caches `install()` results in `static` variables stays correct.

### 1.5 Data pointers and ALTREP

| Call | Rule |
|---|---|
| `INTEGER`, `REAL`, `LOGICAL`, `RAW`, `COMPLEX`, `DATAPTR_RW` | Return `buf.as_mut_ptr()`. The type check uses R's exact error text (black-box probe, for example `INTEGER() can only be applied to a 'integer', not a '…'`). Whether `INTEGER()` accepts `LGLSXP` comes from an oracle probe. |
| `*_RO`, `DATAPTR_RO`, `STRING_PTR_RO`, `VECTOR_PTR_RO` | Apply the ADR-0003 wrapper duplication guard first, then return the pointer. |
| Zero-length vectors | Return `Box<[T]>`'s aligned dangling pointer, which is non-NULL. |
| Built-in ALTREP (compact sequences, wrappers, deferred strings) | `DATAPTR*` materializes into `AltObj.materialized` once. That buffer is a stable `Box`. `*_OR_NULL` returns NULL when not yet materialized. `*_GET_REGION`, `*_ELT`, `*_IS_SORTED` and `*_NO_NA` use the class methods. |
| Pointer lifetime | Valid until GC frees the object, or until `R_resizeVector` goes past capacity. ADR-0017 invariant: **`roxide-gc` stays non-moving.** |
| Foreign (C-defined) ALTREP classes (W3) | `AltObj` stores `AltKind::Foreign(ForeignAlt)`. The object starts Lazy with `materialized = None`. Materialization sets `materialized = Some` and makes the object Owned. The object stays Owned. |

`roxide-value` never calls C. A read that needs a C method on a Lazy object returns `ValueError::ForeignAltrep { id, need }`. The evaluator calls the method before it borrows a `VecView` (§2.4).

An `R_altrep_class_t { SEXP ptr }` holds an `EXTPTRSXP` descriptor. Its `tag` stores the class `Sym` and its `prot` stores the package `Sym`. Its `addr` stores the index in the `CtxExt` method table plus one. Roxide creates stable, rooted descriptors on demand for built-in classes such as `compact_intseq` and `wrap_integer`.

Roxide seals the length after the first `Length` call. After materialization, Roxide owns the single Rust buffer and stops calling C class methods for that object. Direct writes to `data2` that bypass `DATAPTR` have no effect on the Owned buffer.

## 2. GC safety, protect stack, unwind safety, and reentrancy

### 2.1 Protect stack mapping

| C API | Implementation |
|---|---|
| `Rf_protect(s)` | `heap.protect(id)` and return `s`. Depth limit 50000 (`--max-ppsize`). When exceeded, raise `protect(): protection stack overflow` (exact text from an oracle probe). |
| `Rf_unprotect(n)` | `heap.unprotect(n)`. On underflow, raise R's exact error. |
| `R_ProtectWithIndex`, `R_Reprotect` | `ProtectIndex(usize)` ↔ C `int`. Range-check the value. |
| `Rf_unprotect_ptr(s)` | **New `Heap::unprotect_ptr`:** remove the topmost matching entry and shift the entries above it down. |
| `R_PreserveObject`, `R_ReleaseObject`, `R_PreserveInMSet`, `R_ReleaseFromMSet` | Precious multiset (multiset objects are `VECSXP`s that are themselves preserved). |
| Trampoline entry and exit | Record `protect_mark` and the `R_alloc` depth on entry. On a **normal** return, if the depth differs, print `Warning: stack imbalance in '.Call', A then B` to stderr (exact format from an oracle probe), then call `unprotect_to(mark)`. On a **jump**, call `unprotect_to(mark)` without a warning. GNU R behaves the same way. |
| `R_alloc`, `vmaxget`, `vmaxset`, `R_CheckStack` | Side table `Vec<Box<[MaxAlign]>>` with `#[repr(align(16))]`. It is not stored in the GC heap. Reset it at trampoline exit. |
| `R_Calloc`, `R_Free` (`R_chk_calloc`, `R_chk_free`, `R_chk_realloc`) | System `calloc`, `realloc` and `free` through `libc`. On failure, raise R's exact error. |

**Safepoints and GC torture.** A safepoint runs only in API functions with `alloc=1`: `Rf_allocVector*`, `Rf_mkChar*`, `Rf_cons`, `Rf_lcons`, `Rf_lang*`, `Rf_list*`, `Rf_duplicate`, `Rf_shallow_duplicate`, `Rf_coerceVector`, `Rf_eval`, `R_ParseVector`, `R_MakeExternalPtr`, `Rf_ScalarX`, `Rf_mkString`, `Rf_install`. Each such function **protects all of its `SEXP` arguments before its safepoint**, because GNU R's own implementations do the same.

Accessors never collect. Examples: `INTEGER`, `LENGTH`, `STRING_ELT`, `SET_VECTOR_ELT` (it adjusts refcounts but does not allocate).

With `ROXIDE_GC_TORTURE=1`, every allocating call collects. A forgotten `PROTECT` then becomes a **deterministic stale-handle R error**, not heap corruption. `ROXIDE_CAPI_STRICT=1` turns that error into an abort that prints the API function name and the stale bits.

Roots during C execution:
- the `Ctx` roots, the protect stack and the precious set
- `.Call` arguments, protected by the trampoline
- `CapiState` (through `ctx.ext::<CapiState>()` with `Trace`): registered `NativeSymbolInfo` objects, unwind tokens, ALTREP class objects, and `R_RegisterCCallable` metadata

### 2.2 Foreign control transfer: setjmp trampolines and POF-by-construction

Principles:
- Rust never unwinds into C, and C never unwinds into Rust.
- A `longjmp` jumps only over C frames plus **at most one Rust frame**. That Rust frame must be a plain-old frame (POF), meaning it holds no live values that need `Drop`. Construction guarantees this.

Components:

1. **`csrc/shim.cc`** is C++ so that it can catch foreign exceptions.

   ```c
   struct rx_target { jmp_buf buf; struct rx_target *prev; };
   static _Thread_local struct rx_target *rx_top;
   int rx_tramp_call(void *fn, int nargs, const uint64_t *args, uint64_t *out);
   // also: rx_tramp_void1, rx_tramp_unwind_fun, rx_tramp_finalizer, rx_tramp_alt_*
   ```

   Each trampoline does three things:
   - Push a target and call `_setjmp`.
   - Call the foreign function inside `try { … } catch (...) { status = RX_CXX_ESCAPED; }`.
   - Pop the target and return a status code: `OK`, `JUMPED` or `CXX_ESCAPED`.

   The trampoline has no automatic objects with destructors, so a `longjmp` out of the `try` region is well defined. A C++ exception that escapes `.Call` becomes the R error `C++ exception escaped native routine '<name>'`. In GNU R this case calls `std::terminate`, so Roxide is strictly safer here.

   Dispatch is generic: one trampoline per arity 0…65 through a `switch` that casts to `SEXP(*)(SEXP, …)`. A `.C` dispatcher passes `void*` arguments.

2. **`roxide_jump()`** (C, `noreturn`) calls `_longjmp(rx_top->buf, 1)`. If `rx_top` is NULL (a jump outside any foreign call), it prints a diagnostic and calls `abort()`.

3. **`boundary()`** (Rust) wraps every exported function that can raise an error:

   ```rust
   #[inline(always)]
   fn boundary<T: Copy>(min: Mode, f: impl FnOnce(&mut ApiCx<'_>) -> CResult<T>) -> T {
       match run(min, f) {              // run(): #[inline(never)], catch_unwind, drops f and all locals
           Ok(v) => v,
           Err(()) => unsafe { roxide_jump() }, // only Result<Copy, ()> is live: POF by construction
       }
   }
   ```

   `run` converts the `Err(Unwind)` (or a caught Rust panic, which becomes `Unwind::Internal`) into a **pending-unwind slot** indexed by the current trampoline depth. Then it returns `Err(())`.

4. **The Rust caller of a trampoline** receives `JUMPED`, takes the pending `Unwind` from the slot and returns `Err(unwind)` to the evaluator. All of R's machinery then runs as typed `Result` (ADR-0004): `tryCatch`, restarts, `on.exit` and frame pops.

5. **Rule (ADR-0018): every Rust→C call goes through a trampoline.** This includes `R_init_<pkg>`, routines, C finalizers, ALTREP methods, `R_UnwindProtect` and `R_ToplevelExec` callbacks, and `R_ExecWithCleanup`. So the innermost jump target is always the most recent Rust→C transition.

Variadic functions are in C (`shim_va.c`), because Rust `c_variadic` is unstable. For example, `Rf_error(fmt, …)` formats into `char[8192]` (truncation text and limit from an oracle probe) and then calls the Rust `rx_raise_error(call_bits, msg)`, which has type `-> !`. That function runs `ctx.signal_error(call, msg)`. Calling handlers run normally in Rust, the result is stored, and the jump follows.

`Rf_warning` returns normally unless `options(warn = 2)` makes it an error. In that case it jumps.

Nested example:

```
T0 → C → Rf_eval → R → .Call → T1 → C2 → Rf_error
```

`Rf_error` jumps to T1. T1 returns `Err` to the evaluator, the evaluator returns `Err` to the `Rf_eval` boundary, and that boundary jumps to T0.

### 2.3 Error and unwind API mapping

| API | Contract |
|---|---|
| `Rf_error`, `Rf_errorcall`, `Rf_errorcall_cpy` | Signal the condition with handlers, store the pending unwind, jump. |
| `Rf_warning`, `Rf_warningcall`, `Rf_warningcall_immediate` | Go through the warning system. This can jump (`warn = 2`). |
| `R_CheckUserInterrupt` | `ctx.poll_interrupt()`. If set, signal an `interrupt` condition, which can jump. |
| `R_tryEval`, `R_tryEvalSilent` | Pure Rust: `ctx.eval` returns a `Result`, so no setjmp is needed. On error, print the message (only `R_tryEval` prints), set `*errorOccurred = 1` and return NULL. |
| `R_ToplevelExec(fun, data)` | Save the handler stack and hide it, then call `fun` through a trampoline. On `JUMPED`, print the error and return `FALSE`. Restore the handler stack in all cases. |
| `R_tryCatchError`, `R_tryCatch`, `R_withCallingErrorHandler` | Install a Rust-level handler frame, then call the C body through a trampoline. On error, call the C handler through a trampoline. |
| `R_ExecWithCleanup(fun, data, clean, cdata)` | Trampoline `fun`, always trampoline `clean`, and re-raise the pending unwind if `fun` jumped. |
| `R_MakeUnwindCont()` | Allocate a `LISTSXP` (`CONS(R_NilValue, R_NilValue)`, matching GNU R `TYPEOF == 2` so `cpp11` `SETCAR(token, R_NilValue)` succeeds). Its Rust payload stack (`Vec<UnwindCont>`) lives in `UnwindConts` keyed by `SexpId` and is traced through `CtxExt`. |
| `R_UnwindProtect(fun, data, cleanfun, cdata, cont)` | See below. |
| `R_ContinueUnwind(cont)` | Take the `Unwind` from the token, store it as pending, then call `roxide_jump()`. |
| Rust panic inside capi | Caught in `run` and converted to `Unwind::Internal` with an R error `internal error in Roxide C API: <msg>`. Exported functions are `extern "C"` (no unwind), so any panic that escapes aborts. It never causes undefined behaviour. |

`R_UnwindProtect` has the cpp11 and Rcpp `longjmp` contract:
1. Call `fun` through T1.
2. On `OK`, call `cleanfun(cdata, FALSE)` through a trampoline and return the result.
3. On `JUMPED`, move the pending `Unwind` into `cont` **and fully restore state as if returning**: protect mark, TLS mode, target stack.
4. Then, from a POF tail, call `cleanfun(cdata, TRUE)` **with no trampoline**. cpp11's `cleanfun` itself jumps over our POF frame into its own `jmp_buf`, then throws `unwind_exception`, and later calls `R_ContinueUnwind`.
5. If `cleanfun` returns instead, call `R_ContinueUnwind(cont)` ourselves.

### 2.4 Thread-local `ApiCx` modes (reentrancy without aliasing undefined behaviour)

```rust
enum Mode {
    Full(*mut Ctx),        // .Call, finalizers, callbacks: everything is allowed
    Session(*mut RSession), // Session access permits allocation.
    Heap(*const RHeap),     // Heap access permits reads.
    Released,               // C is running; the next API call re-enters with the published mode
}
```

Rules:

- **Publish and restore.** `call_foreign(ctx, …)` publishes a pointer derived from the caller's current `&mut Ctx` reborrow, sets `Released`, calls the trampoline, and restores the previous mode. The Rust caller sits above the setjmp point, so a jump never skips the restore.
- **Per-call reborrow.** Each API call does `with_cx(min_mode)`: it reborrows `&mut *p` (or `&*p`) for the duration of that call only, and sets `Borrowed`. A second reborrow while `Borrowed` means a capi bug. It aborts with a diagnostic and never aliases.
- **Callbacks into C.** An API function that calls back into C (for example `Rf_eval` → R → `.Call`) re-publishes from its **own** reborrow through `call_foreign`. Borrows therefore nest correctly under Stacked Borrows.
- **Mode minimum.** Each manifest row declares the `min_mode` for its C API function.
- **Foreign ALTREP callbacks.** The evaluator calls `NativeHooks::altrep(&mut Ctx, AltOp)` before it borrows a `VecView`. All C ALTREP callbacks run inside `FrameKind::Method` with `Mode::Full`. A live `VecView` borrow prevents the mutable context borrow that the hook requires. The callback path needs no separate `Heap` or `Session` ceiling.
- **Process singleton (ADR-0019).** Exactly one capi-enabled `Ctx` exists per process. After the first `dyn.load`, `NativeHooks` binds to that session. Another `Ctx` that calls `dyn.load` gets an error. **Compiled `R CMD INSTALL` uses subprocesses for the build and test-load steps** (§3.5). GNU R does the same.

### 2.5 Finalizers and weak references (W0 `roxide-gc` contract)

- Add `Heap::register_finalizer(key, FinalizerRef, onexit)`. `FinalizerRef` is either an R closure (`SexpId`) or a C function (`usize` code pointer plus a DLL handle).
- **Mark phase:** after the roots are marked, iterate to a fixpoint for ephemerons. A `WEAKREF` value is marked only if its key is marked. Keys that are dead but have a finalizer are resurrected and queued. `Marked::sweep` returns the queue.
- **Running finalizers:** `Ctx::safepoint` stores the queue. The queue drains **only at evaluator safepoints**, never inside capi allocator safepoints, so C code never sees reentrant finalizers in the middle of an allocation. C finalizers run through `NativeHooks::run_c_finalizer`, which uses a trampoline. `onexit = TRUE` finalizers run at session end. `R_RunPendingFinalizers` drains the queue on request.
- **External pointers:** `R_MakeExternalPtr`, `R_ExternalPtrAddr`, `R_ExternalPtrTag`, `R_ExternalPtrProtected`, the setters, `R_ClearExternalPtr`, `R_MakeExternalPtrFn` and `R_ExternalPtrAddrFn` map onto `ExtPtrData`. Add accessors to `roxide-value`. Creating a raw pointer is safe Rust, so `roxide-value` keeps `forbid(unsafe_code)`.

### 2.6 Threads

- The `R` API runs on the main thread only. An API call from a thread with no published mode prints `R API called from a non-R thread (<fn>)` and aborts. GNU R silently corrupts memory in this case.
- **Read-only worker access.** While the main thread is `Released` inside a foreign call, `ACTIVE_HEAP: AtomicPtr<RHeap>` is published. OpenMP or pthread workers may call `heap`-mode read accessors such as `INTEGER`, `REAL`, `STRING_ELT`, `LENGTH` and `TYPEOF`. data.table and similar packages do this. All threads only read, and the main thread cannot allocate while it is blocked in C, so there is no data race. This requires one audited `unsafe impl Sync` wrapper in capi.

## 3. Dynamic loading and the compilation pipeline

### 3.1 Link model (ADR-0020)

**Problem.** Rust `cdylib` export lists hide C symbols from linked static libraries (`Rf_error` and the other variadic functions). Also, a `bin` that statically links the engine and then `dlopen`s `libR` would run two engines.

**Decision: final link with the system linker, driven by the manifest.**

1. `cargo build -p roxide-libr --release` builds `libroxide_libr.a`, a staticlib that bundles the `cc`-built shim objects.
2. `scripts/stage_rhome.sh` links the final library:
   - macOS: `cc -dynamiclib -o $R_HOME/lib/libR.dylib -Wl,-force_load,libroxide_libr.a -Wl,-exported_symbols_list,libR.exp -install_name $R_HOME/lib/libR.dylib -framework Accelerate`
   - Linux: `cc -shared -Wl,--whole-archive … -Wl,--no-whole-archive -Wl,--version-script=libR.map -Wl,-soname,libR.so`
   - `libR.exp` and `libR.map` are generated from the manifest.
3. `bin/exec/R` is built from `launcher.c`, linked `-L$R_HOME/lib -lR -Wl,-rpath,$R_HOME/lib`. `bin/R` and `bin/Rscript` are small shell front-ends that set `R_HOME`. This needs no Rust launcher crate and no `dlopen` of `libR`.
4. The install name is **absolute** and is set at stage time. This matches GNU R's framework model. Packages link `-lR`, and dyld or ld.so then deduplicates to the one loaded image.
5. The development binary `cargo run -p roxide-cli` keeps working without the C API (`NativeHooks = None`). If you call `.Call`, `dyn.load` or install a package with `src/` from that binary, you get this error: `compiled code requires the staged Roxide R_HOME; run scripts/stage_rhome.sh`. The harness switches to `$ROXIDE_HOME/bin/Rscript` for Layer-3/4 C API suites only.

### 3.2 `NativeHooks` (coordinator, `roxide-eval/src/api.rs`)

```rust
pub trait NativeHooks {
    fn dyn_load(&self, ctx: &mut Ctx, path: &str, local: bool, now: bool, call: SexpId) -> EvalResult<SexpId>; // DLLInfo
    fn dyn_unload(&self, ctx: &mut Ctx, path: &str, call: SexpId) -> EvalResult<()>;
    fn resolve(&self, ctx: &mut Ctx, name: SexpId, package: Option<SexpId>, kind: NativeKind, call: SexpId) -> EvalResult<NativeRoutine>;
    fn invoke(&self, ctx: &mut Ctx, r: NativeRoutine, kind: NativeKind, args: &[(Option<Sym>, SexpId)],
              call: SexpId, env: SexpId, opts: DotCOpts) -> EvalResult<SexpId>;
    fn loaded_dlls(&self, ctx: &mut Ctx) -> EvalResult<SexpId>;
    fn registered_routines(&self, ctx: &mut Ctx, dll: SexpId) -> EvalResult<SexpId>;
    fn run_c_finalizer(&self, ctx: &mut Ctx, f: CFinalizer, obj: SexpId) -> EvalResult<()>;
}
// NativeKind = Call | External | External2 | C | Fortran; NativeRoutine = Copy handle (u64) into capi's registry.
// Ctx::set_native_hooks(&'static dyn NativeHooks), mirroring s4_hooks.
```

### 3.3 `dyn.load` and registration

- **Loading.** `dyn.load(x, local = TRUE, now = TRUE)` canonicalizes the path through `Host`, runs the foreign-binary check (§3.6), then calls `libloading` `open(RTLD_LOCAL|RTLD_NOW)`.
- **Init function.** The init symbol is `R_init_<name>`, where `<name>` is the basename without extension and with `.` replaced by `_` (for example `data.table` → `R_init_data_table`). It runs through a trampoline and receives an opaque `DllInfo*`, which is a capi-owned `Box` registry entry.
- **Registration calls.** `R_registerRoutines` copies the 4 tables, enforcing `numArgs` ≤ 65 and `-1` for variable arguments. `R_useDynamicSymbols(dll, FALSE)` disables `dlsym` fallback. `R_forceSymbols(dll, TRUE)` rejects string lookup.
- **Symbol lookup order:** a `NativeSymbolInfo` or external pointer argument (fast path), then the registered table for `PACKAGE`, then `dlsym` if dynamic symbols are allowed. `.Fortran` looks up the lowercase name with a trailing `_`.
- **`useDynLib(pkg, .registration = TRUE, .fixes = "C_")`** creates namespace variables. Each variable is a `list(name, address, dll, numParameters)` with class `c("CallRoutine", "NativeSymbolInfo")`. `address` is an `EXTPTRSXP` with class `"RegisteredNativeSymbol"`.
- **R-level functions:** `getNativeSymbolInfo`, `getDLLRegisteredRoutines`, `is.loaded`, `getLoadedDLLs` (`DLLInfoList`), `library.dynam`, `library.dynam.unload` and `.dynLibs`, with print methods that match the oracle byte for byte.
- **Cross-package calls.** `R_RegisterCCallable` and `R_GetCCallable` use a map `(pkg, name) → fn ptr`. `R_GetCCallable` loads the target package's namespace on demand, as R does. rlang, vctrs and xts/zoo need this.
- **Unload.** `dyn.unload` calls `R_unload_<pkg>` through a trampoline, removes the registry entries and calls `dlclose`. pkgload reloads need a fresh image. Before `dlclose`, scan C finalizers and external-pointer finalizers whose code pointer lies in the image range (`dladdr`). Run `onexit` finalizers first, otherwise raise an error. This prevents dangling finalizer calls.

### 3.4 Argument marshalling for the native call interfaces

| Interface | Rule |
|---|---|
| `.Call(.NAME, …)` | Up to 65 `SEXP` arguments. The trampoline protects them. The result must be a valid `SEXP`. A NULL return becomes the R error `'.Call' returned NULL`. |
| `.External(.NAME, …)` | The routine receives one `LISTSXP` whose first element is `.NAME`. |
| `.External2` | The routine receives `(call, op, args, rho)`. |
| `.C`, `.Fortran` | Copy each argument into a C buffer:<br>- logical → `int*`<br>- integer → `int*`<br>- double → `double*`<br>- complex → `Rcomplex*`<br>- character → `char**` (copied; `.Fortran` gets one 255-byte string)<br>- raw → `unsigned char*`<br>- list → `SEXP*`<br>Return a named list of the copies. `NAOK = FALSE` rejects NA, NaN and Inf with R's exact message (`NA/NaN/Inf in foreign function call (arg N)`). `PACKAGE=` is stripped. `DUP` is ignored. **Hardening:** 64-byte canaries around each buffer. A damaged canary raises `array over-run in .C("<name>") argument N` (exact text from an oracle probe). |

### 3.5 `R_HOME` staging and the `R CMD SHLIB`, `config` and `INSTALL` commands

`scripts/stage_rhome.sh` builds `$ROXIDE_HOME = target/rhome/` with:
- `bin/{R,Rscript,exec/R}`
- `lib/libR.{dylib,so}`
- `include/`, copied from `roxide-capi/include`
- `etc/Makeconf`, generated (see below)
- `share/make/shlib.mk`, written by us under MIT OR Apache-2.0. Never copy GNU R's `.mk` files.
- `library/` base package stubs (as in M3)

`etc/Makeconf` uses WRE-documented variable names:

```
CC = clang -arch arm64          CXX = clang++ -arch arm64 -std=gnu++20
CXX11..CXX26 = clang++ -arch arm64 -std=gnu++NN     CXX_STD selects the flags
CFLAGS = -Wall -g -O2           CPICFLAGS = -fPIC  CXXPICFLAGS = -fPIC
SHLIB_LD = $(CC)  SHLIB_CXXLD = $(CXX)  SHLIB_EXT = .so
SHLIB_LDFLAGS = -dynamiclib -Wl,-headerpad_max_install_names -undefined dynamic_lookup
LIBR = -L"$(R_HOME)/lib" -lR    R_XTRA_CPPFLAGS = -I"$(R_INCLUDE_DIR)" -DNDEBUG
BLAS_LIBS = -framework Accelerate      LAPACK_LIBS = -framework Accelerate   (Linux: -lopenblas or -lblas -llapack)
FC = (empty unless gfortran is found)  SHLIB_OPENMP_*FLAGS = (empty on Apple clang)
```

**BLAS/LAPACK decision.** Packages link the system BLAS and LAPACK: Accelerate on macOS (it exports the Fortran-ABI symbols such as `dgemm_`) and OpenBLAS (BSD) on Linux.

- **Why:** this avoids 543 Fortran-ABI exports now. The engine itself keeps the bit-parity `roxide-math` path from ADR-0009.
- **Known deviation:** GNU R defaults to reference `libRblas`, so package results can differ at the ULP level.
- **Later:** an optional `libRblas`/`libRlapack` that exports Fortran-ABI wrappers over `roxide-math`, for bit parity.

`R CMD SHLIB` (`roxide-pkg/src/shlib.rs`) runs:

```
make -f $R_HOME/etc/Makeconf [-f src/Makevars] -f $R_HOME/share/make/shlib.mk SHLIB=<pkg>.so OBJECTS=<auto>
```

- `<auto>` means all `*.c`, `*.cc`, `*.cpp`, `*.f`, `*.f90`, `*.f95` and `*.m` files, unless `Makevars` defines `OBJECTS`.
- `src/Makefile` takes precedence over the generated invocation.
- The command honours `PKG_CPPFLAGS`, `PKG_CFLAGS`, `PKG_CXXFLAGS`, `PKG_LIBS`, `CXX_STD` and `USE_FC_TO_LINK`.
- `make` is a system program. It runs through the `Host` process API, so `roxide-pkg` keeps `forbid(unsafe_code)`.

`R CMD config VAR`, `--cppflags` and `--ldflags` answer from `Makeconf`. `configure` scripts need these values.

Compiled `R CMD INSTALL` (an extension of `install.rs`):
1. Unpack into a `tempfile` stage directory.
2. Run `configure` with `R_HOME`, `R_PACKAGE_NAME`, `R_PACKAGE_DIR`, and `PATH` starting with `$R_HOME/bin`.
3. Run SHLIB, then copy the result to `<lib>/<pkg>/libs/<pkg>.so`.
4. Build the lazy-load DB **in a child `bin/R` process**, because loading the namespace runs `dyn.load`.
5. Test-load in another child process.
6. Run `cleanup` if present.
7. Write `Meta/Roxide.rds` with the fields `capi_abi`, `roxide_version` and `built`. This changes no visible R output.
8. If no Fortran compiler is present, a package with Fortran sources fails with `Fortran compiler not found (FC is empty); install gfortran`.

### 3.6 Refusing foreign binaries

A loader check (`roxide-pkg/src/loader/dynlib.rs`) runs for each compiled package:
- If `Meta/Roxide.rds` is missing, raise `package 'x' was installed by a different R implementation; reinstall it from source with Roxide`.
- `dyn.load` also parses Mach-O `LC_LOAD_DYLIB` or ELF `DT_NEEDED` entries with a small pure-Rust parser. It is a fuzz target. Any `R.framework` or `libR` dependency that is not our own `libR` gets the same refusal.

**Why:** this keeps the clean-room runtime rule ("Roxide never invokes GNU R at runtime") and prevents two runtimes in one process.

## 4. Tier-3 compiled CRAN corpus ladder

W0 must confirm the symbol lists below with `symbol_census.py` (`nm -u` on oracle-header builds). The lists show expected needs. Packages marked "black-box only" are GPL. You may build and test them, but never vendor them, read their source, or derive code from them.

| Tier | Packages (licence) | New API needed beyond the tiers above it |
|---|---|---|
| **3a: core C** | `fastmap` (MIT, C++), `glue` (MIT), `utf8` (Apache-2.0), `magrittr` (MIT), `jsonlite` (MIT; bundles yajl ISC), `yaml` (BSD-3; libyaml MIT) | `Rf_allocVector`, `Rf_protect`, `Rf_unprotect`, `Rf_mkChar{,CE,Len,LenCE}`, `R_CHAR`, `STRING_ELT`, `SET_STRING_ELT`, `VECTOR_ELT`, `SET_VECTOR_ELT`, `INTEGER`, `REAL`, `LOGICAL`, `RAW`, `LENGTH`, `XLENGTH`, `Rf_xlength`, `TYPEOF`, `Rf_getAttrib`, `Rf_setAttrib`, `Rf_install`, `Rf_error`, `Rf_warning`, `Rf_as{Integer,Logical,Real,Char}`, `Rf_Scalar*`, `Rf_mkString`, `Rf_is*`, `Rf_inherits`, `Rf_coerceVector`, `Rf_duplicate`, `Rf_shallow_duplicate`, `Rf_eval`, `Rf_lang1..6`, `Rf_cons`, `Rf_lcons`, `CAR`, `CDR`, `CADR`, `CADDR`, `SETCAR`, `SET_TAG`, `TAG`, `R_registerRoutines`, `R_useDynamicSymbols`, `R_forceSymbols`, `R_CheckUserInterrupt`, `Rprintf`, `REprintf`, `R_alloc`, `vmaxget`, `vmaxset`, `R_chk_calloc`, `R_chk_free`, `Rf_translateCharUTF8`, `Rf_getCharCE`, `R_MakeExternalPtr`, `R_ExternalPtrAddr`, `R_ClearExternalPtr`, `R_RegisterCFinalizerEx`, `R_PreserveObject`, `R_ReleaseObject`, `Rf_allocMatrix`, `Rf_nrows`, `Rf_ncols`, `Rf_defineVar`, `R_ParseVector`; data: `R_NilValue`, `R_NaString`, `R_BlankString`, `R_NaInt`, `R_NaReal`, `R_GlobalEnv`, `R_BaseEnv`, `R_EmptyEnv`, `R_NamesSymbol`, `R_ClassSymbol`, `R_DimSymbol`, `R_RowNamesSymbol` |
| **3b: tidyverse C** | `rlang` (MIT), `vctrs` (MIT), `cli` (MIT; uses a ticker thread), `purrr` (MIT), `tibble` (MIT), `lobstr` (MIT, cpp11) | `R_UnwindProtect`, `R_MakeUnwindCont`, `R_ContinueUnwind`, `R_ToplevelExec`, `R_tryCatchError`, `R_withCallingErrorHandler`, `R_GetCCallable`, `R_RegisterCCallable`, `R_getVar`, `R_getVarEx`, `R_existsVarInFrame`, `R_removeVarFromFrame`, `R_NewEnv`, `R_ParentEnv`, `R_Closure{Formals,Body,Env,Expr}`, `R_mkClosure`, `R_BytecodeExpr`, `R_BindingIsActive`, `R_ActiveBindingFunction`, `R_MakeActiveBinding`, `R_lsInternal3`, `R_getAttribCount`, `R_getAttribNames`, `R_getAttributes`, `R_GetCurrentEnv`, `Rf_GetOption1`, `MAYBE_REFERENCED`, `MAYBE_SHARED`, `NO_REFERENCES`, `ALTREP`, `DATAPTR_RO`, `DATAPTR_OR_NULL`, `*_GET_REGION`, `*_IS_SORTED`, `*_NO_NA`, `Rf_allocS4Object`, `R_do_slot`, `R_do_slot_assign`, `R_has_slot`, `IS_S4_OBJECT`, `OBJECT`, `Rf_any_duplicated`, `Rf_match`, `R_compute_identical`, `Rf_type2char`, `Rf_str2type`, `R_ExternalPtrTag`, `R_ExternalPtrProtected`, `R_PreserveInMSet`, `R_ReleaseFromMSet`; plus the `compat` rows that the census finds |
| **3c: C++ and ALTREP** | `cpp11` (MIT), `vroom`/`readr` (MIT; C ALTREP classes), `processx`/`ps` (MIT; OS-heavy), `Rcpp` (GPL, black-box only), `data.table` (MPL-2.0; OpenMP is off on Apple clang) | `R_make_alt{integer,real,logical,string,raw,list}_class`, all `R_set_alt*_method` setters, `R_new_altrep`, `R_altrep_data1`, `R_altrep_data2`, `R_set_altrep_data1`, `R_set_altrep_data2`, `R_altrep_inherits`, `ALTREP_CLASS`; resizable API (`R_allocResizableVector`, `R_resizeVector`, `R_isResizable`, `R_maxLength`); `Applic.h` optimizers; Rcpp's `R_GetCCallable("Rcpp", …)` table |
| **Stretch** | `stringi` (BSD-3 + ICU; heavy compile), `Matrix` (GPL, black-box only; needs LAPACK and SuiteSparse), `digest` and `htmltools` (both GPL: black-box only; they were in the brief's list, but they are not permissive) | Graphics engine API (M5), embedding (`Rf_initEmbeddedR`) |

Store the corpus pins in `tests/corpus/tier3.tsv` with the columns `pkg`, `version`, `sha256`, `licence`, `blackbox_only` and `tier`. Download tarballs into a `tempfile` directory, never into the repository.

## 5. Bytecode VM (`roxide-eval::vm`) and performance

**Decision (ADR-0020): Roxide never executes GNU R bytecode instruction streams.**

Reasons:
1. The `BCODESXP` opcode set is an internal, version-coupled (`bcVersion`) interface. Its only specification is GPL `compiler` package code.
2. The ADR-0014 lowering already rebuilds exact semantics from `consts[0]`.
3. A Roxide-native IR gives the same speed with no reverse engineering.

Data flow: `BCODESXP` read from an `.rdb` or `.rds` file → `consts[0]` AST → Roxide IR → VM. Roxide writes plain closures. GNU R reads them and compiles them again with its own JIT, so interop is preserved.

Design:
- **Layout:** `crates/roxide-eval/src/vm/{mod,ir,compile,exec,guard,cache}.rs`. The register IR uses registers only for **temporaries**. Variables stay in the real `ENVSXP`, with binding-slot inline caches. This keeps `eval`, `assign`, `sys.function`, `environment()`, `missing()` and `on.exit` exactly correct.
- **Promises:** promises keep the **AST** sub-expression as their code, so `substitute()` and `match.call()` do not change.
- **Guards:** each `ENVSXP` gets a `shape_version: u32`, incremented when a binding is added or removed. A call-site cache stores `[(env, version)…]` plus the resolved function `SexpId`. On a cache hit that resolves to a base builtin (`+`, `[`, `[[<-`, `length`, `c`, `seq_along`, …), the VM uses the fast path. Any guard failure falls back to the AST interpreter for that call. The AST interpreter stays the semantic reference.
- **Fast paths:**
  - scalar `double` and `int` arithmetic and comparison on unclassed operands without attributes, with NA and overflow rules from ADR-0005 (no allocation)
  - `for` over `seq_len`, `seq_along` and `:` with an unboxed counter (compact ALTREP is never materialized)
  - in-place `x[i] <- v` and `x[[i]] <- v` when `refcnt == 1` (no `*tmp*` copy)
  - `if` with a scalar condition
  - local `break`, `next` and `return` as direct jumps
- **JIT:** `enableJIT(level)` returns the previous level. The default is 3, and the `R_ENABLE_JIT` environment variable sets it. W0 confirms the exact compile trigger (first or second call, and size heuristics) with a black-box probe that prints `f` after N calls.
- **Printing parity:** a compiled closure prints the `<bytecode: 0x…>` line. The harness normalizes the address.
- **`compiler` namespace (clean-room, `roxide-base/src/compiler/`):** `cmpfun`, `compile`, `enableJIT`, `compilePKGS`, `setCompilerOptions`, `getCompilerOption`, `disassemble` (prints the Roxide IR; the output is documented to differ from GNU R).
- **Differential gate:** run the full Layer 1–3 suites with `ROXIDE_JIT=0` and `ROXIDE_JIT=3`. Outputs must be identical, including under GC torture.
- **SIMD:** NA-free fast loops use `chunks_exact`, which LLVM auto-vectorizes. IEEE NaN payload propagation already keeps `NA_real_` on arm64 and x86-64. Integer operations use widened `i64` checks without branches, so overflow gives `NA` plus the warning.
- **Benchmarks (`tools/bench/`, a Python driver, no Cargo slot for the oracle side):** startup latency, scalar `for` sum 1e7, `fib(25)`, vector arithmetic 1e7, `lm(mtcars)` ×1000, `sort` 1e6, peak RSS, and wall time under GC torture. Record a baseline against oracle `Rscript` in `docs/BENCH.md`.
- **Startup:** serialize the base environment prelude into an `.rdb` at stage time and lazy-load it.

## 6. Fuzzing and hardening

- **`fuzz/`**: `cargo-fuzz` on nightly. The directory is excluded from the workspace and from the gate. Run one target at a time, and count it as a Cargo slot while it builds. Use `-rss_limit_mb=2048 -timeout=10 -max_len=65536`.
- **Targets:**
  - `parse` and the parse→deparse→parse round trip
  - `unserialize` (XDR, ASCII and native, with gz, bz2, xz and zstd)
  - lazy-load `.rdx`/`.rdb`
  - `read.dcf`, `read.table`, the `DESCRIPTION` and `NAMESPACE` parsers
  - regex translation (TRE and PCRE dialects)
  - `ustar` extraction (path traversal and symlink escape)
  - the Mach-O/ELF dependency parser
  - `sprintf` and `format` specifications, numeric parsing
  - **C API sequence fuzzer:** a C harness that makes random valid API call sequences under GC torture. The only allowed failure is a stale-handle error on objects it deliberately left unprotected.
- **Stable gate:** `tests/fuzz_regressions.rs` replays every crash reproducer and seed corpus file in compact pack files.
- **Hardening checklist:**
  - decompression limits on both ratio and absolute size
  - unserialize depth limit (already present) plus a vector-length ceiling (`R_XLEN_T_MAX` = 2^52)
  - the `expressions` option limit and the `stack_budget` C-stack guard, applied to `Rf_eval` reentry
  - `.C` canaries
  - NULL and stale `SEXP` diagnostics
  - abort on a non-main-thread API call
  - refusal of foreign binaries
  - `dyn.load` with canonical paths only
  - exact protect-stack limits

## 7. Proposed ADRs

- **ADR-0017: C ABI representation and headers.**
  - `SEXP` = `SexpId` bits: opaque, never dereferenced, 64-bit only. C NULL raises an R error.
  - Well-known symbols and objects are permanent, so all data globals are link-time constants.
  - Data pointers are shared without copying. `roxide-gc` is non-moving forever.
  - Headers are clean-room, written from WRE. The manifest comes from `funAPI` plus `nm`. `abi_probe` compares the two header sets numerically.
  - Graphics and embedding headers are deferred.
- **ADR-0018: Foreign control transfer.**
  - C++ setjmp trampolines with a `catch(...)` wall.
  - The POF-by-construction `boundary()`.
  - Pending-unwind slots indexed by trampoline depth.
  - Every Rust→C call goes through a trampoline.
  - `R_UnwindProtect` hands `cleanfun` a POF tail. The continuation token is an `EXTPTRSXP`.
  - Rust panics never cross a boundary, and `C-unwind` is never used.
- **ADR-0019: Reentrancy, GC and threads.**
  - TLS `Mode {Full, Session, Heap, Released}` with per-call reborrow and a double-borrow abort.
  - Allocating API functions protect their arguments and then act as safepoints.
  - Trampolines restore the protect mark. Stack imbalance gives a warning.
  - Finalizers and ephemerons in `roxide-gc`. Finalizers drain only at evaluator safepoints.
  - One capi session per process. Compiled installs use subprocesses.
  - Main-thread API, with read-only worker access while the main thread is `Released`.
  - The foreign ALTREP single-authority rule.
- **ADR-0020: Build, link, packages and VM.**
  - `libR` = staticlib plus a system-linker final link with a manifest export list and an absolute install name.
  - Staged `R_HOME`, a generated `Makeconf` and our own `shlib.mk`.
  - System BLAS and LAPACK for packages.
  - The `Meta/Roxide.rds` stamp and refusal of foreign binaries.
  - New dependencies: `cc`, `libloading`, and `cargo-fuzz` (nightly only, outside the workspace).
  - The VM uses its own IR, never executes GNU R `BCODESXP`, uses guard-based fallback, and keeps the AST interpreter as the semantic reference.

## 8. Wave plan and file ownership

Run three lanes. Lanes C and V share the global limit of **2 Cargo slots** (`CARGO_BUILD_JOBS=2`, `RUST_TEST_THREADS=3`). Header writing, ABI probes, census, corpus triage and benchmark scripting need **no Cargo slot**, so they keep the fleet busy.

### M4 W0: contracts (coordinator, serial, 1 slot, Opus review)

- **Workspace:** add `roxide-capi` and `roxide-libr`. Add the `cc` and `libloading` dependencies. Add the `roxide-cli` library target.
- **`roxide-gc`:**
  - `unprotect_ptr`
  - protect depth limit
  - finalizer registry and ephemeron marking
  - `Marked::sweep` returns the finalizer queue
- **`roxide-value`:**
  - extend the permanent table (well-known symbols, `R_BlankScalarString`, `R_NamespaceRegistry`, `R_MethodsNamespace`)
  - raw data-pointer accessors (`data_ptr_ro`, `data_ptr_rw` → `(usize, len)`)
  - external pointer and `gp`/`LEVELS` accessors
  - `alloc_resizable`
  - `AltClass::foreign()`
- **`roxide-eval`:**
  - `NativeHooks` and `set_native_hooks`
  - public `signal_error_at`, `warning_at` and interrupt API
  - finalizer drain at safepoints
  - `ENVSXP` `shape_version`
  - `jit_level` plumbing
  - empty `vm/` module
- **Tools:** `tools/capi/{api_manifest,abi_probe,check_exports,symbol_census}.py`, `scripts/stage_rhome.sh`, and the licence-scan rule for `include/`. Update `PRIOR_ART.md`, `THIRD_PARTY_NOTICES.md` and ADR-0017…0020.
- **Probes:** exact error texts (protect overflow and underflow, `INTEGER()` on the wrong type, `.C` over-run and NAOK, stack imbalance, `'.Call' returned NULL`, error truncation length) and the JIT compile trigger.

### M4 W0b: capi core (coordinator with Opus, then Sol review)

Files: `roxide-capi/{build.rs, csrc/*, src/abi.rs, cx.rs, jump.rs, globals.rs}`.

Exit test: a C fixture calls `Rf_error` from three nesting levels, `R_UnwindProtect` from cpp11-style C++ code, and a thrown C++ exception. All three paths must work under GC torture.

### M4 W1: API families (4 parallel Sol workers)

| Worker | Owns (disjoint) | Slot |
|---|---|---|
| A | `roxide-capi/src/api/{vector,attrib,coerce,pairlist,string,sym,resizable}.rs` | yes |
| B | `roxide-capi/src/api/{env,eval,error,print,unwind,parse}.rs` (Opus review: high risk) | yes |
| C | `roxide-capi/src/api/{memory,extptr,weakref,rmath,random,arith,utils,s4}.rs` | after A or B |
| D | `roxide-capi/include/**` and the C side of the ABI probe | **no** (clang only) |
| V1 (lane V) | `roxide-eval/src/vm/{ir,compile}.rs` (Astra consult on guards) | after C |

### M4 W2: loading and the build pipeline

| Worker | Owns |
|---|---|
| E | `roxide-capi/src/{dll,register,dotcall,hooks}.rs` |
| F | `roxide-base/src/native/**` (`.Call`/`.External`/`.C`/`.Fortran` primitives, the `dyn.load` family, print methods) and its registration line in `roxide-base/src/lib.rs` (coordinator applies it) |
| G | `roxide-pkg/src/{shlib.rs, makeconf.rs, install_compiled.rs, loader/dynlib.rs, binfmt.rs}`, `share/make/shlib.mk`, `roxide-cli/src/rcmd/{shlib,config}.rs` |
| H | `crates/roxide-libr/**`, `csrc/launcher.c`, stage-script final link, `check_exports` gate |
| V2 | `roxide-eval/src/vm/{exec,guard,cache}.rs` |

### M4 W3: ALTREP, C++ and Tier 3a/3b

- `api/altrep.rs` (foreign classes, the 3 modes)
- cpp11 and Rcpp hardening
- `tests/capi/fixtures/` bring-up
- read-only corpus triage workers (no slot) feed bug lists back to the owners

### M4 W4: performance, fuzzing and Tier 3c

- V3: `roxide-base/src/compiler/**` plus benchmark-guided tuning
- K: `fuzz/**`, `tests/fuzz_regressions.rs`, `tools/bench/**`
- Tier-3c triage
- `Rf_initEmbeddedR` stretch

**Review pairing:**
- Every W0, W0b and lane-B slice gets Opus 5.5 review plus a fresh Sol review.
- VM guard soundness and numerical fast paths go to GPT 6.2 Astra.
- Routine families get a fresh Sol reviewer plus the fixture oracle gate.

## 9. Conformance layout and exit gate

- **Fixtures.** `tests/capi/fixtures/<family>.c` holds one compact C file per family with many routines. `tests/capi/<family>.R` is the driver. The harness builds each fixture twice:
  - oracle: `clang -dynamiclib -undefined dynamic_lookup -I$ORACLE/include`, then `dyn.load` in oracle `Rscript`
  - Roxide: staged `R CMD SHLIB`

  It compares stdout, stderr and the exit code byte for byte.
- **Negative fixtures (Roxide only):** a missing `PROTECT` under torture must give a deterministic stale-handle error. A thread API call must abort with the diagnostic. A C++ escape must become an R error.
- **Exit gate (M4 done):**
  1. The full `scripts/gate.sh`, plus the `check_exports` and `abi_probe` diffs, are empty.
  2. 100% fixture parity, also with `ROXIDE_GC_TORTURE=1`.
  3. Every Tier-3a package installs from source and passes its `tests/`.
  4. Tier-3b: ≥ 90% of test files pass. Every failure is triaged.
  5. Layer 1–3 output is identical with `ROXIDE_JIT=0` and `ROXIDE_JIT=3`.
  6. Each fuzz target runs 1 h with zero crashes. The regression replay is green.
  7. `docs/BENCH.md` holds the baseline against GNU R 4.6.2.

## 10. Decisions and blockers for the coordinator

1. **Oracle `R CMD`:** the relocated framework breaks `R CMD config` and `R CMD SHLIB`. The fixture workaround in §9 avoids this. To get full oracle `R CMD INSTALL` parity, choose one of these options:
   - (a) A symlink `/Library/Frameworks/R.framework` → `env/oracle/R.framework`. This needs `sudo`, which only the user can give.
   - (b) Install the official pkg normally.
2. **`abi_sigdiff.py`:** a clang-AST signature diff against the oracle headers, where agents see only mismatches. Approve or reject this tool.
3. **GPL corpus packages** (`Rcpp`, `digest`, `htmltools`, `Matrix`): confirm the black-box-only policy, or drop them.
4. **Permanent-table extension** (ADR-0002 amendment): confirm that no code assumes the permanent count is 13.
