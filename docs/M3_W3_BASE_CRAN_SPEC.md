# Roxide Tier-2 CRAN readiness audit (base / eval / utils)

## 0. Method and summary

- **Method.**
  - I ran the prebuilt `target/debug/roxide` (built 21:19, HEAD `7059989`) and GNU R 4.6.2 on 8 probe suites, about 450 cases, and compared the output.
  - I read the source at each failure.
  - I did not run `cargo`. I did not edit files. Probe files are in `/private/tmp/claude-501/-Users-devg-Documents-Roxide/ade2f088-87ef-44c4-9d7c-33179e9d49ac/scratchpad/`.
  - The uncommitted diff (`set_length` and `rep` dispatch) does not change any finding.
  - I moved 10 probe output files that I wrote into the repo root by mistake to the scratchpad. The tree is clean again.
- **Inventory.**
  - `baseenv()` has 505 symbols in Roxide and 1407 in GNU R. Without S3 methods and internal names, 411 base functions are missing.
  - The `utils` namespace has 40 symbols in Roxide and 611 in GNU R.
  - From the brief's list, 172 functions are missing.
- **Result.** No Tier-2 package can load or run its tests now. About 12 root-cause bugs plus about 120 missing Tier-A functions block them.

## 1. P0 root-cause bugs (fix first)

