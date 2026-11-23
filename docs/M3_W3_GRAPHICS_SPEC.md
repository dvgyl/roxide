# Roxide M3 Wave 2 review (W2-D `d1b0b57`, W2-E `7059989`) and Wave 3 contract

I did not edit files, run Cargo, or commit. I read the two commits. I compared about 15 probe scripts on two binaries: GNU R 4.6.2 (`Rscript --vanilla`) and the prebuilt `target/debug/roxide`. That binary was built at 21:19, so it also contains the uncommitted working-tree changes.

## Part 1: Review findings

### What matches the oracle (Roxide gives the same values as R)

- **`par()` coupling:**
  - `csi` changes at `plot.new()`, not at `par(cex=)`.
  - `mai` follows `csi*mex`.
  - `cxy` is `cin*usr/pin` with no `cex` factor, as R does.
  - `mfrow`/`mfcol` reset `cex` (0.83 for 2×2, 0.66 for 3 or more) and `mex`.
  - After `mfrow`, `mfg` is `c(nr,nc,nr,nc)`.
  - `fig` is relative to the inner region.
  - `fig` resets `mfrow` to `c(1,1)`.
  - `omd` and `fin` match when `oma` is set.
- **`layout()`:** `respect=TRUE` mixed with `lcm()`, matrix `respect` without `lcm`, 2×2 matrix `respect`, partial overfull `lcm`, the "figure region too large" error, figure IDs, and zero cells all match.
- **Log axes:**
  - `xaxp` and `axTicks` match for 8 ranges: 1–1e3, 1–10, 1–1e6, 2–30, 1–1e12, 0.5–3 (gives `n=-5`), 1e-3–1e3, and the reversed range 1000–1.
  - `par(usr=)` on a log axis gives linear ticks on `10^usr`, which is R's odd behavior (`200 1000 4`).
  - Both give the same error for `axTicks` with `n=4`.
- **`grconvertX`/`grconvertY`:** `user`, `ndc`, `inches`, `lines`, `nic`, and `npc` match, including on log axes. `"chars"` is wrong (P1-1 below).
- **`strwidth`/`strheight`:** AFM widths match, with kerning (`AV`, `bb`), bold, `cex`, multi-line text, an empty string (cap height), and the `figure`/`user` units.
- **Grid units:**
  - The internal `list(value, data, code)` form and the `simpleUnit` class order match.
  - `sum`, `min`, `max`, `unit.pmin`, `rep`, negation, scaling of sums, and nested sums match.
  - `absolute.size` and `null` inside `min`/`max` conversion match.
- **Grid geometry:**
  - `gpar`: `cex` multiplies down the stack (2×3=6), a child `fontsize` replaces the old value and keeps `cex`, and `char` = 40.15 pt.
  - A layout child ignores its own `x`/`y`/`width`/`height`.
  - `layout.pos` is ignored when the parent has no layout.
  - The rotated `current.transform()` matrix matches.
  - All 35 `viewport` field names match.
- **GC safety in W2-D:** `record()` protects each new cons cell. Display-list entries are traced. `GPar` holds no `SexpId`. I found no rooting bug.

### P0 (blocks Wave 3; the cause is outside W2-D/E)

- **P0-1: base `format()` and `cat()` format numbers incorrectly.** Default `axis()` labels are `format(at, trim=TRUE)`, so this blocks Wave 3 axis labels.

  | Call | GNU R | Roxide |
  |---|---|---|
  | `format(c(1,1e6))` | `"1e+00" "1e+06"` | `"1" "1e+06"` |
  | `format(c(0.001,0.0015,0.002))` | `"0.0010" "0.0015" "0.0020"` | `"0.001" "0.0015" "0.002"` |
  | `format(c(1e5,1.5e5,3e5))` | `"100000" "150000" "300000"` | `"1e+05" "150000" "3e+05"` |
  | `cat(1e6)` | `1e+06` | `1000000` |

  Roxide does not use one common format (decimals and fixed-or-scientific choice) across the vector. Also, `print(matrix)` formats all columns together, but R formats each column separately.

### P1

1. **The `"chars"` unit uses the character width on the x axis.** [coords.rs:327](/Users/devg/Documents/Roxide/crates/roxide-graphics/src/graphics/coords.rs:327) uses `char_size(axis)`, which gives `cin[1]` on x. R uses the character height `cin[2]*cex` on both axes.
   - `grconvertX(1,"chars","inches")` with `mex=2`: R gives 0.2, Roxide gives 0.15.
   - The same call with `cex=2`: R gives 0.4, Roxide gives 0.3.
   - Fix: use `par.char_size(1)` for both axes.
