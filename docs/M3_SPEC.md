# Milestone 3 clean-room oracle specification

**Oracle:** GNU R 4.6.2, dated 2026-10-29, on macOS arm64. I ran the R probes with `$ROXIDE_ORACLE_RSCRIPT --vanilla` after sourcing `env/env.sh`. I used a temporary launcher for the official `R CMD` dispatcher and temporary X11 libraries for the Cairo device. The probes wrote only to temporary directories. I made no repository edits and ran no Cargo commands.

## Track A — S4 and reference classes

### Object representation and construction

Use this fixture for the ordinary S4 object tests:

```r
setClass("A", slots = c(x = "numeric", y = "character", z = "ANY", n = "NULL"),
         prototype = list(x = 2))
a <- new("A")
```

| Probe | GNU R 4.6.2 result |
|---|---|
| `typeof(a)`, `isS4(a)` | `"S4"`, `TRUE` |
| `class(a)` | `structure("A", package = ".GlobalEnv")`. A class defined inside package `rxs4` had `package = "rxs4"`. |
| `attributes(a)` | In order, `x = 2`, `y = character(0)`, `z` and `n` as the symbol `\001NULL\001`, then `class`. `a@n` returns `NULL`. Preserve the distinction between a present null slot and an absent slot. |
| `slotNames(a)`, `getSlots("A")` | `c("x", "y", "z", "n")`, `c(x="numeric", y="character", z="ANY", n="NULL")`. |
| `.hasSlot(a, "n")`, `.hasSlot(a, "foo")` | `TRUE`, `FALSE`. `a@foo` errors. |
| `a@x <- 2L` | Accepted. The stored value remains integer. Assignment of `"s"` errors. |
| `slot(a, "foo", check=FALSE) <- 5` | `a@foo` returns `5` and `.hasSlot(a, "foo")` becomes `TRUE`. `slotNames(a)` still reports the declared slots. |
| `setClass("Vec", contains="numeric", slots=c(tag="character"))`; `v <- new("Vec", 1:3, tag="a")` | `typeof(v) == "integer"` and `isS4(v)` is `TRUE`. `slotNames(v)` is `c(".Data", "tag")`. `v@.Data` and `as(v, "numeric")` return `1:3`. The attributes are `tag` and `class`. |