| # | Bug (confirmed against the oracle) | Location | Required R 4.6.2 behavior |
|---|---|---|---|
| **B1** | **Visibility.** `f(invisible(x))`, `withVisible(<promise>)`, `x <- 1` inside a function, active bindings, and `if (FALSE) 1` all return visible. | `crates/roxide-eval/src/eval.rs:136` sets `visible = true` *after* `get_var` forces the promise. Same bug at `:120` for `..N`. | Set `visible = TRUE` at the *start* of symbol evaluation. Forcing an unforced promise, or calling an active-binding getter, can then set it to `FALSE`. An already-forced promise stays visible. Oracle: `g(invisible(1))`→F, `h<-function(x){force(x);x}`→T, `..1`→F, active getter that returns `invisible()`→F. |
| **B2** | **Print.** Lists print as `deparse` text. Named atomic vectors lose their names. Attributes never print. Strings are not padded. No 80-column wrapping and no `[k]` index. Autoprint of a matrix prints a flat vector. S4 elements inside a list are not passed to `show()`. | Autoprint: `crates/roxide-eval/src/ctx.rs:1052-1062` sends non-objects to the minimal `builtins.rs:517 print_value`. `crates/roxide-base/src/subset/printing.rs:46` sends the default case to the same function. | Send autoprint of non-objects to base `print.default`. Rewrite `print.default`: (a) names in a header row, right-aligned per column; (b) strings left-padded to a common width, `quote=` supported; (c) wrap at `getOption("width")` with `[k]` labels padded to the widest label (` [1]` / `[26]`); (d) lists recurse with tags `[[1]]`, `$a`, `$b$c`, a blank line after each element, `list()`, and `named list()`; (e) S4 elements call `show()`, objects call `print()`; (f) trailing `attr(,"name")` blocks for non-internal attributes (not names, dim, dimnames, class, or tsp). Example: `structure("Foo", package="pkg")` prints the `package` attribute. |
| **B3** | **`cat(file=)` is ignored.** Output to a path or to a connection, including `textConnection`, goes to stdout. `fill=` and `append=` are also ignored. | `crates/roxide-eval/src/builtins.rs:727-734` skips these arguments without using them. | `file=` takes a path or a connection. A path opens with `"w"`, or `"a"` when `append=TRUE`. `fill=TRUE` adds a final newline and wraps at the width. withr, testthat reporters, and `capture.output` need this. |
| **B4** | **`$` / `$<-` S3 methods get `name` as a symbol.** | `crates/roxide-eval/src/eval.rs:567` (EvalDollar keeps the symbol) → `dispatch.rs:361 dispatch_primitive`. | When the call dispatches to an S3 or S4 method, pass `name` as a length-1 character vector. crayon's `$.crayon` (`crayon$red`) needs this. |
| **B5** | **Assignment into a call makes a list.** `cl[[1]] <- as.name("g")`, `cl$y <- 2`, and `cl[2] <- list(...)` give a list. `cl[-2]` gives a pairlist. | `crates/roxide-base/src/subset/replacement.rs` and `subset/index.rs`. | A LANGSXP (call object) stays LANGSXP after `[[<-`, `[<-`, `$<-`, and `[`. magrittr rewrites calls this way. |
| **B6** | **P1-8: the S4 bit is lost.** `isS4(q+1)`, `1+q`, `-q`, `sqrt`, `abs`, `round`, and `log` give `FALSE`. | `crates/roxide-base/src/ops/common.rs:265 copy_attrs` never copies `flags.s4`. `ops/arithmetic.rs:194-200` copies attributes with direct `set_attrib`, not through `copy_attrs`. `ops/summary.rs:507` (Math) uses `copy_attrs`. | When the source has the S4 bit and `class` is copied, set the S4 bit on the result (`set_s4_flag`). It must stay `FALSE` for comparisons, `!`, `cumsum`, `sum`, `[`, and `rev`. The oracle matrix is in `s4f.R`. |
| **B7** | **P1-5: `callNextMethod()` forwards a missing formal that the method assigned.** | `crates/roxide-eval/src/closure.rs:356-373 generic_frame_args` reads the *current* `binding.missing`. Assignment clears it. | Forward only the formals that were *supplied in the original call*: use `frame.actuals` (`closure.rs:336`) or a snapshot taken at entry. Pass their *current* values. Oracle: method does `y <- "set"; callNextMethod()` after `ff(1L)` → the next method sees `missing(y)=TRUE`. When `y` was supplied, it sees `"set"`. |
| **B8** | **No base constants.** `T`, `F`, `pi`, `letters`, `LETTERS`, `month.name`, `month.abb`, and `.Machine` do not exist. | None in the tree. Add them to a `roxide-base` prelude or as Rust bindings. | `.Machine` needs at least `integer.max`, `double.eps`, `double.xmin`, `double.xmax`, `double.digits`, `sizeof.pointer`, and `double.neg.eps`. testthat and `all.equal` use `sqrt(.Machine$double.eps)` as the default tolerance. |
| **B9** | **`format.default` is a stub that only calls `as.character`.** | `crates/roxide-base/src/datetime.rs:91` | Support `width`, `justify` (`"left"`/`"right"`/`"centre"`), `nsmall`, `digits`, `big.mark`, `trim`, and `na.encode`. Logical values pad to a common width (`c("TRUE","  NA")`). Lists format each element. `format(NULL)` → `"NULL"` (oracle). `format(<env>)` → `"<environment>"`. |
| **B10** | **`Sys.getenv`** fails with `unset=NA`, and `Sys.getenv()` returns plain character. | `crates/roxide-base/src/io_os.rs:164` → `rox_io_getenv` | `unset=NA` → `NA_character_`. With no arguments, return a named vector with class `"Dlist"`. withr relies on NA to mean "unset". |
| **B11** | `lockEnvironment`: adding a new binding gives the wrong message. | `roxide-value/src/api.rs:69` (one locked error for all cases) | New binding in a locked env → `cannot add bindings to a locked environment`. Change to an existing locked binding → keep `cannot change value of locked binding for 'x'`. |
| **B12** | `do.call(f, list(as.name("x")), quote=TRUE)` fails. `names(env)` returns `NULL`. `loadNamespace` errors have class `simpleError`. | `language.rs:52` (`do.call`); env `names`; `namespace.rs` | `quote=TRUE` wraps symbols and calls in `quote()`. `names(env)` = all names, unsorted. A missing package gives class `packageNotFoundError`, message `there is no package called ‘pkg’`. |
| **B13** | `print(1e5)` / `print(1e6)` choose fixed notation. GNU R gives `1e+05` / `1e+06`. `cat(1e6)` has the same bug. | `roxide-value` `format_real_vec` / `format_real_scalar` | Use fixed notation only when fixed width ≤ scientific width + `scipen`. |

