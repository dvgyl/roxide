# Milestone 3 architecture blueprint and parallel wave plan

I worked in read-only mode for this blueprint. I edited no files, ran no Cargo commands, and read no GNU R source code. The plan uses the oracle spec, the docs, and the current contract crates.

## 0. What the code already has

Four facts in the code make the W0 scope smaller than the brief expects:

| Item | Current state | Effect on M3 |
|---|---|---|
| `SexpType::Obj = 25` | Already exists. `type_name(is_s4)` returns `"S4"` or `"object"`. [object.rs:62](/Users/devg/Documents/Roxide/crates/roxide-value/src/object.rs:62) | Do not add a new type. Rename the docs to "`OBJSXP` with the S4 bit". |
| S4 flag | `ObjFlags { object, s4 }` already exists. `ValueApi::is_s4` and `set_s4_flag` exist. The XDR read and write paths keep the S4 bit. | The flag is already independent of the storage type, so `.Data` vectors already work at the representation level. |
| `S4Hooks` | A trait with one method (`dispatch`). It is used only for primitive dispatch. [dispatch.rs:331](/Users/devg/Documents/Roxide/crates/roxide-eval/src/dispatch.rs:331) | Extend the trait. Do not replace it. |
| Namespaces | `RegisteredPackage { name: &'static str, namespace, attached }`. The namespace's enclosing environment is `BASE_ENV`. `::` uses hard-coded `utils` `head`/`tail` special cases. S3 lookup loops over all package namespaces. [dispatch.rs:216](/Users/devg/Documents/Roxide/crates/roxide-eval/src/dispatch.rs:216) | Replace this. The lexical chain is wrong for M3 (`.GlobalEnv` is not reachable), and the registry is static only. |

There are also two gaps that the brief did not name:

- **Serialization:** the XDR decoder has no `PERSISTSXP` (247) reference-hook path and no `BCODESXP` (21) path. Also, `decode` is `pub(super)`. Lazy-load needs all three.
- **Tier-2 corpus:** **`magrittr` ≥ 2.0 and `testthat` contain compiled code** (`NeedsCompilation: yes`). `testthat` also depends on compiled packages (`rlang`, `cli`, `brio`, and others). Thus neither package can pass a pure-R gate in M3. Section 4.3 gives the resolution.

---

## 1. Core contract changes (coordinator-owned M3 W0)

### 1.1 `roxide-gc`

Make no change. The S4 bit lives in `RObject.flags`, which `roxide-value` owns.

### 1.2 `roxide-value`

1. **Pseudo-NULL slot sentinel.** Add `pub const SEXP_S4_NULL: SexpId = SexpId::permanent(12)`. This is a `SYMSXP` with the name bytes `b"\x01NULL\x01"`. Allocate it after `NA_LGL_SCALAR` in `RSession::new`, and put it into the symbol table so that `intern("\u{1}NULL\u{1}")` returns it. Serialization then writes it as an ordinary symbol, which matches the oracle's `attributes(a)`.
2. **`ValueApi::alloc_s4_object(&mut self) -> Result<SexpId, ValueError>`**: allocates `Body::Obj` with `flags.s4 = true`.
3. **Preserve the S4 bit:** `duplicate`/`unshare` must copy `flags.s4`. `identical` must compare it. Audit [duplicate.rs:33](/Users/devg/Documents/Roxide/crates/roxide-value/src/duplicate.rs:33) and `identical.rs`.
4. **Package-qualified classes.** Store the `class` attribute exactly as given: a `STRSXP` that has its own `package` attribute. `class(x)` and `attr(x, "class")` must return the object with its `package` attribute. S3 code reads only `str_slice`. Add no new type.
5. **`BASE_NAMESPACE` frame alias.** `BASE_NAMESPACE` must resolve bindings through the frame of `BASE_ENV`, but its enclosing environment must be `GLOBAL_ENV`. Do this inside `env_get`/`env_define`/`env_entries`: when `env == BASE_NAMESPACE`, redirect the frame access to `BASE_ENV` and keep `enclos = GLOBAL_ENV`. This is how the chain `imports:pkg → base → .GlobalEnv` works without two copies of base.

### 1.3 `roxide-eval`