2. **`par("mar","cex")` fails.** [graphics/prelude.R:5](/Users/devg/Documents/Roxide/crates/roxide-graphics/src/graphics/prelude.R:5) calls `unlist`, and Roxide base has no `unlist`. The error is "could not find function "unlist"".
   - My probes also found that base has no `try`, `proc.time`, or `Sys.time`. These are coordinator items.
3. **The display list is not ready for `recordPlot`/`replayPlot`.** There are four gaps:
   - `par()` and `layout()` changes are not recorded, and Roxide keeps no snapshot of the graphics state. R's `recordedplot[[2]]` is a state snapshot. When I replayed a 2-panel recording, R restored `mfrow=c(1,2)`, even with an empty display list.
   - [coords.rs:53](/Users/devg/Documents/Roxide/crates/roxide-graphics/src/graphics/coords.rs:53) turns recording on at each new page. In R, file devices (`pdf`, `png`, `svg`) do not record until `dev.control("enable")`. My pdf probe gave a display list of length 0.
   - A replayed call runs `record()` again, so a replay records each entry a second time unless recording is paused.
   - Contract for the fix is in Part 2.
4. **Grid is too slow under `ROXIDE_GC_TORTURE=1`.** This is the root cause of the slow `20_grid_units_viewports.rcase`. There is no leak and no rooting bug. The cost is (number of safepoints) × (one full mark-sweep):
   - Torture mode runs a full non-generational collection at each safepoint. Safepoints are at each `eval_protected`, each promise force, and each closure apply ([eval.rs:60](/Users/devg/Documents/Roxide/crates/roxide-eval/src/eval.rs:60), [eval.rs:150](/Users/devg/Documents/Roxide/crates/roxide-eval/src/eval.rs:150), [eval.rs:344](/Users/devg/Documents/Roxide/crates/roxide-eval/src/eval.rs:344)).
   - With all preludes loaded, one collection takes about 30 ms. Startup alone takes 16–23 s.
   - Measured cost of one call under torture:

     | Call | Time |
     |---|---|
     | trivial closure | 0.11 s |
     | `.grid_match` | 0.26 s |
     | `%.grid_in%` | 0.5 s |
     | `.grid_codes` | 0.7 s |
     | simple `unit()` | 2.8 s |
     | `.grid_items` on 3 elements | +4.1 s |
     | `viewport()` | 29 s |
     | `grid.newpage()` (builds a root `viewport()`) | 28 s |
     | `pushViewport` | 12 s |
     | `convertX`/`convertWidth` | 11–15 s |
     | mixed unit add | 13 s |
     | `grid.layout` | 9 s |
     | push of a viewport with a layout | 40 s |

   - Case 3 makes 6 `box()` calls. Each call does viewport + push + 2 conversions + pop, so the case takes many minutes.
   - The hot spots are in [grid/prelude.R](/Users/devg/Documents/Roxide/crates/roxide-graphics/src/grid/prelude.R):
     - Wrapper chains: `%.grid_in%` → `.grid_match` → `.Internal` at :1 and :7. Also `.grid_rep_len` (:6), `.grid_recycle` (:393), and `.grid_font_params` (:395).
     - Per-element closures: `.grid_items` (:93) turns each `simpleUnit` into a list of lists on every measure.
     - `.grid_measure` (:409) makes more than 6 `%.grid_in%` tests per item.
     - `convertUnit` (:467) measures twice per item, because `.grid_inverse` (:453) calls `.grid_measure` again.
     - `viewport()` (:622) calls `unit()` 4 times, plus 2 calls to `.grid_match_arg` and a 35-field list.
     - `.grid_node` (:761) does matrix algebra in R.
   - Fixes, in order of effect:
     - (a) Move unit evaluation and `convertUnit` into one Rust `.Internal` that walks the unit list once.
     - (b) Move `.grid_node` (size, transform, gpar inheritance, clip) into a Rust `C_grid_push`.
     - (c) Use integer range tests such as `code >= 14L && code <= 17L` in place of `%.grid_in%`.
     - (d) Keep `simpleUnit` as a vector, without `.grid_items`.
     - (e) Coordinator/ADR item: let torture collect every N safepoints, for example `ROXIDE_GC_TORTURE=N` like R's `gctorture2(step)`. This keeps the torture gate practical.

### P2