## 2. Tier-A missing functions (essential for M3), grouped by owner file

Notation: function signature, then edge cases checked against the oracle.

### `crates/roxide-eval/src/environment.rs` (envs and frames)

- [ ] **`get0`**: `(x, envir=pos.to.env(-1L), mode="any", inherits=TRUE, ifnotfound=NULL)`.
- [ ] **`mget`**: `(x, envir, mode="any", ifnotfound, inherits=FALSE)`. Returns a named list. Error: `value for ‘x’ not found`.
- [ ] **`list2env`**: `(x, envir=NULL, parent=parent.frame())`. Returns `envir`.
- [ ] **`as.environment`**: number → search position (1 = globalenv); `"package:base"` → base; list → new env with parent `emptyenv()`; an env passes through.
- [ ] **`reg.finalizer`**: `(e, f, onexit=FALSE)`. Returns `NULL` invisibly. R6 calls it with `onexit=TRUE`. At minimum, run the `onexit` finalizers at session end and at `quit()`.
- [ ] **`activeBindingFunction`**: `(sym, env)`.
- [ ] **`objects`**: alias of `ls`.
- [ ] **`delayedAssign`**: `(x, value, eval.env=parent.frame(1), assign.env=parent.frame(1))`.
- [ ] **`sys.calls`, `sys.frames`, `sys.parent(n=1)`, `sys.parents`, `sys.on.exit`**: `sys.on.exit()` returns the registered expression, for example `1 + 1`.
- [ ] Fix B11 and `names(env)` (B12).

### `crates/roxide-eval/src/language.rs` (functions and language objects)

- [ ] **`formals`, `formals<-`, `body`, `body<-`, `args`, `alist`, `as.pairlist`, `as.function(x, envir)`**: `formals(<primitive>)` → `NULL`.
- [ ] **`call(name, ...)`, `as.call(list)`**: `eval(call("function", as.pairlist(alist(.=)), body), env)` must build a closure. magrittr uses this.
- [ ] **`bquote(expr, where=parent.frame(), splice=FALSE)`**: supports `.()` and `..()`.
- [ ] **`match.arg(arg, choices, several.ok=FALSE)`**:
  - When `arg` is identical to the default `choices`, return the first choice.
  - Use `pmatch` (partial matching).
  - Error: `'arg' should be one of “a”, “bb”`, with fancy double quotes.
- [ ] **`match.fun`, `Recall`, `nargs`**: `nargs` counts supplied arguments, and each `...` element counts as one. `(function(a, ...) nargs())(1,2,3)` → 3.
- [ ] **`Vectorize`**: write it from `?Vectorize` only (see the disclosure above). Delegate to `mapply` for the arguments in `vectorize.args`.
- [ ] **`str2lang`, `str2expression`, `eval.parent(expr, n=1)`, `forceAndCall`**.
- [ ] **Predicates**: `is.primitive`, `is.call`, `is.name`, `is.symbol`, `is.pairlist`, `is.expression`, `is.language`, `is.recursive`, `is.object`.
- [ ] **`identity`, `Negate`**.
- [ ] **`switch`**: must be a primitive. It supports fall-through, for example `switch("b", a=, b="ab")`, and a numeric index. Many packages need it.
- [ ] **`ifelse`**.

### `crates/roxide-eval/src/conditions.rs` (R prelude is acceptable)

- [ ] **`try(expr, silent=FALSE, outFile)`**:
  - Returns a `"try-error"` string with the `condition` attribute.
  - Writes `Error in <call> : msg\n` to stderr when `silent=FALSE`.
  - Sets `geterrmessage()`.