| Change | Contract |
|---|---|
| **Namespace registry** (new `src/namespace.rs`) | Replace `RegisteredPackage` with `NsRecord { name: String, ns: SexpId, imports: SexpId, attached: Option<SexpId>, path: Option<PathBuf>, builtin: bool }`. Public API on `Ctx`: `ns_lookup(&str) -> Option<SexpId>`, `ns_create(name, version, path) -> (ns, imports)`, `ns_register(ns)`, `ns_unregister(name)`, `search_attach(env, pos, name)`, `search_detach(pos) -> SexpId`, `search_path() -> Vec<SexpId>`, `set_namespace_loader(&'static dyn NamespaceLoader)`. All `SexpId`s are traced roots. |
| **Lexical chain** | `ns_create` builds `namespace:pkg → imports:pkg → BASE_NAMESPACE → .GlobalEnv → search path`. The namespace contains `.__NAMESPACE__.` (an environment that holds `spec = c(name=, version=)`, an `exports` environment, `imports`, `path`, `dynlibs`, `S3methods`) and `.__S3MethodsTable__.`. R code reads this layout (`getNamespaceExports`, `asNamespace`), so keep the names exact. |
| **Built-in packages** | `register_package` uses `ns_create`. `Package` gets a new field `exports: Option<&'static [&'static str]>`. `None` keeps the current rule (all names that do not start with `.`). The default search order must be `.GlobalEnv, stats, graphics, grDevices, utils, datasets, methods, Autoloads, base`. Thus register in this order: methods, datasets (empty if not present), utils, grDevices, graphics, stats. Register `grid` and `tools` with `attach: false`. |
| **`::` and `:::`** | Remove the `utils` special cases. Resolve through `ns_lookup`, and on a miss call `NamespaceLoader::load`. `::` checks the `exports` environment. `:::` reads the namespace frame. Neither operator attaches. |
| **S3 lookup** | Order: calling-environment chain, then the `.__S3MethodsTable__.` of the generic's defining `topenv`, then `BASE_NAMESPACE`'s table. Delete the loop over all packages. Deferred registrations: `pending_s3: Vec<DeferredS3 { provider, generic, class, fun }>` is a root. `ns_register(provider)` flushes it. |
| **`@` and `@<-` primitives** | `@` is a SPECIAL. The slot name is a symbol or a string. `.Data` returns the data part (copy, attributes stripped, S4 bit cleared). `SEXP_S4_NULL` maps to `NULL`. An absent slot gives the error `no slot of name "foo" for this object of class "A"`. `@<-` calls `S4Hooks::check_slot` before `set_attrib`. Non-S4 objects that have a class attribute dispatch `@` through S3 (R ≥ 4.3 behavior). Confirm the exact message text with the oracle. |
| **`standardGeneric` primitive** | Calls `S4Hooks::standard_generic(ctx, name, frame)`. |
| **Method invocation API** | `Ctx::apply_matched(fun, matched: &[(Sym, SexpId)], call, env, frame_vars: &[(Sym, SexpId)])`. It reuses the generic frame's promises and `SEXP_MISSING` markers. This lets `missing(y)` stay `TRUE` while the method's default applies, and it leaves non-signature arguments unforced. It also binds `.Generic`, `.Method`, `.defined`, and `.target`. |
| **S4 printing** | In `run_script` auto-print and in the `print` primitive: if `is_s4(v)` and hooks exist, call `hooks.show(ctx, v, env)`. Otherwise keep the current path. `invisible()` still suppresses auto-print. |
| **Script echo** | `run_script` takes `ScriptOpts { auto_print, echo }`. Echo writes each top-level expression's source lines with the prompts `"> "` and `"+ "`. `R CMD BATCH` uses it. |

Extended `S4Hooks` trait (each method has a default, so `roxide-eval` works without `methods`):

```rust
pub trait S4Hooks {
    fn dispatch(&self, ctx: &mut Ctx, generic: Sym, call: &Call, args: &Args) -> EvalResult<Option<SexpId>>;
    fn standard_generic(&self, ctx: &mut Ctx, name: Sym, frame: FrameId) -> EvalResult<SexpId>;
    fn show(&self, ctx: &mut Ctx, value: SexpId, env: SexpId) -> EvalResult<()>;
    /// S4 superclass chain used by S3 dispatch and inherits() on S4 instances (R_data_class2 semantics).
    fn class_chain(&self, ctx: &mut Ctx, value: SexpId) -> EvalResult<Option<SexpId>>;
    fn check_slot(&self, ctx: &mut Ctx, obj: SexpId, name: Sym, value: SexpId) -> EvalResult<SexpId>;
}
pub trait NamespaceLoader {
    fn load(&self, ctx: &mut Ctx, name: &str, call: SexpId) -> EvalResult<SexpId>;
}
```

### 1.4 `roxide-base`

The `serialize/` changes are made by W2-C, but W0 fixes the API shape now:

```rust
pub trait PersistHook { fn write(&mut self, ctx: &mut Ctx, x: SexpId) -> EvalResult<Option<SexpId /*STRSXP*/>>;
                        fn read(&mut self, ctx: &mut Ctx, key: SexpId) -> EvalResult<SexpId>; }
pub fn unserialize_bytes(ctx: &mut Ctx, bytes: &[u8], hook: Option<&mut dyn PersistHook>) -> EvalResult<SexpId>;
pub fn serialize_bytes(ctx: &mut Ctx, x: SexpId, version: u32, hook: Option<&mut dyn PersistHook>) -> EvalResult<Vec<u8>>;
```

### 1.5 Namespace, loader, and lazy-load architecture (new crate `roxide-pkg`)

Dependencies: `roxide-eval`, `roxide-base` (DCF and serialize), `roxide-host`, `flate2`, `bzip2`, `lzma-rust2`. Set `#![forbid(unsafe_code)]`. All file I/O goes through `Host`.

```
crates/roxide-pkg/src/
  lib.rs              (W0) Module registration into BASE_ENV; installs NamespaceLoader
  description.rs      DESCRIPTION read/validate (reuses roxide-base::dcf)
  nsfile.rs           NAMESPACE parser → NsSpec (directives evaluated as R exprs; if/else, braces, unknown-in-taken-branch error)
  loader.rs           loadNamespace / attachNamespace / unloadNamespace / requireNamespace
  library.rs          library / require / use / .libPaths / find.package / system.file / packageVersion
  s3reg.rs            S3method incl. pkg::generic deferred registration
  lazyload/{read.rs, write.rs, hook.rs}   .rdb/.rdx; env::N persistent references
  install.rs          R CMD INSTALL (dir or .tar.gz)
  tar.rs              ustar reader (POSIX spec) over flate2
  batch.rs            R CMD BATCH
  prelude.R           thin R wrappers (visibility, arg matching)
```

**Load sequence** (`loader.rs`). Order is important:

1. Detect cycles and raise `cyclic namespace dependency detected`.
2. Read `Meta/package.rds` and `Meta/nsInfo.rds`, or parse the source `NAMESPACE`.
3. Recursively load each import, and fill `imports:pkg` (`import(except=)`, `importFrom`).
4. Fill the namespace in one of two ways: (a) lazy-load, where each `.rdx` `variables` entry becomes a promise of `lazyLoadDBfetch(c(off, len), file, compressed, hook)`; or (b) direct source from `R/<pkg>`.
5. If `methods` is loaded, call `methods:::.rx_cacheMetaData(ns)` at R level. There is no Rust dependency on `roxide-methods`.
6. Register S3 methods, both immediate and deferred.
7. `ns_register`.
8. Run `.onLoad`.
9. Seal the namespace (lock the environment and its bindings).

**Seal timing:** the oracle says "registers and locks, runs `.onLoad`". A W1-C probe must confirm whether `.onLoad` can still `assign` into the namespace.

**Unload:** detach, run `.onDetach`, run `.onUnload`, then `ns_unregister`. Refuse the unload while another loaded namespace imports this one.

**Lazy-load format** (from the *R Internals* manual):

- `.rdx` is an RDS file that holds a list `variables`, `references`, `compressed`. Each entry is an integer pair: byte offset and length into `.rdb`.
- For `compressed = TRUE`, a record is a 4-byte big-endian uncompressed length followed by a zlib stream. For `2`/`3`, a fifth type byte selects bzip2 or xz.
- Environments travel as `PERSISTSXP` keys `env::N`. Each key resolves through `references` to a serialized list of the frame, enclosing environment, and attributes. The hook builds each environment once and then fills it, which handles cycles.

**`BCODESXP` read path** (to load packages that GNU R installed with byte-compilation): decode the bytecode record and use `consts[0]`, the original expression, as the closure body. Roxide never byte-compiles, so `INSTALL` always behaves like `--no-byte-compile`.

**`R_HOME` and library paths.**
- `R.home()` = `$R_HOME`, otherwise `<exe>/../lib/roxide`.
- `.libPaths()` = `R_LIBS`, `R_LIBS_USER`, `R_LIBS_SITE`, then `<R_HOME>/library`.
- `scripts/make_r_home.sh` (W0) creates `bin/R` and `bin/Rscript` symlinks and `library/<builtin>/DESCRIPTION` (Version `4.6.2`, Priority `base`). Then `system.file`, `packageVersion("grDevices")`, and `system2(file.path(R.home("bin"), "R"), c("CMD", "INSTALL", …))` work.

### 1.6 `roxide-graphics` crate

Set `#![forbid(unsafe_code)]`. Dependencies: `roxide-eval`, `roxide-value`, `roxide-gc`, `roxide-base`, `roxide-host`, and `flate2` (already a workspace dependency). **No new external crate.** Drop the Rust dependency on `roxide-stats`. The R-level preludes call `stats::fivenum` and `stats::IQR` at run time. This change also shortens parallel build chains.