1. **`xlog`/`ylog` toggling** ([par.rs:816](/Users/devg/Documents/Roxide/crates/roxide-graphics/src/graphics/par.rs:816)) recomputes `usr`. R keeps a linear `usr` and a separate `logusr`, and the toggle only changes which one is reported.
   - Example: after `plot.new()`, `par(xlog=TRUE)` gives R `usr = 3.12 -0.12`, which is a stale `logusr` from an earlier plot. Roxide gives `0 0.017`.
   - `par(xlog=FALSE)` gives R `-0.04 1.04`. Roxide gives `1 1.04`.
2. **Grid messages differ from R:**
   - `popViewport` should append `" ('grid' and 'graphics' output mixed?)"` (:861).
   - `unit/unit` should say "only one operand may be a unit".
   - `2/unit` should say "can't divide with a unit" (:255).
   - The rotated-clip warning shows the internal `.grid_node(vp, parent, state$root)` call. R shows `grid.Call.graphics(C_setviewport, vp, TRUE)`.
3. **Device slot removal:** `plot_new` and `viewport::page` take the slot out of the table during `new_page`. While it is out, its display list and grid state are not traced. This is safe now, because `new_page` gets only the host. Add a comment or a guard so that this stays true.

## Part 2: Wave 3 contracts (all values are from GNU R 4.6.2)

### `hist(x, plot=FALSE)`

- **Return value:** a list with class `"histogram"`. Element order: `breaks, counts (int), density, mids, xname, equidist`.
  - `density = counts/(n*diff(breaks))`.
  - `xname` is `deparse(substitute(x))`, for example `"y * 2"` or `"c(1, 2, 3)"`.
- **Example:** `x <- c(1,2,2,3,3,3,4,4,5,10)` gives:
  - `breaks = 0,2,4,6,8,10`
  - `counts = 3,5,1,0,1`
  - `density = .15,.25,.05,0,.05`
  - `mids = 1,3,5,7,9`
- **Non-equal breaks:** `breaks=c(0,2,5,10)` gives `counts 3,6,1`, `density .15,.2,.02`, and `equidist=FALSE`. Without `plot`, there is no warning.
  - If `plot=TRUE` and `freq=TRUE` with non-equal breaks, R warns: `the AREAS in the plot are wrong -- rather use 'freq = FALSE'`.
  - If `plot=FALSE` gets a plotting argument, R warns ``argument ‘freq’ is not made use of`` (or `‘col’`, and so on). The quotes are UTF-8 curly quotes.
- **`breaks` forms:**
  - A number or a string (`"Sturges"`, `"Scott"`, `"FD"`): use `pretty(range(x), n=k, min.n=1)`. `breaks=3` and `"Scott"` give `0,5,10`. `"FD"` gives `0,2,…,10`.
  - A function that returns a vector: use the vector as the breaks.
  - A function that returns a number: use the number as `k`.
  - `hist(rep(5,4))` gives `breaks c(0,5)`, `counts 4`.
- **Counting rules:**
  - Default `right=TRUE`, `include.lowest=TRUE`. With `right=FALSE` the last bin is closed.
  - `include.lowest=FALSE` with a value on the first break gives the error `some 'x' not counted; maybe 'breaks' do not span range of 'x'`. Breaks that do not cover the data give the same error.
  - For fuzz, use spec §1.
- **Input checks:**
  - `NA` and `Inf` are removed silently.
  - `numeric(0)` gives `invalid number of 'breaks'`.
  - A character input gives `'x' must be numeric`.
- **Visibility:** with `plot=TRUE`, the result is invisible.

### `boxplot(..., plot=FALSE)`, `boxplot.stats`, and `bxp`

- **`boxplot` return value:** `list(stats = 5×g double matrix, n = double vector, conf = 2×g matrix, out = double, group = double, names = chr)`.
  - `n` is double here. In `boxplot.stats`, `n` is `11L` (integer).
  - `conf = med ± 1.58*IQR/sqrt(n)`. For `c(1:10,30)`, `conf = 3.618…, 8.382…`.
  - Example: `list(a=c(1:10,30), b=c(2,4,6,8))` gives `stats = 1,3.5,6,8.5,10 | 2,3,5,7,8`, `n = c(11,4)`, `out = 30`, `group = 1`.
- **Names:**
  - One unnamed vector gives `names "1"`.
  - A matrix without column names gives `"1","2"`.
  - A data frame gives its column names.
  - A formula gives the factor levels.