These cases require an S4 flag that is independent of the storage type, package-qualified class identity, and access to the underlying vector through `.Data`. [R Internals](https://cran.r-project.org/doc/manuals/r-release/R-ints.html#S4-objects)

- `setClass` accepts `slots=` and `representation()`. Both a list and `prototype()` supplied default slot values. With `D` declared as `contains=c("A","V")` and local slot `v`, the slot order was `v,x,y,z,n,t`, where `t` came from `V`. `extends("D")` returned `c("D","A","V")`.
- A slotless `setClass("E")`, a class that explicitly contains `"VIRTUAL"`, and a `setClassUnion` class all rejected `new()`. A concrete subclass could initialize a slot inherited from a virtual superclass. A union of `numeric` and `character` accepted `1L` and `"s"`. `as(1L, union)` returned `1L`.
- `setOldClass(c("oldSub","oldBase"))` registered inheritance for an S3 object with those classes. `is(obj,"oldBase")` was `TRUE`, `isS4(obj)` was `FALSE`, and `new("oldSub")` failed because the registered class was virtual.
- `new()` applies the prototype and dispatches `initialize`. Named arguments initialize slots. An unnamed compatible superclass object supplies inherited data. An unknown slot or unrelated unnamed object errors. A custom `initialize` that called `callNextMethod(.Object, x=11, ...)` set `x=11` and preserved the caller’s named slot arguments.
- Checked assignment validates the slot type. Class validity is a separate step. A class with the validity rule `j > 0` rejected `new("J", j=0)`. Assignment `j@j <- 0` succeeded, and `validObject(j, test=TRUE)` returned `"j must be positive"`. `complete=TRUE` also detected an invalid nested S4 slot that passed the default check.
- `as(new("D"), "A")` returned an `A` with its slots. `strict=FALSE` retained the `D` object. A `setAs("A","character", ...)` method also served `D`. Explicit `show()`, `print()`, and top-level auto-print called the registered `show` method. `invisible()` suppressed auto-print.

### Dispatch and `callNextMethod`

`setGeneric(..., signature=...)` defines the order of the dispatch arguments. `setMethod` accepts named signatures and maps them into that order. The resolver dispatches an omitted argument as `missing`, even when the formal has a default. In the selected method, the default was available and `missing(y)` was still `TRUE`. An unused argument outside the signature remained unforced. `selectMethod(..., useInherited=FALSE, optional=TRUE)` returned `NULL` when only inherited matches existed.

For the diamond, `L` and `R` each directly contain `Root`. `D` contains `c("L","R")`, and `E` contains `c("R","L")`. Both diamonds report parent distances 1 and root distance 2.

| Target or comparison | Candidate signatures or distances | Selected |
|---|---|---|
| `(D,D)` | `(R,ANY)`, `(L,ANY)` | `(L,ANY)` |
| `(E,E)` | Same methods | `(R,ANY)` |
| `(D,D)` | `(D,Root)`, `(L,D)` | `(L,D)` |
| `(D,D)`, signature `x,y` | `(L,D)`, `(D,L)` | `(D,L)` |
| Same objects and named methods, signature `y,x` | Same methods | The method with `y=D, x=L` |
| `(D,D)` | `(L,R)`, `(R,L)` | `(L,R)`. Target `(E,E)` selects `(R,L)`. |
| `(D,D)` | `(D,ANY)`, `(L,D)` | `(L,D)` |
| `(D,missing)` and `(E,missing)` | `(D,ANY)`, `(ANY,missing)` | `(D,ANY)` for `D`. `(ANY,missing)` for `E`. |
| Linear hierarchy | `(0,2)` vs `(1,1)` | `(0,2)` |
| Linear hierarchy | `(0,3)` vs `(1,1)` | `(1,1)` |
| Linear hierarchy | `(0,5)` vs `(5,0)` vs `(2,2)` | `(2,2)` |
| Three dispatch arguments | `(0,0,4)` vs `(0,2,2)` | First with signature `x,y,z`. Second with signature `z,y,x`. |

**Resolver inference.** The tested ordinary hierarchies fit minimum total inheritance distance. For equal totals, the results fit a preference for a candidate with at least one exact component, then comparison in signature order. Equal-distance superclasses follow the declared parent order in the tested diamonds. Counting all exact components would fail the last case. In an additional probe with six direct parents at distance 1, `(Wide,ANY)` beat `(W1,W1)`. In a linear hierarchy of depth 6, `(ANY,exact)` lost to `(3,3)` and beat `(4,4)`. These results fit an `ANY` distance of one more than the deepest superclass in those fixtures. Keep the table as the acceptance contract for the inference. Competing applicable signatures can produce a method-selection message.

`callNextMethod()` resolved from the defining method’s signature. A `D` method that reassigned its local `x` to an `R` instance still called the `L` method next, then `Root`. The next method received the current `R` value. Calling `callNextMethod(x, "explicit")` changed the arguments but selected the same next method. Adding and removing an exact method changed the next ordinary dispatch result immediately.

### Reference classes

- `setRefClass` returns a generator. `$new(n=2)`, generator call syntax, and `new("Count", n=2)` constructed equivalent instances in the probe. `typeof(obj)` was `"S4"` and `slotNames(obj)` returned `".xData"`. The `.xData` slot is an environment. `identical(obj, obj$.self)` was `TRUE`.
- Aliases share the same fields. After `b <- a`, `b$incr()` changed `a$n` from `2` to `3`. A typed `numeric` field used an active binding and rejected a character value. A function field also used an active binding and received a write value. Unknown field reads and writes raised errors.
- `$copy()` created independent field storage. A directly held reference-class field was copied by default and shared with `shallow=TRUE`. A custom binding that rejected writes caused `$copy()` to error in the fixture.
- `contains=` inherited fields and methods. A subclass override that called `callSuper(by * 2)` advanced the inherited counter by twice the input increment.

## Track B — Namespaces, packages, and `R CMD`

### `NAMESPACE` grammar

Parse directives as R expressions. Interpret the selected directives. A braced group works at top level and in `if` branches. `if (getRversion() >= "4.6.2") export(new) else export(old)` selected `new`. An unknown directive in a false branch was ignored. An unknown directive in a selected branch errored. Symbols and quoted strings work as names. `export(c(a,b))` parsed as one name, `"c(a, b)"`.

| Directive | Contract |
|---|---|
| `export(a, "b")`, `exportPattern("^pub")` | Explicit names and names that match the pattern enter the export table. `pub` and `pub2` exported in the fixture. `hidden` remained internal. |
| `exportClasses(A)`, `exportMethods(f)` | Export formal class definitions and method metadata. The fixture exposed `.__C__PkgObj` and `.__T__score:rxs4`. Its locally defined generic `score` was also callable through `::`. |
| `import(p1, p2, except=c(a,b))` | Import each package’s exports, subject to the exclusions. |
| `importFrom(p, f, "g")` | Import the selected names. The fixture resolved `median` through its imports environment. |
| `importClassesFrom(p, A)`, `importMethodsFrom(p, f)` | Import selected S4 definitions and methods. A locally defined generic for an imported method also imports. A package extended an imported class and overrode its imported generic. The override returned `10` through both packages’ call paths. |
| `S3method(print, A)`; `S3method(summary, B, bSummary)` | Register `print.A` by convention, or register the explicit function. Both worked while the defining package was unattached. Other arities errored. |
| `S3method(rxgen::talk, A, talkA)` | Defer registration while `rxgen` is unloaded. The first `rxgen::talk()` loaded the provider and used the registered method. The parser split bare `rxgen::talk` into provider and generic. A quoted `"rxgen::talk"` remained a literal generic name and failed lookup in the fixture. |
| `useDynLib(p, foo, alias=bar)`; `useDynLib(p, .registration=TRUE, .fixes="C_")` | Preserve the library name, native symbol mappings, registration flag, and name fix. The oracle parsed `alias=bar` as an R binding named `alias` for symbol `bar`. This probe covered parsing for Milestone 4. |

The registered deferred S3 method also survived unloading its defining namespace in the fixture while the generic’s namespace remained loaded. [Writing R Extensions — package namespaces](https://cran.r-project.org/doc/manuals/r-release/R-exts.html#Package-namespaces)

### Load, attach, and unload

- The lexical chain is **namespace → imports environment → base namespace → `.GlobalEnv` → search path**. `environmentName()` reported `rxmeth`, `imports:rxmeth`, `base`, and `R_GlobalEnv`. A closure saw its internal value, its imported `median`, and a fallback binding in `.GlobalEnv`, yielding `c(9,2,501)`.
- `loadNamespace()` registers and locks a namespace, runs `.onLoad`, returns the environment visibly, and reuses it on subsequent loads. `loadedNamespaces()` includes loaded but unattached packages. `::` loads and looks in exports. `:::` also resolved the internal binding. Neither operator attached the package in these probes.
- `attachNamespace()` added a separate `package:rxmeth` environment to `search()` and ran `.onAttach`. Only exports appeared in the fixture’s attached environment. Exported closures retained their namespace as their enclosing environment.
- `library()` loads and attaches. Its default successful result was an invisible package-name vector. With `logical.return=TRUE` it returned a visible logical. A missing package raised an error by default. `require()` and `requireNamespace()` returned invisible logicals. Only `require()` attaches.
- `unloadNamespace()` detached the attached fixture, ran `.onDetach` and then `.onUnload`, and removed the namespace registration. It refused to unload a namespace while a loaded package imported it.
- `use("grid", include.only=c("unit","viewport"))` returned invisible `TRUE` and attached exactly those two names in a fresh session. The namespace still exposed its full export table. A second `use()` call with a different selection left that attachment in place. Requesting an unknown export on first attach returned invisible `FALSE` and left the package unattached. The package argument was evaluated. An unbound bare name errored.

### Installation and batch execution

| Probe | Result |
|---|---|
| `R CMD INSTALL --no-byte-compile --library=<temp-lib> <source-dir>` | Status `0`. Staged installation, lazy-load generation, and load checks from the staging and final locations. The installed function returned `42L` through `::` in a fresh session. |
| Source layout | `DESCRIPTION`, `NAMESPACE`, and `R/probe.R` sufficed for the pure-R fixture. Honor source order and `DESCRIPTION`’s `Collate` when present. |
| Installed layout | `DESCRIPTION`, `NAMESPACE`, `Meta/package.rds`, `Meta/nsInfo.rds`, `R/<pkg>`, `R/<pkg>.rdb`, and `R/<pkg>.rdx`, plus help and index files. A separate handcrafted fixture with `Meta/package.rds` and direct source in `R/<pkg>` also loaded. |
| Lazy-load database | The `.rdx` index is readable with `readRDS()`. Observed keys were `variables`, `references`, `compressed`. An entry contains two integers, the byte offset and length in `.rdb`. `compressed` was `TRUE` in the fixture. Create promises for the indexed objects and load each record on demand. |
| `R CMD BATCH --vanilla sample.R [outfile]` | Combined standard output and error in the output file. Default name was `sample.Rout`. Success returned status `0` and appended timing. `stop("PROBE")` returned status `1`, with `Error: PROBE` and `Execution halted` in the output. Dispatcher help reports default `--restore --save --no-readline` and supports `--no-timing`. |

For installed-package interoperability, read the GNU R lazy-load format in addition to supporting direct-source fixtures. [Writing R Extensions](https://cran.r-project.org/doc/manuals/r-release/R-exts.html), [R Internals — lazy loading and installed packages](https://cran.r-project.org/doc/manuals/r-release/R-ints.html#Lazy-loading)

## Track C — Graphics, devices, and `grid`

### Device state and file formats

- With no open device, `dev.list()` returns `NULL`. `dev.cur()`, `dev.next()`, and `dev.prev()` return `c("null device"=1L)`. `dev.off(1)` errors. `dev.size()` opened the configured default device on demand and returned its size.
- Device IDs started at 2. Opening devices selected the newest one. With IDs `2,3,4` and 4 current, `dev.next()` returned 2 and `dev.prev()` returned 3. For unknown ID 99, `dev.next(99)` returned 2, `dev.prev(99)` returned 4, and `dev.set(99)` selected 2. `dev.off(99)` left the current device active. Closing current device 4 selected device 2. `dev.set(1)` returned the null-device result and opened a new default device. `dev.new()` also opened the configured default.
- `dev.size()` defaults to inches. A 5 × 4 inch PDF reported `c(360,288)` in `"px"` and `c(12.7,10.16)` in `"cm"`.

| Format | Exact file probe |
|---|---|
| PDF | A 4 × 3 inch, two-page file began `%PDF-1.4`. Its page tree contained `/Count 2` and `/MediaBox [0 0 288 216]`. A numbered filename with `onefile=FALSE` produced `m01.pdf` and `m02.pdf`. [Adobe PDF 1.4 reference](https://opensource.adobe.com/dc-acrobat-sdk-docs/pdfstandards/pdfreference1.4.pdf) |
| SVG | A 4 × 3 inch Cairo device emitted `width="288pt"`, `height="216pt"`, and `viewBox="0 0 288 216"`. `onefile=FALSE` produced `page01.svg` and `page02.svg`. `onefile=TRUE` produced `multi01.svg` with `<pageSet>` and `<page>` elements. A stroke with `#FF000080` emitted `stroke-opacity="0.501961"`. [SVG 1.1](https://www.w3.org/TR/SVG11/), [SVG Print 1.2 draft](https://www.w3.org/TR/SVGPrint/) |
| PNG | The Quartz probe produced two files with the PNG signature and `IHDR` dimensions `300 × 240`. Cairo with `width=4, height=3, units="in", res=100` reported `400 × 300` pixels and also produced two files. Device names were `quartz_off_screen` and `png`, respectively. [PNG specification](https://www.w3.org/TR/png/) |

The SVG shape, page, and opacity probes passed. The temporary Cairo environment lacked its default Fontconfig configuration, so the SVG glyph output does not establish font parity.

### Base graphics state and drawing

On a fresh 7 × 7 inch PDF, `usr=c(0,1,0,1)`, `fig=c(0,1,0,1)`, `fin=c(7,7)`, and `pin=c(5.76,5.16)`. Defaults were `mar=c(5.1,4.1,4.1,2.1)`, `mai=c(1.02,.82,.82,.42)`, and `oma=rep(0,4)`, in bottom, left, top, right order. `plt` was approximately `c(.117143,.94,.145714,.882857)`. Other defaults were `mfrow=mfcol=c(1L,1L)`, `col=fg="black"`, `bg="transparent"`, `pch=1L`, `lty="solid"`, `lwd=cex=1`, `las=0L`, `bty="o"`, `xaxs=yaxs="r"`, and `new=FALSE`. `par("log")` warned and returned `NULL`. Query `xlog` and `ylog` instead.

| Probe | Result |
|---|---|
| `plot.window(c(0,10), c(2,4))` | `usr=c(-.4,10.4,1.92,4.08)`. Style `"r"` extends each end by 4% of the range. Style `"i"` preserves the endpoints, including reversed limits. |
| `plot.window(c(1,100), c(.01,100), log="xy")` | `usr=c(-.08,2.08,-2.16,2.16)` in base-10 log units. Both log flags were `TRUE`. The x endpoints converted to `1/27` and `26/27` of plot width. |
| Log and constant limits | A negative log limit errored. Zero on a log axis warned and substituted finite limits. A fresh linear plot with equal limits `x=0`, `y=2` gave `usr=c(-1.08,1.08,1.136,2.864)`. `plot.new()` reset the log flags. |
| `mfrow` / `mfcol` | Advance row-first or column-first, respectively. `cex` became `1`, `.83`, and `.66` for `1×2`, `2×2`, and `3×2`. `mex` reset to `1`. |
| `layout(matrix(c(1,2,1,3),2), widths=c(1,2))` | Returned `3L`. The successive `fig` regions were `c(0,1,.5,1)`, `c(0,1/3,0,.5)`, and `c(1/3,1,0,.5)`. |
| `plot.default(1:3,1:3)` | Invisible `NULL`, with `usr=c(.92,3.08,.92,3.08)`. `par(new=TRUE)` before any plot warned and left `new=FALSE`. A subsequent high-level plot consumed a valid `new=TRUE`. |

`plot.new()` starts a frame. `plot.window()` sets the coordinate transform. The tested low-level commands required an initialized frame. `lines`, `points`, `abline`, `segments`, `rect`, `polygon`, `text`, `mtext`, `title`, and `box` then returned invisible `NULL`. PDF path probes confirmed that `NA` splits lines and polygons, an `NA` point is skipped, and scalar coordinates and labels recycle in the tested `segments`, `rect`, and `text` calls. `axis(1, at=c(3,1,2,10))` returned `c(1,2,3,10)` invisibly. `legend(..., plot=FALSE)` returned invisible `list(rect=list(w,h,left,top), text=list(x,y))`, with the geometry in user coordinates.

### Colors, ranges, and statistical plots

| Probe | Result |
|---|---|
| `col2rgb(..., alpha=TRUE)` | Integer matrix with rows `red`, `green`, `blue`, `alpha`. `NA` and `"transparent"` gave `(255,255,255,0)`. `"#12345678"` gave `(18,52,86,120)`. Numeric zero errored. Positive indices wrap through the palette. |
| Color construction | `rgb(.5,0,0,alpha=.5)` gave `"#80000080"`. `hsv(c(0,1,1/3),1,1)` gave `c("#FF0000","#FF0000","#00FF00")`. `hcl(c(0,360),50,50)` gave two `"#AD6071"` values. An out-of-gamut HCL sample returned `NA` with `fixup=FALSE`. |
| Alpha and interpolation | `adjustcolor("#12345678",alpha.f=.5)` gave `"#1234563C"`. `colorRampPalette(c("black","white"))(3)` gave `c("#000000","#7F7F7F","#FFFFFF")`. |
| `palette()` | Default `c("black","#DF536B","#61D04F","#2297E6","#28E2E5","#CD0BBC","#F5C710","gray62")`. Setting a palette returned the prior palette. `"default"` restored the default. |
| Layout and ranges | `n2mfrow(7, asp=.5/1/2)` in three separate calls gave `(4,2)`, `(3,3)`, `(2,4)`. `extendrange(c(1,NA,5), f=c(.1,.2))` gave `c(.6,5.8)`. `pretty(c(.13,.91), n=5)` gave `seq(0,1,.2)`. `pretty(rep(2,3))` gave `c(0,2)`. Empty input gave `numeric(0)`. |

**`hist()`** returns class `"histogram"` with fields in order `breaks`, `counts`, `density`, `mids`, `xname`, `equidist`. For `x=c(0,1,1,2,3,4,NA,Inf)` and `breaks=c(0,1,2,4)`, the oracle ignored nonfinite input, returned `mids=c(.5,1.5,3)` and `equidist=FALSE`, and used these counts:

| Intervals | Integer counts | Density |
|---|---|---|
| `right=TRUE`, `include.lowest=TRUE` | `3,1,2` | `1/2,1/6,1/6` |
| `right=FALSE`, `include.lowest=TRUE` | `1,2,3` | `1/6,1/3,1/4` |

Sort explicit breaks. Use `counts / (sum(counts) * diff(breaks))` for density. Error if finite input falls outside the counted intervals. The fuzz tolerance affects bin assignments without changing the returned breaks. With `x=c(0,1,1+1e-8,2)` and `breaks=0:2`, `fuzz=0` gave counts `2,2`; `fuzz=1e-7` gave `3,1`.

For `x=c(1:8,50)`, Sturges, Scott, and Freedman–Diaconis returned target class counts `5`, `2`, `13`. The resulting breaks were `seq(0,50,10)`, `seq(0,60,20)`, and `seq(0,50,5)`. Use the target count with `pretty`. The usual widths are `3.5 * sd(x) / n^(1/3)` for Scott and `2 * IQR / n^(1/3)` for Freedman–Diaconis, which uses five significant digits by default. Preserve separate zero-spread cases. `nclass.FD(rep(1,10))` returned `1`, and `nclass.FD(c(rep(0,99),100))` returned `21`.

**`boxplot()`** returns a plain list in order `stats`, `n`, `conf`, `out`, `group`, `names`. `stats` is a 5 × group-count matrix. For sorted nonmissing observations, use positions `1, k, (n+1)/2, n+1-k, n`, where `k=floor((n+3)/2)/2`. Average the adjacent observations at a half-integer position. For example, `1:5` gives `(1,2,3,4,5)`, and `1:8` gives `(1,2.5,4.5,6.5,8)`.

For `list(A=c(1:8,50), B=c(5,5,NA), C=numeric())`, the columns were `(1,3,5,7,8)`, `(5,5,5,5,5)`, and five `NA`s. The result had `n=c(9,2,0)`, `out=50`, and `group=1`. Whiskers end at observed values within `range` times the hinge spread beyond the hinges. `range=0` uses the extremes and clears outliers. The two-row `conf` matrix uses `median ± 1.58 * hinge_spread / sqrt(n)`. `boxplot.stats(c(-Inf,1:4,Inf,NA))` counted six observations and treated both infinities as outliers.

**`warnN` oracle result:** `boxplot.default`, `boxplot.formula`, `boxplot.matrix`, `bxp`, and `boxplot.stats` exposed no `warnN` formal. `warnN=FALSE` and `warnN=TRUE` gave identical empty-group and singleton results. Both retained the notch-outside-hinges warning for `boxplot(1:3, notch=TRUE)`. The observed behavior supports accepting the argument through `...` where available.

**`barplot()`** returns midpoints. `barplot(1:3, plot=FALSE)` returned a 3 × 1 matrix with `.7,1.9,3.1`. A 2 × 3 matrix returned the same midpoints as a vector when stacked. `beside=TRUE` returned a 2 × 3 matrix with columns `(1.5,2.5)`, `(4.5,5.5)`, `(7.5,8.5)`. `hist`, `boxplot`, and `barplot` all returned visibly with `plot=FALSE` and invisibly with `plot=TRUE`.

### `grid`

- `unit(1,"npc")` is a numeric vector with `unit=0L` and class `c("simpleUnit","unit","unit_v2")`. `npc` measures a fraction of the current viewport. Mixed units use `c("unit","unit_v2")` and hold per-element value, data, and unit code. `unit(1:3,c("npc","inches"))` recycled the names. `"in"` aliases `"inches"`, `"pt"` aliases `"points"`, and unknown unit names error. Like units can be scaled by a number. Multiplying two unit objects errored. Addition across unit types retained a composite unit.
- On an 8 × 4 inch PDF, `grid.newpage()` set the current viewport to `ROOT`, with native x scale `c(0,576)`, and cleared the display list. A centered viewport of width and height `.5`, with `xscale=c(0,10)`, converted native widths `c(1,5,10)` to `c(.4,2,4)` inches. Native x coordinate 5 converted to `.5 npc`.
- `pushViewport()` inserts and enters a viewport. `upViewport()` ascends and preserves descendants. `seekViewport()` can return to a preserved descendant. `popViewport()` ascends and removes the popped subtree. After popping `B` from `A::B`, seeking `B` errored. `upViewport(0)` went to the root and preserved `A::B`. `popViewport(0)` went to the root and removed it.
- `gpar()` creates a named list with class `"gpar"`. `fontface="bold"` yielded `font=c(bold=2L)`. Invalid `alpha` and zero-length `col` errored. Nested viewport settings `cex=2, alpha=.5` produced effective `cex=4, alpha=.25`.
- `rectGrob`, `linesGrob`, `pointsGrob`, `textGrob`, `polygonGrob`, `circleGrob`, and `segmentsGrob` create lists with class `c(shape,"grob","gDesc")` and common fields `name`, `gp`, `vp`. Polygon grouping uses `id` or `id.lengths`. `gTree` adds `children` and `childrenOrder`. Axis constructors prepend `xaxis` or `yaxis`, then `axis`, to the `gTree` class chain and carry `at`, `label`, `main`, and `edits`. Constructors create objects. `grid.draw()` draws and records them, and returns invisible `NULL`. The display list for a rectangle `r` and a tree `tree` containing text `t` reported `c("r","tree","t")`.
- `grid.layout(2,3)` used three `1null` widths and two `1null` heights. A child viewport in row 1, column 2 occupied `8/3` by `2` inches inside the full 8 × 4 inch parent. Resolve `null` units from the layout’s available space.