```
crates/roxide-graphics/src/
  lib.rs                       (W0) Packages: grDevices, graphics (attach), grid (attach=false); register()
  engine/mod.rs                (W0 contract) Device trait, GContext, Rgba, Rect, DeviceTable, GraphicsExt: CtxExt
  engine/dl.rs                 display list (traced SexpId call records; replay)
  devices/pdf.rs               PDF 1.4 writer
  devices/svg.rs               SVG 1.1 writer (+ <pageSet>/<page> for onefile=TRUE)
  devices/raster/{mod,fill,stroke,png}.rs   rasterizer + PNG encoder
  devices/metrics/{helvetica_afm.rs, hershey.rs}
  grdevices/{colors.rs, palette.rs, dev.rs, ranges.rs, prelude.R}
  graphics/{par.rs, layout.rs, coords.rs, lowlevel.rs, prelude.R}
  grid/{unit.rs, viewport.rs, gpar.rs, layout.rs, draw.rs, prelude.R}
```

**Device contract.** Device units are 1/72 inch, with the origin at the bottom left and y pointing up. Engine code never passes user coordinates to a device. SVG and PNG flip the y axis when they write output.

```rust
pub trait Device {
    fn name(&self) -> &str;                       // "pdf", "svg", "png", "quartz_off_screen"
    fn size_in(&self) -> (f64, f64);
    fn new_page(&mut self, host: &mut dyn Host, bg: Rgba) -> HostResult<()>;
    fn clip(&mut self, r: Rect);
    fn polyline(&mut self, pts: &[(f64, f64)], gc: &GContext);
    fn polygon(&mut self, pts: &[(f64, f64)], gc: &GContext);
    fn path(&mut self, subpaths: &[&[(f64, f64)]], winding: bool, gc: &GContext);
    fn rect(&mut self, r: Rect, gc: &GContext);
    fn circle(&mut self, c: (f64, f64), r: f64, gc: &GContext);
    fn text(&mut self, at: (f64, f64), s: &str, rot: f64, hadj: f64, gc: &GContext);
    fn str_width(&self, s: &str, gc: &GContext) -> f64;
    fn metric_info(&self, c: char, gc: &GContext) -> (f64, f64, f64);
    fn close(&mut self, host: &mut dyn Host) -> HostResult<()>;
}
```

`GContext` holds `col`/`fill: Rgba`, `lwd`, `lty`, `lend`, `ljoin`, `lmitre`, `cex`, `ps`, `fontface`, `fontfamily`, and `lineheight`. `GraphicsExt` holds device slots 1..=63 (slot 1 is the null device), the current slot, per-device base `GPar` and grid state, and the display lists. It traces the recorded `SexpId`s through `trace_fn`.

**Device outputs:**

- **PDF:** a 1.4 xref writer. Content streams use FlateDecode through `flate2::write::ZlibEncoder` (`compress = TRUE`). The font is the base-14 Helvetica. Alpha uses `/ExtGState` (`ca`/`CA`). `CreationDate` comes from `Host::now`, so the output is deterministic on `PureHost`. Filenames with `%d`/`%03d` give one file per page when `onefile = FALSE`.
- **SVG:** text output. Sizes are `width="288pt"` and `viewBox="0 0 288 216"`. Opacity is written as `alpha/255` with 6 significant digits (`0.501961`).
- **PNG:** a clean-room scanline coverage rasterizer (nonzero and even-odd fill rules) with stroke expansion (caps, joins, dashes) into a premultiplied RGBA buffer. The encoder writes the PNG signature, `IHDR`, `pHYs` (from `res`), and `IDAT` through `ZlibEncoder`, and uses `flate2::Crc` for chunk CRC values. The encoder alone is about 150 lines. The device name follows `type`; the default `getOption("bitmapType")` on macOS is `"quartz"`, which gives `quartz_off_screen`.

**Text metrics:** `strwidth` and the legend geometry use Adobe Core-14 Helvetica AFM widths. These AFM files have a permissive Adobe notice; log them in `PRIOR_ART.md` and `THIRD_PARTY_NOTICES.md`. The PNG device draws glyphs as Hershey Simplex strokes (public domain), scaled to the same AFM advance widths.

**Parity contract:** numeric and geometry results must match exactly (on the PDF device). File output is checked by structure only (headers, page count, `MediaBox`, `IHDR` dimensions, opacity values). The oracle itself did not establish glyph or pixel parity.

**Coordinate pipeline** (this matches the oracle numbers):

1. Device inches → `oma` (outer margins).
2. → figure region (from `mfrow`/`mfcol`/`layout`).
3. → plot region = `fin − mai`, where `mai = mar × csi` and `csi = 0.2 in` at `cex = 1`, `ps = 12`.
4. → `usr`. Style `"r"` extends each end by 4% of the range. Log axes work in base-10 log units.