- [ ] **`suppressWarnings(expr, classes="warning")`, `suppressMessages(expr, classes="message")`, `suppressPackageStartupMessages`**.
- [ ] **`packageStartupMessage`**: the condition class is `c("packageStartupMessage","simpleMessage","message","condition")`.
- [ ] **`simpleCondition`, `errorCondition(message, ..., class, call)`, `warningCondition`, `computeRestarts`, `tryInvokeRestart`, `geterrmessage`**.

### `crates/roxide-base/src/subset/` (vectors, sets, apply family)

- [ ] **`%in%`, `match(x, table, nomatch=NA_integer_)`, `unique`, `duplicated`, `anyDuplicated`, `setdiff`, `union`, `intersect`, `is.element`**. magrittr binds `is_in <- \`%in%\`` at load time.
- [ ] **`unlist(x, recursive=TRUE, use.names=TRUE)`**: the naming rule from the oracle: `list(a=1:2, b=list(c=3, 4), 5)` → `a1 a2 b.c b ""`.
- [ ] **`which`** (keeps names), **`which.max`, `which.min`**.
- [ ] **`Map`, `mapply(FUN, ..., MoreArgs, SIMPLIFY=TRUE, USE.NAMES=TRUE)`**: when the first `...` argument is character and has no names, use its values as names.
- [ ] **`Find`, `Position`, `rapply(how=)`, `tapply`, `apply`, `array`, `vector(mode, length)`, `replicate`, `eapply`, `rep_len`, `append`, `lengths`, `split`, `pmax`, `pmin`, `NROW`, `NCOL`**.
- [ ] **`.subset`, `.subset2`, `getElement`**: R6 uses `.subset2` heavily.
- [ ] **Replacement functions**: `rownames<-` (magrittr binds `set_rownames` at load time), `oldClass<-`, `storage.mode<-`, `mode<-`, `is.na<-`, `substr<-`, `regmatches<-`.
- [ ] **`mean` + `mean.default`, `nlevels`, `with`/`with.default`, `within`**.

### `crates/roxide-base/src/strings/`

- [ ] **`gettext`, `gettextf`, `ngettext(n, msg1, msg2)`**: `gettext` returns its input unchanged; `gettextf` is `sprintf` applied after `gettext`.
- [ ] **`sQuote(x, q=TRUE)`, `dQuote`**: the default follows `getOption("useFancyQuotes")`. In UTF-8 this gives `‘x’` and `“x”`.
- [ ] **`shQuote`**.
- [ ] **`strwrap(x, width=0.9*getOption("width"), indent, exdent, prefix, simplify, initial)`**.
- [ ] **`encodeString(x, width, quote, na.encode, justify)`, `formatC(x, width, digits, format, flag, mode, big.mark)`, `prettyNum(x, big.mark)`, `toString(x, collapse=", ")`, `strtrim`, `strrep`, `utf8ToInt`, `intToUtf8(multiple=)`, `iconv` (UTF-8, latin1, and ASCII with `sub=`), `make.unique(sep=".")`, `regexec`, `strtoi(x, base)`, `casefold`**.
- [ ] B9 (`format.default`).

### `crates/roxide-base/src/io_os/`

- [ ] **File functions**: `file.copy(from, to, overwrite=FALSE, recursive=FALSE)`, `file.remove`, `file.size`, `file.mtime`, `file.append`, `file.access(names, mode)` (assertthat `is.writeable` / `is.readable` need it), `path.expand`, `list.dirs(full.names=TRUE, recursive=TRUE)`, `Sys.glob`.
- [ ] **`Sys.time`, `Sys.Date`, `Sys.sleep`, `Sys.which`, `Sys.getpid`**.
- [ ] **`Sys.getlocale(category="LC_ALL")`, `Sys.setlocale(category, locale)`**: return the locale string. withr `local_locale` and `local_collate` need them.
- [ ] **`Sys.timezone`**.
- [ ] **`sink(file, append, type, split)`, `sink.number`**: withr `local_output_sink` and `capture.output` need them.
- [ ] **Session queries**:
  - `isatty(con)` and `interactive()`: both return `FALSE` under Rscript. crayon needs them.
  - `capabilities(what)`: must not load X11.
  - `l10n_info()`: returns `list(MBCS, \`UTF-8\`, \`Latin-1\`, codeset)`.
  - `commandArgs(trailingOnly)`.