- **Special cases:**
  - An empty group gives a column of `NA` in `stats` and `conf`, and `n = 0`.
  - `range=0` gives no outliers.
  - `boxplot.stats(coef=0)` sets the whiskers to the full range.
- **`bxp(b)`** returns the box positions invisibly: `1:n`, or `at` when given.
- **Plot window:**
  - Box axis: `usr` is `c(.5, n+.5)` with 4% expansion. For n=2 this gives `0.42 2.58`.
  - Value axis: the range includes the outliers, with 4% expansion.
  - `horizontal=TRUE` exchanges the two axes.
  - `boxplot()` returns invisibly.

### `barplot()`

- **Return value:** the bar midpoints, invisibly.
  - A vector input gives an n×1 **matrix** (`dim c(n,1)`).
  - A stacked matrix gives a plain **vector**, one value per column.
  - `beside=TRUE` gives a matrix with the same `dim` as `height`.
- **Bar positions:** gaps are `space*mean(width)`. Default `space` is 0.2. With `beside`, the default is `c(0,1)`: 0 inside a group and 1 between groups.
  - `c(3,5,2)` gives `0.7, 1.9, 3.1`.
  - `space=.5, width=2` gives `2, 5, 8`.
  - `width=c(1,2,3)` gives `0.9, 2.8, 5.7`.
  - A 2×3 matrix with `beside` gives `1.5,2.5 | 4.5,5.5 | 7.5,8.5`.
  - `space=c(.2,1)` gives `1.5,2.7 | 4.7,5.9 | 7.9,9.1`.
- **Plot window:**
  - Category axis: from the first bar edge to the last bar edge, with `xaxs="r"` (4%).
  - Value axis: `range(-0.01*height, height, 0)` with `yaxs="i"`. `c(3,5,2)` gives `-0.05 5`. `c(-2,3)` gives `-2 3`.
  - A log value axis starts at `0.9*min`.
  - `horiz=TRUE` exchanges the axes and gives the same midpoints.

### `legend()`

- **Return value:** invisible `list(rect=list(w,h,left,top), text=list(x,y))`. The coordinates are in **`usr` space**, which is log10 on a log axis. For example, `left=-0.08, top=2.08` for `log="xy"`.
- **Formulas** (checked on the pdf probes, inset 0, `x.intersp=1`, `y.intersp=1`, `seg.len=2`):
  - `xc = cex*cin[1]*diff(usr[1:2])/pin[1]` and `yc = cex*cin[2]*diff(usr[3:4])/pin[2]`.
  - `ychar = max(yc, strheight)`.
  - `h = (rows + has_title)*ychar + yc`.
  - Width of one column: `w0 = maxstrwidth + (1 + x.intersp + [seg.len if lines])*xc`. Total `w = ncol*w0 + 0.5*xc`.
  - Text position: `text.x = left + (1 + x.intersp + [seg.len])*xc` and `text.y[i] = top - i*ychar`.
  - `"topright"` sets `left = usr[2] - w` and `top = usr[4]`.
  - Example: `plot(1:10)` with `legend("topright", c("alpha","b"), lty=1:2)` gives `w=1.827, h=1.1302, left=8.533, top=10.36`, `text.x=9.5455`, `text.y=9.9833, 9.6065`.
- **Other layouts:** probe the oracle again for `fill` boxes and `horiz`. My data for one probe gave `w=2.4823125` and `text.x=4.968, 6.145`, and my derivation of the `fill` width did not close.
- **Bad position:** gives ``'arg' should be one of “bottomright”, “bottom”, …, “center”``.

### `axis()`

- **Return value:** the `at` positions, invisibly. These are `axTicks(side)` when `at` is omitted.
  - Explicit `at` is **sorted**, with `NA` last. Values outside `usr` are kept: `c(NA,3,20)` gives `c(3,20,NA)`.
  - `labels=FALSE` returns the same value.
- **Default labels:** `format(at, trim=TRUE)`, with common decimals and one fixed-or-scientific choice. This needs P0-1 fixed first.
  - Examples: `0.0010 … 0.0030`, `-1.0 … 1.0`, `0e+00 2e+04 …`, `100000 150000 …`, `1e+06 …`.
  - A log axis gives `1e+00 … 1e+05`.
  - Numeric `labels=` are formatted one at a time with `as.character`: `0.3`, `123456789`, `2e+08`.

### `recordPlot()` and `replayPlot()`