### 1.7 `roxide-methods` crate layout

Dependencies: `roxide-eval`, `roxide-value`, `roxide-gc`, `roxide-base`.

```
crates/roxide-methods/src/
  lib.rs           (W0) Package "methods" (attach), set_s4_hooks
  hooks.rs         (W0) S4Hooks impl delegating to modules below
  registry.rs      (W0 contract) MethodsExt: CtxExt — ClassCache, MethodCache, epoch: u64
  class/           (W1-A) classRepresentation, setClass/representation/prototype, contains linearization,
                   VIRTUAL/setClassUnion/setOldClass, new/initialize default, validity/validObject,
                   is/extends/as/setAs, slotNames/getSlots/.hasSlot/slot/slot<-, check_slot
  dispatch/        (W1-B) setGeneric/standardGeneric, setMethod/removeMethod/selectMethod/getMethod/
                   existsMethod/hasMethod/isGeneric/showMethods, resolver, callNextMethod, primitive & group generics
  show/            (W1-B) show/print for S4 instances, classRepresentation, generics
  refclass/        (W2-A) setRefClass, generator, fields (active bindings), methods, callSuper, copy, $ / $<-
  meta/            (W2-B) .rx_cacheMetaData, exportClasses/exportMethods/import*From integration
```

**Source of truth.** The R metadata objects are canonical: `.__C__<Class>` (a `classRepresentation`) and the `.__T__<generic>:<pkg>` method-table environments. Lazy-load and export carry these objects. The Rust caches (linearized superclasses with distances and parent rank, and the selected-method memo keyed by the target signature) are derived from them. Any `setClass`, `setMethod`, `removeMethod`, or namespace load or unload increases `epoch`, which invalidates the caches.

**Resolver.** Empirically verified against live GNU R 4.6.2 (35+ two- and three-argument tie and ambiguity-note probes):

1. Each candidate is applicable when, for every argument, the candidate class is `ANY`, is the target class itself, or is a superclass of the target class. `"missing"` matches only `missing` or `ANY`.
2. Let `D_max` be the largest superclass depth across **all** target arguments. The distance for an argument is its superclass distance (exact match = 0), and `ANY` has shared distance `1 + D_max` across the whole signature.
3. Select the candidate with the minimum total distance.
4. If candidates tie at minimum total distance, prefer a candidate that has at least one exact match (`any(d_i == 0)`, boolean — not count).
5. If candidates still tie, compare argument distances lexicographically one argument at a time in the generic's `signature` order.
6. If two distances on an argument are equal, the superclass that comes first in the declared `contains` order wins.
7. Emit GNU R's method-selection ambiguity note whenever **any** other applicable candidate strictly beats the winner in `(distance, parent_rank)` on at least one argument (Pareto non-dominance).

**`callNextMethod`.** The candidates are the methods that are strictly more general than the current method's *defined* signature. The selection uses the same resolver, with the target set to the defined signature. Memoize the result per method and epoch. The fallback is the generic's derived default or the primitive's internal default.

### 1.8 `roxide-cli` subcommand dispatch

When `argv[0]` is `R` or `roxide` and `argv[1] == "CMD"`, use this table:

| Subcommand | Action |
|---|---|
| `INSTALL [--library=DIR \| -l DIR] [--no-byte-compile] [--no-test-load] [--no-docs] <dir \| tar.gz>…` | `roxide_pkg::install::run`. The steps are below. |
| `BATCH [opts] infile [outfile]` | `roxide_pkg::batch::run`. The behavior is below. |
| `SHLIB`, and `INSTALL` of a package with `src/` | Return status 1 with a clear "compiled code requires Milestone 4" error. |
| Any other subcommand | R's unknown-command message, status 1. |

**`INSTALL` steps:**

1. Validate `DESCRIPTION`.
2. Stage into `<lib>/00LOCK-<pkg>/00new/<pkg>`.
3. Sort the source files by `Collate`, or else in C-locale order over `R/*.{R,r,q,S}`.
4. In a fresh `Ctx`, source the files into a namespace that `ns_create` made.
5. Write `R/<pkg>.rdb`, `R/<pkg>.rdx`, a clean-room `R/<pkg>` stub, `Meta/package.rds`, `Meta/nsInfo.rds`, `DESCRIPTION` (with a `Built:` field), and `NAMESPACE`.
6. Test-load in a second fresh `Ctx`.
7. Use `Host::rename` to move the package to its final location.
8. Test-load again.

The build `Ctx` and the test-load `Ctx` are separate in-process contexts, so `INSTALL` spawns no subprocess.