- [ ] **`quit` / `q(save, status=0, runLast)`**: exits with `status` and runs the `onexit` finalizers. The test runner needs the exit code.
- [ ] **`source(file, local=FALSE, echo, ...)`, `sys.source(file, envir, chdir, toplevel.env)`**: `local=TRUE` means `parent.frame()`.
- [ ] **`system.file`**: returns `""` for built-in packages now. It must return the package directory.
- [ ] **`proc.time`, `system.time`, `date`**.
- [ ] **`readline`**: returns `""` when not interactive.
- [ ] B3 (`cat`) and B10 (`Sys.getenv`).

### `crates/roxide-utils/src/` (new submodule, for example `pkgutils.rs`)

- [ ] **`globalVariables(names, package, add=TRUE)`**: returns its result invisibly. Package code calls it at top level, so it is a **load blocker**.
- [ ] **`modifyList(x, val, keep.null=FALSE)`**: merges recursively.
- [ ] **`packageVersion(pkg)`**: returns `package_version`. For built-in packages it is `"4.6.2"`.
- [ ] **`compareVersion(a, b)`**: returns -1, 0, or 1. Example: `"1.2"` vs `"1.10"` → -1.
- [ ] **`getFromNamespace`, `assignInNamespace`**: `assignInNamespace` unlocks the binding, assigns, then locks it again.
- [ ] **`getS3method(f, class, optional=FALSE)`, `hasName`**.
- [ ] **`person`, `as.person`, `format.person`**: Tier-A only if `roxide-pkg` evaluates `Authors@R` at install time. R6, withr, and crayon use it.

## 3. Tier-B (not urgent)

- **Collation.** `sort` and `ls` use C order. The oracle uses locale order, `c("_x","a","A","b","B")`, in `C.UTF-8`. This affects R6 print order and snapshots.
- **Frame numbers.** `sys.nframe()` inside `tryCatch` gives 2 in Roxide and 7 in GNU R, because `tryCatch` is native in Roxide. `sys.function(n)` and `parent.frame(n)` see different numbers.
- **Error output order.** stdout is not flushed before the error text goes to stderr, so the order is wrong in `2>&1` logs.
- **`options(warn=2)`** stores a double. GNU R stores an integer.
- **`callNextMethod()` result** prints `NULL` at top level in GNU R in some cases. This is a visibility difference.
- **Missing functions**: `traceback`, `trace`, `untrace`, `browser`, `globalCallingHandlers`, `warnings`, `dynGet`, `pos.to.env`, `bitwAnd` family, `as.hexmode`, `noquote` + `print.noquote`, `table`, `ls.str`, `summary.default` (now "no applicable method"), `methods()`, `packageDescription`, `installed.packages`, `bibentry`, `citation`, `txtProgressBar`, `flush.console`, `file_test`, `glob2rx`, `combn`, `URLencode`, `osVersion`, `system`, `Sys.chmod`, `Sys.umask`, `Sys.readlink`, `Sys.setLanguage`, `agrep`, `charmatch`, `pmatch`, `max.col`, `env2list`-style helpers.

## 4. Deliverable 2: print and visibility audit

| Item | Status | Root cause and fix |
|---|---|---|
| Print of a list deparses and never calls `show()` on S4 elements | **Confirmed** | B2. `ctx.rs:1052-1062` and `builtins.rs:517` fall back to `deparse`. Implement recursive `print.default`. S4 elements go through `ctx.show_s4`, objects through `print`. |
| Character print hides attributes, for example the `package` attribute on an S4 class name | **Confirmed** | B2(f). No printer writes `attr(,"x")` blocks. |
| `f(invisible(x))` visibility | **Confirmed** | B1. One-line move at `eval.rs:136` (and `:120`). |
| `closure.rs:366` P1-5 | **Confirmed** (one edge case) | B7. Simple `callNextMethod()` cases, a modified value, and explicit arguments already match the oracle. |
| `ops/` P1-8 `flags.s4` | **Confirmed** | B6. `common.rs:265` plus `arithmetic.rs:194-200`. |