- **Structure:** `class "recordedplot"`, a list of 2.
  - `[[1]]` is a pairlist of the display-list entries.
  - `[[2]]` is an opaque snapshot of the graphics state, with the attribute `pkgName "graphics"`.
  - Attributes: `engineVersion` (17L), `pid`, `Rversion`, `load` (`character(0)`), `attach` (`character(0)`).
  - Roxide can use any opaque snapshot, but it must keep the same class and attributes.
- **Replay:** `replayPlot()` restores the snapshot first: `mfrow`, `mfg`, and `usr`. Then it runs the display list with recording paused.
- **Errors:** `replayPlot(1)` gives ``argument is not of class “recordedplot”``.
- **File devices:** these start with the display list inhibited.

### Grid grobs

- **`grob(name=, cl="foo")`:**
  - Class is `c("foo","grob","gDesc")`.
  - Fields are `name, gp, vp`.
  - Default names are `GRID.grob.<k>`, from one global counter for each type: `GRID.gTree.1`, and so on.
- **Field order of common grobs:**
  - `rectGrob`: `x,y,width,height,just,hjust,vjust,name,gp,vp`.
  - `textGrob`: `label,x,y,just,hjust,vjust,rot,check.overlap,name,gp,vp`.
- **`gTree`:**
  - Fields are `name, gp, vp, children (class "gList", named), childrenOrder`.
  - Duplicate child names are accepted, with `childrenOrder c("a","a")`.
- **`gList`:**
  - `NULL` items are dropped.
  - A non-grob item gives the error `only 'grobs' allowed in "gList"`.
- **`gPath("gt","t")`:**
  - Value is `list(path="gt", name="t", n=2L)` with class `c("gPath","path")`.
  - It prints `gt::t`.
- **Grob measures** (pdf 7×7, Helvetica 12 pt):

  | Call | Result |
  |---|---|
  | `grobWidth(textGrob("hello"))` | unit `"1grobwidth"`, type `grobwidth`; converts to 0.352 in |
  | `grobHeight` of the same grob | 0.11967 in (cap height) |
  | `rectGrob()` width | 7 in |
  | `circleGrob(r=1in)` width | 2 in |
  | `linesGrob` | bounding box of its points |
  | text with `rot=90` | width = text height |
  | text with `fontsize=24` | width 0.704 in |
  | `grobAscent("g")` | 538/1000 em |
  | `grobDescent("g")` | 220/1000 em |
  | `nullGrob` | 0 |
  | `gTree` with no methods | width 0, and `grobX(..., "west")` = 0.5 npc |
  | grob with `vp=viewport(width=.5)` | measured inside that `vp` |
  | `grobX(r,0)` and `grobX(r,"west")` | angle or compass name: 0 or 7 in for the default rect |

  - Odd case: several text labels at **the same anchor** measure only the first label. Labels at different positions measure the full bounding box.
  - Note: `pointsGrob` uses `default.units="native"` by default.
- **`grid.ls(print=FALSE)`:**
  - Returns class `"flatGridListing"`, a list of `name, gDepth (dbl), vpDepth (dbl), gPath, vpPath, type`.
  - `type` is one of `gTreeListing`, `grobListing`, `vpListing`, `vpUpListing`.
  - With `viewports=TRUE`, the list starts with `ROOT`, and each pushed viewport adds a `"1"` `vpUpListing` row.
  - The printed form indents 2 spaces for each level.
  - Children that `makeContent` adds are **not** listed.
  - An empty page gives zero-length columns.
- **Drawing behavior:**
  - `makeContext` can set `x$vp`. That viewport shows in `grid.ls(viewports=TRUE)`.
  - `makeContent` runs at draw time.
  - `grid.get` of a missing name returns `NULL`, not an error.
  - `grid.edit` changes the display-list copy.
  - `grid.remove` deletes the grob, and `getNames()` changes to match.
- **Axis grobs:**
  - `xaxisGrob()` is class `c("xaxis","axis","gTree","grob","gDesc")`.
  - Fields are `at, label, main, edits, name, gp, vp, children, childrenOrder`.
  - `at` and `children` are `NULL` after creation, and also after `grid.xaxis` (`grid.get` also gives `NULL`). The ticks and labels are built only in `makeContent.xaxis` at draw time.
- **Grab:**
  - `grid.grab()` returns a `gTree` with the drawn top-level grobs as children: `c("gt","outer")`.
  - `grid.grabExpr(expr)` does the same off-screen: `"zz"`.

My probe scripts and Roxide timing logs are in `/private/tmp/claude-501/-Users-devg-Documents-Roxide/90fc8f32-ce8c-4660-b946-f6db904fa237/scratchpad/`.