**`BATCH` behavior:**

- Default options: `--restore --save --no-readline`.
- Combine stdout and stderr into `<infile-base>.Rout`.
- Run with the startup banner and with `echo = true`.
- Append a `proc.time()` block unless `--no-timing` is given.
- `--save` writes `.RData`.
- An error writes `Error: …` and `Execution halted`, with exit status 1.

---

## 2. Proposed ADRs

**ADR-0012: S4 object model and multiple-dispatch contract.**
An S4 instance is any `SEXPTYPE` that has `flags.s4` set (`OBJSXP` for slot-only classes; a vector for `.Data` classes). Absent slots are attributes that are not present. A present `NULL` slot is the permanent `SEXP_S4_NULL` (`\001NULL\001`), and class attributes carry `package`. The `.__C__`/`.__T__` metadata objects are canonical, and the Rust caches are derived and epoch-invalidated. Method selection uses the resolver in 1.7 (minimum total distance, then an exact component, then signature order, then parent order; `ANY` = 1 + depth). The extended `S4Hooks` trait is the only coupling between `roxide-eval` and `methods`.

**ADR-0013: Dynamic namespace registry, lexical chain, and the `roxide-pkg` crate.**
Built-in and on-disk packages share one `NsRecord` registry. The chain is `namespace → imports → BASE_NAMESPACE` (a frame alias of `BASE_ENV` whose enclosing environment is `.GlobalEnv`) `→ search`. The layout uses R's `.__NAMESPACE__.` and `.__S3MethodsTable__.`, and deferred S3 registration flushes on load. A new crate `roxide-pkg` owns the NAMESPACE, DESCRIPTION, loader, `library`/`use`, and `R CMD`, and it connects to `::` through `NamespaceLoader`. `methods` integrates at R level only (`.rx_cacheMetaData`), so the two crates have no Rust dependency on each other.

**ADR-0014: Installed-package format, lazy-load DB, and `R CMD INSTALL`/`BATCH`.**
Read and write the GNU R–compatible `.rdb`/`.rdx` format (zlib/bzip2/xz records, `env::N` `PERSISTSXP` references) through a public `serialize_bytes`/`unserialize_bytes` API with a `PersistHook`. A `BCODESXP` read uses `consts[0]` as the body, and Roxide never byte-compiles. `INSTALL` stages and test-loads in two fresh in-process `Ctx` instances. A package with `src/` is rejected until M4.

**ADR-0015: Graphics engine and zero-new-dependency devices.**
A single `Device` trait works in 1/72-inch, y-up device units. The engine owns `par`/grid state and the display lists in a traced `GraphicsExt`. PDF 1.4 and SVG are hand-written text and bytes. PNG uses a clean-room coverage rasterizer with `flate2` zlib and CRC (already vetted under ADR-0008), so the change adds **no new crates**. Metrics are Adobe Core-14 Helvetica AFM; PNG glyphs are Hershey strokes. Parity is exact for numbers and geometry and structural only for file output.

**Documentation changes in W0 (not new ADRs):**

- In `ARCHITECTURE.md`, redefine **Layer 3** as "Object systems, packages and graphics". The binary-interchange tests already live in `layer1/07`.
- Amend the ADR-0004 DAG: add `roxide-pkg`, and remove the `roxide-stats` dependency from `roxide-graphics`.
- Add `scripts/cargo_slot.sh`: a 2-slot lock built on python3 `fcntl` (macOS has no `flock`). Every builder wraps its Cargo commands with it.

---

## 3. Disjoint parallel wave plan

### 3.1 W0: coordinator scaffold (serial)

Coordinator work:

- Make the changes in §1.2 and §1.3.
- Fix the API shape in §1.4.
- Create the empty crates `roxide-methods`, `roxide-graphics`, and `roxide-pkg` with their `lib.rs` and the W0 contract files (`registry.rs`, `hooks.rs`, `engine/mod.rs`). Add them to the workspace and the CLI registration.
- Write ADRs 0012–0015 and the doc amendments.
- Write `scripts/make_r_home.sh` and `scripts/cargo_slot.sh`.
- Add `conformance/layer3/` with each file holding one smoke case.

**Gate:** all 174 Layer 1 cases and all Layer 2 cases stay green, in normal mode and with `ROXIDE_GC_TORTURE=1`. The namespace-chain refactor is the main regression risk. `search()` must match the oracle. Have Opus 5.5 review the W0 diff before W1 starts.

### 3.2 File ownership

Each row is one builder. After W0, the coordinator keeps ownership of everything not in this table. Contract requests go to the coordinator.