## 5. Clean-room spec: `tools/corpus/testthat_shim/`

**Package metadata.**
- `Package: testthat`, `Version: 3.2.99`, `License: MIT + file LICENSE`, `Config/roxide/shim: true`.
- The package name `testthat` lets each package run its own `tests/testthat.R` without change. The version satisfies `Suggests: testthat (>= 3.0.0)`.
- Write the shim from the public testthat reference pages only. Do not read any package source.
- The shim imports nothing (no withr, no rlang), so withr's own tests do not depend on themselves.

**Base functions the shim may use.** Restrict the shim to these functions, so that one file runs the same on both engines:
`tryCatch withCallingHandlers withRestarts invokeRestart signalCondition structure conditionMessage conditionCall inherits identical all.equal isTRUE deparse substitute eval new.env parse(file=, keep.source=FALSE) list.files file.path basename sort grepl regexpr paste paste0 sprintf cat capture.output withVisible vapply lapply seq_along length is.null nchar names class typeof isS4 is.function environment asNamespace isNamespaceLoaded requireNamespace library options getOption Sys.setenv Sys.getenv on.exit quit`
- Do not use `source`, `switch`, `match.arg`, or `setdiff`.
- When `.Machine` does not exist, use the literal `1.490116e-08`.
- **Gate:** B1, B3, B8, `all.equal`, `quit`, and `try` must land before the shim can run on Roxide.

**Files.**
- `R/expectation.R`
  - `expect(ok, failure_message, info, srcref)`, `succeed()`, `fail(message)`.
  - A success signals `c("expectation_success","expectation","condition")` through `withRestarts(signalCondition(e), continue_test=function() NULL)`.
  - A failure is `c("expectation_failure","expectation","error","condition")` and is raised with `stop(e)` inside the same restart.
  - A skip is `c("skip","condition")`.
- `R/expect-equality.R`
  - `expect_equal(object, expected, ..., tolerance=sqrt(.Machine$double.eps), info, label, expected.label)` uses `all.equal` in both editions. Document this difference from waldo.
  - `expect_identical` uses `identical`.
  - `expect_equivalent` uses `all.equal(check.attributes=FALSE)`.
  - Also: `expect_setequal`, `expect_mapequal`.
  - The failure text is `"<label> not equal to <expected.label>.\n<all.equal msgs>"`. Labels come from `paste(deparse(substitute(object), width.cutoff=500L), collapse=" ")`.
- `R/expect-logical.R`: `expect_true`, `expect_false`, `expect_null`, `expect_length`, `expect_named(object, expected, ignore.order, ignore.case)`, `expect_gt`, `expect_gte`, `expect_lt`, `expect_lte`, `expect_type`, `expect_s3_class(object, class, exact)`, `expect_s4_class`, `expect_is` (legacy), `expect_match(object, regexp, perl, fixed, all=TRUE)`, `expect_no_match`.
- `R/expect-conditions.R`
  - `expect_error(object, regexp=NULL, class=NULL, ..., fixed, perl)` returns the condition invisibly.
  - `expect_warning` and `expect_message` capture conditions with `withCallingHandlers` and muffle them. They pass when one condition matches both the class and the regexp. Conditions that do not match propagate.
  - Also: `expect_condition`, `expect_no_error`, `expect_no_warning`, `expect_no_message`, `expect_silent` (no output, no warnings, no messages).
  - `expect_output(object, regexp, fixed)` uses `capture.output` and joins with `"\n"`.
  - `expect_invisible` and `expect_visible` use `withVisible`, so they need B1.
