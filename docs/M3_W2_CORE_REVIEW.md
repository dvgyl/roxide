**Milestone 3 Wave 2 Core review (Opus 5.5, read-only, no Cargo)**

I found no P0. I found 10 P1 and 15 P2 items. Seven P1 items are confirmed by running the same script in GNU R 4.6.2 and in `target/debug/roxide` (built 21:19, after all three commits). Three P1 items come from reading the code only, and I say so for each.

The biggest finding: `roxide CMD INSTALL` fails for every package that calls `setClass` or `setRefClass`, because the reader cannot read serialized S4 objects back.

## P0 (soundness, GC, security, data corruption, clean room)

None in the code.

- **GC:** collection runs only at `Ctx::safepoint`, and `SexpId` handles carry a generation. In hook mode the decoder protects every partly built value before a hook can run R code. `SelectionCache` and `NamespaceCache` trace their handles.
- **Paths:** `valid_package` and the `Collate` checks block path traversal through `Package:` or `Collate:`. `tar::safe_path` rejects `..`, absolute paths, NUL, `\` and `:`, and the same check applies to PAX `path` and GNU `L` names. Extraction rejects links, checks every directory for symlinks, and limits both the entry count and the total decompressed bytes.
- **Clean-room disclosure:** one of my oracle probes printed the GPL loader stub that GNU R installs as `library/MASS/R/MASS`. I did not use that text, and nothing below depends on it. The Roxide `STUB` in `install.rs` is a different text; keep it that way.

## P1 (GNU R 4.6.2 parity bugs)

1. **S4 objects (type 25) cannot be read back. This blocks install of any S4 package.**
   - Location: `crates/roxide-base/src/serialize/read.rs:357-383`. The writer emits type 25 (`write.rs:349`), but the reader has no case for it.
   - Evidence: `unserialize(serialize(new("Q", x = 1), NULL))` fails with "unsupported serialized R type 25". `CMD INSTALL` of a package with only `setClass("Q", ...)` fails with the same error and then "test load failed". A refclass package fails the same way.
   - Fix: add a type-25 case that allocates an S4 object, reads the attributes, and sets the S4 bit from `flags & S4`. Add an S4 package and a refclass package to `12_rcmd_install.rcase`.

2. **A lazy-load DB whose `compressed` value is the double `3` is rejected.**
   - Location: `crates/roxide-pkg/src/lazyload/read.rs:40`. The function accepts only a logical or an integer.
   - Evidence: GNU R's own `datasets/data/Rdata.rdx` stores `compressed = 3` as a double. `makeLazyLoadDB(..., compress = 3)` does the same. Roxide gives "invalid lazy-load compression mode" for both. `TRUE`, `FALSE`, `2L` and `3L` load correctly.
   - Fix: also accept a double scalar with an integer value from 0 to 3. Reject `NA`.

3. **`R/sysdata.rda` is ignored.**
   - Location: `install.rs` `build()` (around line 274) and `loader.rs:238` (`source_files`).
   - Evidence: after install, `f()` gives "object 'internal_tbl' not found". GNU R installs `R/sysdata.rdb` and `R/sysdata.rdx` (seen in the oracle's tools and utils packages). Writing R Extensions documents this behaviour.
   - Fix: at build time, load the file into a new environment and write `R/sysdata` with `write_lazyload_db`. At load time, lazy-load it into the namespace. When the package is loaded from source, use `load()` into the namespace.

4. **`data/` and `LazyData: true` are dropped.**
   - Location: `install.rs` `build()`. The function does not copy `data/` and does not build `data/Rdata.rdb`, `data/Rdata.rdx` or `Meta/data.rds`.
   - Evidence: after install, `exists("ds")` is FALSE.
   - Fix: build the data DB. When the package is attached, lazy-load the DB into the package's lazydata environment.

5. **`CMD INSTALL` rejects standard options.**
   - Location: `install.rs:71-115`.
   - Evidence: `--no-lock` fails with "unknown INSTALL option".
   - Fix: accept the documented INSTALL options (for example `--no-lock`, `--install-tests`, `--no-staged-install`, `--with-keep.source`, `--html`, `--example`, `--resave-data`, `--data-compress=`, `--byte-compile`, `--fake`). Treat as a no-op the options that do not apply to pure-R packages.

6. **`setClass(..., contains = <class imported from a locked namespace>)` fails.** W2-B flagged this.
   - Location: `crates/roxide-methods/src/class/definitions.rs:547`. `refresh()` calls `store()` to write the parent's updated `subclasses` back into the locked namespace binding.
   - Evidence: `setClass("Sub", contains = "PC")` gives "cannot change value of locked binding for '.__C__PC'". In GNU R, `getClass("mle")@subclasses` includes `MyMLE`, and the stats4 namespace stays locked.
   - Fix: if the binding or the namespace is locked, update only `MethodsExt.classes` and `ClassState`. Do not write the binding.

7. **Inside a refclass method, `getClass(...)` calls the refclass method, not `methods::getClass`.**
   - Location: `crates/roxide-methods/src/refclass/instances.R:240-241`.
   - Evidence: a method body that calls `getClass("numeric")` gives "unused argument" in Roxide. GNU R returns the class. (`show(42)` gives "unused argument" in both engines, so that part already matches.)
   - Fix: `getClass = function(...) if (nargs()) methods::getClass(...) else .refClassDef`. Change `getRefClass` the same way.

8. **`callGeneric(<explicit args>)` builds a wrong call.** Confirmed by running both engines.
   - Location: `dispatch/invoke.rs:512-536`. `make_call` puts the generic's function object and the already-evaluated argument values into the call.
   - Evidence: when the callee prints `sys.call()`, Roxide gives "deparse depth limit exceeded". GNU R prints `g(as.character(x), ...)`. This construction can also break `substitute`, `match.call` and `missing` in the callee; I did not run probes for those.
   - Fix: make a call whose head is the generic's name (`.Generic`) and whose arguments are the original unevaluated expressions. Evaluate it in the method frame. Note: `callGeneric()` with no arguments already matches GNU R, because it uses the current values of the formals.

9. **R-level `serialize`/`unserialize(refhook=)` still stops with "serialization reference hooks are unavailable".**
   - Location: `crates/roxide-base/src/serialize.rs:163`.
   - Fix: wrap the R function in a `PersistHook` adapter, like `Callback` in `lazyload/read.rs`. A `NULL` result means "write normally". Any other result must be a character vector of length 1 or more. The wire format of the hook path already matches GNU R: GNU R calls the hook for each occurrence, and each `PERSISTSXP` uses one reference index (REF index 2 in my probe).

10. **`is.primitive(lazyLoadDBfetch)` is FALSE in Roxide and TRUE in GNU R.**
    - Location: the prelude in `lazyload/mod.rs`.
    - Fix: register it as a primitive.

## P2 (edge cases and performance)

- **Why `06_refclass.rcase` times out under GC torture.** CPU user time measured with `ROXIDE_GC_TORTURE=1`:

  | Step | Cost (CPU s) |
  |---|---|
  | Startup | 2.8 |
  | `setRefClass` | +1.9 |
  | One `$new()` | +2.2 (about 100 closure calls) |
  | One method call | about 0.5 (about 25 closure calls) |
  | One `$copy()` | +3.6 |
  | One `x$field` read | about 0.13 |

  A loop of 200 plain closure calls takes 4.2 s, so one closure call costs about 0.02 s. The rcase therefore needs about 100 s or more of CPU.

  The hot paths are these:
  - The `initialize` wrapper (`registration.R`) calls `getClass`, and `.ref_definition` calls `getClass` 1–2 more times. That is 2–3 `getClass` calls for each `new()`.
  - `.ref_method_names` runs `ls`, `sort` and `.ref_unique` again at every inheritance level, for every object.
  - `.ref_install_field` runs `getClass` and a prototype `new`/`.s4_new` for each typed field of each object.
  - `.ref_install_methods` makes one environment and one closure for each of the 15 base methods plus each user method, for every object.
  - `$` goes through S3 dispatch to the R-level `.ref_member`.
  - Each `n <<- ...` on a typed field runs the R-level setter: `missing`, `bindingIsLocked`, `is()`, `assign`.

  Fix: cache the definition, the sorted method names and the field prototypes in `meta`, and clear them on `$methods()`, `$lock()` and `$accessors()`. Read `meta` from the class definition once per `new()`. Move `.ref_member` and the typed-field setter to Rust primitives. Share one set of method bindings for each class, not for each object.
- `merge_tables` makes a new environment and binds every method again on each `standardGeneric` call (`invoke.rs:263`), and on primitive dispatch when more than one table has methods. It also turns off the selection cache (`invoke.rs:606`). `show` with methods in the global environment and in a package is a common case. Fix: keep the merged table in a cache, keyed by the method epoch and the IDs of the source tables.
- `clear_unloaded` runs on every dispatch. Your uncommitted `loader.rs` change already uncaches on unload, so you can remove the per-dispatch call or run it only when the set of loaded namespaces changes.
- `"as.double"` is changed to `"as.numeric"` by a string test in two places (`metadata.rs:794`, `invoke.rs:560`). The behaviour matches the oracle, but one alias table should serve `setMethod`, `getMethod`, `removeMethod` and `existsMethod`.
- Hook-mode decoding protects every item, including each CHARSXP (`read.rs:245`). The protect stack therefore grows with the size of the record. Protect only values that stay alive across a hook call.
- A lazy-load environment reference that is nested deeper than 64 levels fails (`lazyload/read.rs:362`). Make hydration two-phase or iterative.
- `EnvironmentQueue::write` gives a persistent key for ExtPtr and WeakRef objects too (`lazyload/write.rs:79`). `describe()` then fails on them. Return `None` for any type that is not an environment.
- The Roxide namespace DB leaves out `.__S3MethodsTable__.` and `.__NAMESPACE__.`. GNU R's stats, MASS and tools DBs include both. The loader also drops `.__S3MethodsTable__.` when it reads a DB that GNU R wrote.
- `persistent()` writes an empty STRSXP if the hook returns `character(0)` (`write.rs:155`). Validate that the key has length 1 or more.
- A top-level `BCODESXP` ignores the attribute flag (`read.rs:357`). If the flag is set, the reader loses its position in the stream and then fails.
- R file selection (`loader.rs:253-265`): it does not accept `.s` files. It does not require that file names start with an ASCII letter or digit. It does not source `R/unix/`.
- `tar.rs`:
  - The check-then-open sequence leaves a small race window. Use create-new with no-follow.
  - It rejects an archive that ends with only one zero block.
  - It does not keep executable mode bits.
  - It rejects type `'7'` entries.
- `install.rs` `test_load` drops the cause of the failure. `paths.retain` compares a canonical path with non-canonical paths, so `.libPaths()` can contain the library twice.
- The install writes no `Meta/{features,Rd,hsearch,links,data}.rds`, `INDEX` or `help/`.
- `obj$copy` returns a plain function. GNU R returns a `refMethodDef`.

## Behaviour that matches GNU R

- Refhook call count and reference numbering.
- All 213 bindings in GNU R's MASS lazy-load DB force without error in Roxide. This includes the byte-code (`BCODESXP`) decoder.
- `env::N` cycles keep their identity. A locked environment comes back locked, and its bindings stay unlocked.
- Raw LZMA2 records for `compressed = 3L`, including a 24 MB record.
- Group-generic precedence (`+` before `Arith` before `Ops`), and the value of `.Generic`.
- The P0-1 fix: a global `show` method plus `library(pS4)`, then `detach` and `unloadNamespace`.
- A refclass defined in a namespace that is loaded but not attached.
- A deep `$copy()` of a cycle fails with infinite recursion in both engines.
- The message for `$lock` on an `ANY` field.

## Outside the Wave 2 scope

- Base functions missing in Roxide: `unique`, `setdiff`, `match.arg`, `as.environment`, `proc.time`, `Sys.time`, `.Library`.
- `methods::signature` is missing.
- `err <- function(e) tryCatch(e, ...)`; `err(print(x))` prints twice in Roxide.
- Character vectors print without padding to a common width.

Your uncommitted `loader.rs` change (uncache the metadata on a failed load and on unload) is correct. Land it.

All probe scripts are in `/private/tmp/claude-501/-Users-devg-Documents-Roxide/f571184e-91a7-414b-9785-56348fff99af/scratchpad/p/`. `both.sh <file.R>` runs one script in both engines.