| Builder | W1 | W2 | W3 | Owned paths (for all waves) |
|---|---|---|---|---|
| **A**: S4 classes | Classes, slots, `new`/`initialize`, validity, virtual/union/oldClass, `as`/`is`/`setAs`, `check_slot` | Reference classes | S4 hardening against the 00–02 and 06 suites | `roxide-methods/src/class/**`, `refclass/**`, `conformance/layer3/0{0,1,2,6}_*` |
| **B**: S4 dispatch | Generics, methods, resolver, `callNextMethod`, `show` | Primitive and group generics; `meta/` (exportClasses/exportMethods, import\*From, cacheMetaData) | Dispatch hardening against the 03–05 suites | `roxide-methods/src/{dispatch,show,meta}/**`, `conformance/layer3/0{3,4,5}_*` |
| **C**: packages | `nsfile`, `loader`, `library`/`require`/`use`, `s3reg`, `description` (source-directory packages) | `lazyload/**`, `install`, `tar`, plus `roxide-base/src/serialize/**` (persist hook, `BCODESXP`, public bytes API) | `batch`, `roxide-cli/src/cmd/**`, Tier-2 runner and shim | `roxide-pkg/src/**` (except `lib.rs`), `roxide-base/src/serialize/**`, `roxide-cli/src/cmd/**`, `tools/corpus/**`, `conformance/layer3/{07..12}_*`, `conformance/layer4/tier2/**` |
| **D**: devices and base graphics | `devices/**` (PDF, SVG, raster, PNG, metrics), `engine/dl.rs` | `graphics/{par,layout,coords,lowlevel}.rs` | `graphics/prelude.R`: `plot.default`, `hist`, `boxplot`/`bxp`, `barplot`, `legend`, `axis` | `roxide-graphics/src/{devices,graphics}/**`, `engine/dl.rs`, `conformance/layer3/{15..19}_*` |
| **E**: grDevices and grid | `grdevices/**` (colors, palette, `dev.*` table, `pretty`/`extendrange`/`n2mfrow`, `boxplot.stats`/`nclass.*`) | `grid/{unit,viewport,gpar,layout}.rs` | `grid/{draw.rs,prelude.R}`: grobs, gTrees, axis grobs, display list, `grid.ls`, `convertUnit` | `roxide-graphics/src/{grdevices,grid}/**`, `conformance/layer3/{13,14,20,21}_*` |

### 3.3 Later waves

- **W4: Tier-2 corpus burn-down.** Read-only triage workers run the corpus and file failures. Fixes go to the owner of the affected path in the table above, so W4 adds no new ownership.
- **W5: interoperability.**
  - Load packages that GNU R installed, from a library GNU R built (through the `BCODESXP` path).
  - Optional: make Roxide-installed packages loadable by GNU R.
  - Add `recordPlot`/`replayPlot`/`dev.copy`.
  - Complete grid on the PNG and SVG devices.
- **Hardening.**
  - `ROXIDE_GC_TORTURE=1` on all of Layer 3.
  - Fuzz targets for `nsfile`, `description`, the `.rdx`/`.rdb` reader, the `tar` reader, and the PNG/PDF writers (bounded output size).
  - Licence scan.
  - Cross-model review: Opus 5.5 reviews the W1-B resolver and the C loader; Sol reviews the W0 diff.
  - Ask GPT 6.2 Astra about the `hist` fuzz, `pretty`, and the grid unit conversion.

**Routing:** Builders A–E are GPT 6.2 Sol at `high` effort. Each wave uses a fresh Sol reviewer and the oracle gate. There are at most 2 Cargo slots through `cargo_slot.sh`.

---

## 4. Conformance layout

### 4.1 Layer 3 suites

`conformance/layer3/` uses the `.rcase` format, `#@ compare: exact`. Cases write only under `tempdir()`. Package fixtures are created inline with `writeLines`, so there are no fixture trees; this keeps the file count small for exFAT.