- `R/legacy.R`: magrittr 1.5 era tests use these. `context()`, `expect_that(object, condition)`, `equals`, `is_identical_to`, `is_equivalent_to`, `is_a`, `is_true`, `is_false`, `is_null`, `matches`, `prints_text`, `throws_error`, `gives_warning`, `shows_message`.
- `R/skip.R`: `skip`, `skip_if`, `skip_if_not`, `skip_on_cran` (skips unless `NOT_CRAN=="true"`), `skip_on_os`, `skip_on_ci`, `skip_if_not_installed` (uses `requireNamespace(quietly=TRUE)`), `skip_if_offline` (always skips).
- `R/helpers.R`
  - `local_edition`, `edition_get`, `is_testing`, `testing_package`, `test_path(...)`.
  - `local_reproducible_output(width=80)` sets `width`, `crayon.enabled=FALSE`, `cli.num_colors=1`, `useFancyQuotes=FALSE`, and `OutDec="."`, and restores them with `on.exit` in the caller through `do.call(on.exit, ..., envir=)`.
  - `capture_output`, `capture_messages`, `capture_warnings`, `evaluate_promise`.
  - `expect_snapshot*` → `skip("snapshots unsupported")`.
- `R/runner.R`
  - `test_that(desc, code)` evaluates `code` in `new.env(parent=<file env>)`.
    - Expectation handlers record a result and call `invokeRestart("continue_test")`.
    - The warning handler records the warning and muffles it. The message handler muffles.
    - An outer `tryCatch` turns a `skip` into SKIP and an error into ERROR. The run goes on to the next test.
  - `test_file(path, env)`, `test_dir(path, package, stop_on_failure=TRUE)`, `test_check(package)`:
    1. Set `TESTTHAT="true"`.
    2. Load the package with `library(package)`.
    3. Create a file env that is a child of `asNamespace(package)`, so internal functions are visible.
    4. Evaluate the `helper*.R` files, then the `setup*.R` files, then the `test*.R` files. Use byte-order sort (do not use locale sort), match pattern `^test.*\.[rR]$`, and evaluate each top-level expression with `eval`.
- `R/reporter.R`
  - ASCII only and deterministic: `ok|not ok|skip <n> - <file>: <desc>`. A failure adds indented message lines.
  - The last line is `[ FAIL f | WARN w | SKIP s | PASS p ]`.
  - When `f>0`, exit with `quit(status=1)`.
- `tests/`: shim self-tests. Each one passes on GNU R 4.6.2 first. The harness then compares the reporter output byte for byte against Roxide.

## 6. Suggested build order (disjoint owners)

1. **Coordinator / eval (one owner, small diffs):** B1, B4, B7, then the `language.rs`, `environment.rs`, and `conditions.rs` Tier-A functions, including `switch`.
2. **Base workers (parallel):**
   - `subset/`: sets, apply family, `unlist`, `.subset2`, B5.
   - `ops/`: B6, B8, `mean`.
   - `strings/`: B9 and the string functions.
   - `io_os/`: B3, B10, `sink`, `quit`, `source`, files, locale.
   - `printing.rs` + `ctx.rs` autoprint: B2, B13.
3. **utils worker:** `pkgutils.rs`.
4. **Shim author:** can write and validate the shim on GNU R now. Gate it on Roxide after steps 1–2.
5. **Gate:** rerun `probe1.R`, `probe2.R`, `probe3.R`, `probe7.R`, `vis.R`, `cnm.R`, and `s4f.R`, then diff Roxide against the oracle. All of them must match, except for addresses and the Tier-B items. Probe files: `/private/tmp/claude-501/-Users-devg-Documents-Roxide/ade2f088-87ef-44c4-9d7c-33179e9d49ac/scratchpad/`.

**Limits of this audit.**
- I did not inspect the target packages' source, because the oracle does not have them installed and the brief excluded it. The per-package needs above come from each package's public API and documented behavior.
- `assertthat` is GPL-3, so its spec must come from its documentation only.
- After Tier-A lands, run each package's install and load once against Roxide to find gaps that this list does not cover.