| File | Covers (from the oracle spec) |
|---|---|
| `00_s4_classes_slots.rcase` | `typeof`/`isS4`, the package-qualified `class`, attribute order with `\001NULL\001`, `slotNames`/`getSlots`/`.hasSlot`, `slot(check=FALSE)`, `.Data` on `Vec` |
| `01_s4_new_initialize_validity.rcase` | Prototype, named and unnamed `new()` arguments, `initialize` with `callNextMethod`, validity versus checked `@<-`, `validObject(test=, complete=)` |
| `02_s4_virtual_union_oldclass_as.rcase` | `VIRTUAL`, slotless classes, `setClassUnion`, `setOldClass`, `as(strict=)`, `setAs` inheritance, slot order for multiple `contains` |
| `03_s4_dispatch_resolver.rcase` | All 12 rows of the diamond/linear table, the 6-parent case, the depth-6 `ANY` case, `missing`, `selectMethod(useInherited=FALSE, optional=TRUE)` |
| `04_s4_callnextmethod_show.rcase` | The next method comes from the defined signature, explicit arguments, add/remove method invalidation, `show`/`print`/auto-print/`invisible` |
| `05_s4_primitive_group_generics.rcase` | `setMethod("[", …)`, Arith/Compare/Ops groups, S3 dispatch on S4 instances |
| `06_refclass.rcase` | Generator forms, `.xData`, aliasing, typed and function fields, `$copy(shallow=)`, `callSuper` |
| `07_namespace_directives.rcase` | All directives, `if`/braces, `export(c(a,b))`, `exportPattern`, `except=` |
| `08_load_attach_unload.rcase` | Chain `environmentName`s, `c(9,2,501)`, hook order, `loadedNamespaces`, refused unload |
| `09_library_require_use.rcase` | Visibility and return values, `logical.return`, `use(include.only=)` cases |
| `10_s3_registration_deferred.rcase` | Conventional and explicit `S3method`, `rxgen::talk` deferred, quoted literal, survival after unload |
| `11_lazyload_rdb_rdx.rcase` | `.rdx` keys and offsets, compressed records, promise fetch, environment references |
| `12_rcmd_install_batch.rcase` | Installed layout, `42L` through `::`, `.Rout` content and status 0/1 |
| `13_grdevices_colors_palette.rcase` | `col2rgb`, `rgb`/`hsv`/`hcl`/`adjustcolor`/`colorRampPalette`, `palette` |
| `14_device_table.rcase` | Null device, IDs, `dev.next`/`prev`/`set`/`off` (including 99), `dev.size` units |
| `15_device_files_pdf_svg_png.rcase` | Structural checks: `%PDF-1.4`, `/Count`, `MediaBox`, numbered files, SVG `pt`/`viewBox`/`pageSet`/opacity, PNG signature and `IHDR` |
| `16_par_coords_layout.rcase` | `par` defaults, `plot.window` (r/i/log/constant), `mfrow` `cex` steps, `layout` regions |
| `17_base_lowlevel.rcase` | Frame requirement, invisible `NULL` results, `NA` splitting, recycling, `axis` return value |
| `18_hist_boxplot_barplot.rcase` | `hist` fields/right/fuzz/`nclass`, `boxplot` stats/conf/out/`warnN`, `barplot` midpoints, visibility |
| `19_legend_axis.rcase` | `legend(plot=FALSE)` geometry on the PDF device |
| `20_grid_units_viewports.rcase` | Unit classes, codes, arithmetic, `convertUnit`, push/up/seek/pop semantics |
| `21_grid_grobs_dl_layout.rcase` | `gpar`, grob classes, gTree, axis grobs, `grid.ls` order, `grid.layout` null units |

### 4.2 Tier-2 corpus layout

```
tools/corpus/fetch_tier2.py        # pinned versions + SHA-256; downloads to $TMPDIR (never committed)
tools/corpus/testthat_shim/        # Roxide-authored MIT mini-package: test_that, expect_{equal,identical,error,
                                   #   warning,message,true,false,null,length,named,s3_class,output},
                                   #   skip_*, local_edition, test_dir → prints per-file pass/fail/skip counts
conformance/layer4/tier2/
  manifest.tsv                     # pkg, version, sha256, NeedsCompilation(no), imports
  run.R                            # install → library() → test_dir via shim
  baseline.tsv                     # GNU R 4.6.2 counts from the SAME shim (differential)
```

### 4.3 M3 exit gate

For each package, all four conditions must be true:

1. `R CMD INSTALL` returns status 0 under Roxide.
2. `library(pkg)` succeeds.
3. The per-file shim counts equal the GNU R baseline.
4. A smoke run under `ROXIDE_GC_TORTURE=1` has no panics.

**Packages in the M3 gate.** All of these are pure R (`NeedsCompilation: no`):

| Package | Main areas it tests |
|---|---|
| `R6` | Environments, `lockBinding`, active bindings |
| `withr` | `graphics`/`grDevices` imports, `with_par`, devices |
| `crayon` | `methods`/`grDevices`/`utils` imports |
| `praise` | No imports |
| `magrittr` **1.5** | The last pure-R release, from the CRAN archive |
| `zeallot` | Extra pure-R coverage (`assertthat` is GPL-3 and excluded per the clean-room licence wall) |

**Moved to M4 Tier 3:** `magrittr` ≥ 2.0 and the real `testthat` (both compiled). In M3, the shim stands in for "testthat-core". Run the shim under both engines so that the test is still differential.

**Coordinator actions in W0:**

- Pin the exact versions with the oracle's `available.packages()`.
- Confirm `NeedsCompilation` for each package.
