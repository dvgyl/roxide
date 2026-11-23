# Milestone 2 empirical specification

Validated with GNU R 4.6.2 (2026-10-29), `aarch64-apple-darwin23`. Each probe ran in a fresh process with `source env/env.sh && "$ROXIDE_ORACLE_RSCRIPT" --vanilla -e '…'`. The probes used in-memory connections for document input and output. Two exploratory calls printed closure bodies as a side effect. This specification uses only the observed behavior and public return values.

## W6B — Object inspection and DCF

### `utils::str`

Use `capture.output(str(x))` to compare exact lines. The strings below use R escapes. Leading spaces, trailing spaces, and `\t` are significant.

| `x` | Captured lines |
|---|---|
| `c(pi, NA, NaN, Inf, -0)` | `" num [1:5] 3.14 NA NaN Inf 0"` |
| `c(1L, NA_integer_)` | `" int [1:2] 1 NA"` |
| `c("hi", "é", "a\nb", NA)` | `" chr [1:4] \"hi\" \"é\" \"a\\nb\" NA"` |
| `c(TRUE, FALSE, NA)` | `" logi [1:3] TRUE FALSE NA"` |
| `c(1+2i, NA_complex_)` | `" cplx [1:2] 1+2i NA"` |
| `as.raw(c(0,255))` | `" raw [1:2] 00 ff"` |
| `1L` | `" int 1"` |
| `numeric()` | `" num(0) "` |
| `NULL` | `" NULL"` |
| `list()` | `" list()"` |
| `c(a=1,b=2)` | `" Named num [1:2] 1 2"` then `" - attr(*, \"names\")= chr [1:2] \"a\" \"b\""` |
| `matrix(1:6,2)` | `" int [1:2, 1:3] 1 2 3 4 5 6"` |
| `array(1:2,c(1,2,1))` | `" int [1, 1:2, 1] 1 2"` |
| `matrix(numeric(),0,3)` | `" num[0 , 1:3] "` |
| `factor(c("b",NA,"a","b"),levels=c("a","b","c"))` | `" Factor w/ 3 levels \"a\",\"b\",\"c\": 2 NA 1 2"` |
| `ordered(c("b","a"),levels=c("a","b","c"))` | `" Ord.factor w/ 3 levels \"a\"<\"b\"<\"c\": 2 1"` |
| `as.Date(c("2020-01-02",NA))` | `" Date[1:2], format: \"2020-01-02\" NA"` |
| `as.POSIXct(c("2020-01-02 03:04:05",NA),tz="UTC")` | `" POSIXct[1:2], format: \"2020-01-02 03:04:05\" NA"` |
| `function(x,y=1) x+y` | `"function (x, y = 1)  "` |
| `sum` | `"function (..., na.rm = FALSE)  "` |
| `baseenv()` | `"<environment: base> "` |
| `new.env()` | `"<environment: 0x…> "` with a process-specific hexadecimal address |

Empty integer, character, logical, complex, and raw vectors have the same form as `numeric()`, with their respective type names. A one-element date uses `[1:1]`. An empty date prints `" 'Date' num(0) "`. A factor with no levels prints `" Factor w/ 0 levels: "`.

`str(data.frame(a=1:2,b=c("x","y")))` gives:

```r
c("'data.frame':\t2 obs. of  2 variables:",
  " $ a: int  1 2",
  " $ b: chr  \"x\" \"y\"")
```

List and data frame names align at the colon. `list(a=1L,b=list(z=3L),c=NULL)` gives:

```r
c("List of 3", " $ a: int 1", " $ b:List of 1",
  "  ..$ z: int 3", " $ c: NULL")
```

- `max.level=0` prints the list or data frame header and any applicable top-level attributes. `max.level=1` prints immediate children. A nested list at that limit prints its header.
- `list.len=1` on a longer list adds `"  [list output truncated]"` after the first child.
- Default `vec.len` is `4` and default `digits.d` is `3`. The element limit depends on type. At `vec.len=2`, `1:10` prints `1 2 3 4 5 ...`, `letters[1:10]` prints `"a" "b" ...`, and a long logical vector prints three values followed by `...`. A factor with 26 one-letter levels shows the first four quoted levels followed by `,..`. Its codes follow the integer limit in the tested cases.
- Matrix and array values appear in column order. `dimnames`, when present, print as a list attribute. Custom attributes use `" - attr(*, \"unit\")= chr \"kg\""` for `structure(1:3,unit="kg")`. `give.attr=FALSE` suppresses the names attribute line but retains `Named` in the vector header.

### `utils::head`, `tail`, `capture.output`, `sessionInfo`

- For vectors and lists, positive `n` selects from the start or end. Negative `n` removes `abs(n)` elements from the opposite end. Names persist. Zero gives an empty result. The default count is six.
- Matrices, arrays, and data frames can take one `n` element per dimension. Omitted dimensions and `NA` entries select the full dimension. At least one entry must be non-missing. A single `n` applies to the first dimension. Subsets preserve dimensions.
- For `m <- matrix(1:12,3)`, `head(m,c(2,2))` has columns `c(1L,2L)` and `c(4L,5L)`. `tail(m,c(2,2))` has columns `c(8L,9L)` and `c(11L,12L)`, row names `c("[2,]","[3,]")`, and column names `c("[,3]","[,4]")`. `keepnums=FALSE` suppresses generated names. Existing dimension names survive. In the same fixture `head(m,-1)` keeps rows 1–2, and `tail(m,-1)` keeps rows 2–3.
- Fractional counts have asymmetric rounding. At dimension size five, `n=2.9` gave two elements for `head` and three for `tail`. `n=-1.1` gave three and four. `Inf` selects all and `-Inf` selects none. A dimensionless object with two counts raises `invalid 'n' - must have length one when dim(x) is NULL, got 2`.
- For `f <- function(x,y=1){a<-x+y;a^2}`, both methods return a one-column matrix with class `"noquote"` and column name `""`. `head(f,2)` contains `"function (x, y = 1) "` and `"{"`, with row names `"1","2"`. `tail(f,2)` contains `"    a^2"` and `"}"`, with row names `"4","5"`. The primitive `sum` gives the single line `".Primitive(\"sum\")"`.
- `capture.output(...,file=NULL,append=FALSE,type=c("output","message"),split=FALSE)` evaluates expressions in order. It prints visible values and captures complete lines and any final partial line. `capture.output({cat("a\nb"); 7},print(3))` gives `c("a","b[1] 7","[1] 3")`. The empty call and `capture.output(invisible(7))` give `character(0)`. `type="message"` captures messages and immediate warnings. Deferred warnings appeared outside the capture. With a supplied connection, the function writes to it and returns invisible `NULL`. `split=TRUE` also writes ordinary output to the previous output destination.
- `sessionInfo(package=NULL)` returns class `"sessionInfo"`. On the fresh oracle its ordered fields were `R.version`, `platform`, `locale`, `tzone`, `tzcode_type`, `running`, `RNGkind`, `basePkgs`, `loadedOnly`, `matprod`, `BLAS`, `LAPACK`, `LA_version`. Package state can add `otherPkgs`. The default base package vector was `c("stats","graphics","grDevices","utils","datasets","methods","base")` and `loadedOnly` contained `compiler`. `RNGkind` was `c("Mersenne-Twister","Inversion","Rejection")`. `sessionInfo(package=c("utils","stats"))$basePkgs` was `c("utils","stats")`. The printed report includes host and library details, locale and time zone, and the package lists.

### `base::read.dcf` and `write.dcf`

`read.dcf(file,fields=NULL,all=FALSE,keep.white=NULL)` splits a regular line at its first colon. Tag names are case-sensitive. In the probe `A : v`, the tag was `"A "`.

- Empty lines and lines that contain only spaces or tabs separate records. A line that starts with `#` in column one is discarded in both modes. It leaves the current field and record open. Thus `c("A: x","# comment"," y")` gives `"x\ny"`. An indented `#` line is field content. An indented `B: y` line is also field content.
- A continuation line must follow a field in the same record. Without `keep.white`, surrounding spaces and tabs are removed from field lines. A continuation whose trimmed text is `.` adds a blank line. An inline `.` or a continuation that contains `..` stays literal. For `c("A: x"," . "," y "," .")`, both default readers give `"x\n\ny"`. With `keep.white="A"` they give `"x\n\n y \n"`. `keep.white` preserves leading whitespace on continuations and trailing whitespace on the first line. It still removes the whitespace immediately after the first colon.
- The default result is a character matrix with no row names. Field order follows first occurrence, unless `fields` supplies the order. A missing value is `NA_character_`. `A:` gives `""`. The final occurrence of a repeated field wins.
- `all=TRUE` gives a data frame. A field repeated in any record becomes a list column, with one character vector per record. For `c("A: x","A: y","","A: z")`, the default result has column `c("y","z")`; the data frame has `A=list(c("x","y"),"z")`. A missing cell in a list column is `NA_character_`. Selecting an absent column with `all=TRUE` raises `undefined columns selected`; the matrix mode supplies an all-`NA` column. Blank or comment-only input gives a `0 × 0` matrix by default and raised `missing value where TRUE/FALSE needed` with `all=TRUE`.
- The default malformed-line error is `Line starting 'bad line ...' is malformed!`. With `all=TRUE`, the error begins `Invalid DCF format.` followed by `Regular lines must have a tag.` An orphan continuation reports `Found continuation line starting ' y ...' at begin of record.` in matrix mode and `Continuation lines must not start a record.` in data frame mode.
- With a raw connection, UTF-8 `café` retains bytes `63 61 66 c3 a9` and reports encoding `"unknown"` in both modes. Latin-1 input retains byte `e9` and fails `validUTF8()`. A UTF-8 byte order mark before the first tag remains part of that tag in matrix mode. Data frame mode strips it.

`write.dcf(x,file="",append=FALSE,useBytes=FALSE,indent=0.1*getOption("width"),width=0.9*getOption("width"),keep.white=NULL)` accepts a data frame or a matrix with column names. It emits fields in column order, skips missing scalar values, and puts one blank line between emitted records. An empty string gives `Field:`. A list cell `c("x","y")` emits two occurrences. Entirely missing rows emit no record.

For `X <- data.frame(Name=" café ",Desc="alpha beta gamma delta epsilon\n\n  déjà  vu\n.\nend",Empty="")`, width 25 and indent 4 give:

```r
c("Name: café", "Desc: alpha beta gamma", "    delta epsilon",
  " .", " déjà vu . end", "Empty:")
```

With `keep.white=c("Name","Desc")`, the same call gives:

```r
c("Name:  café ", "Desc: alpha beta gamma delta epsilon",
  " .", "   déjà  vu", " .", " end", "Empty:")
```

On the UTF-8 host, both `useBytes` values wrote `A: café\n` as UTF-8 to a raw connection even when the input value was marked Latin-1. `width=14,indent=2` split `"ééé ééé ééé ééé"` after the second word with either setting.

## W8B — Rd and license analysis

### `tools::parse_Rd` and `tools::Rd2txt`

Use this input with a text connection:

```r
rd <- c(
  "\\name{sample}",
  "\\alias{sample}",
  "\\title{Sample \\emph{title}}",
  "% top comment",
  "\\description{",
  "  First   line % comment",
  "  next line. \\code{x  +  1}, \\emph{word}, \\verb{a   b}.",
  "  \\href{https://example.org}{link}; \\url{https://example.org/a}.",
  "  Inline \\eqn{x^2}{x squared}.",
  "  \\deqn{y^2}{y squared}",
  "}",
  "\\usage{sample(x, y = 1)}",
  "\\arguments{",
  "  \\item{x}{First \\code{arg}.}",
  "  \\item{y}{Second arg.}",
  "}",
  "\\value{A value.}",
  "\\section{Extra heading}{",
  "One paragraph.",
  "",
  "\\preformatted{",
  "  first   line",
  "    second",
  "}",
  "Last.",
  "}",
  "\\examples{",
  "x <- 1  # R comment",
  "sample(x)",
  "\\dontrun{sample(0)}",
  "}"
)
```

`parse_Rd` returns a list with class `"Rd"` and attributes `class`, `srcref`, `macros`. `macros=FALSE` omits the `macros` attribute. Each syntax node has `Rd_tag` and `srcref`.

- Text leaves are character scalars. `TEXT` preserves spaces and line endings. `COMMENT` contains the `%` and comment text, with the line ending in a separate node. `VERB` covers `\name`, `\alias`, `\verb`, `\url`, and `\preformatted` contents. `RCODE` covers `\usage`, `\code`, and ordinary `\examples` contents. R `#` comments in examples stay in `RCODE`.
- One-argument macros such as `\title`, `\description`, `\emph`, and `\code` hold their child nodes directly. `\item` inside `\arguments`, `\href`, `\eqn`, `\deqn`, and `\section` hold an untagged list per argument. `\eqn{x^2}` produced one argument list.
- `srcref` has six integers in this order: first line, first byte, last line, last byte, first column, last column. For `\name{sample}`, the macro reference is `c(1L,1L,1L,13L,1L,13L)`. The `VERB` child spans bytes 7–12. UTF-8 byte offsets and character columns can differ. In `é\emph{好}`, the macro starts at byte 3 and column 2.
- In fragment mode, `\unknown{foo}` warns and gives an `UNKNOWN` leaf, a `LIST` node, and a final newline. `\newcommand{\hello}{Hi #1}` followed by `\hello{world}` yields a `USERMACRO` leaf with `c("Hi #1","world")`, attribute `macro="\\hello"`, and expanded text `"Hi world\n"`. `\ifelse{text}{yes}{no}` renders `yes`.

`capture.output(tools::Rd2txt(parsed,options=list(underline_titles=FALSE)))` returns these exact lines for the fixture:

```r
c("Sample _title_", "", "Description:", "",
  "     First line next line. ‘x + 1’, _word_, a b.  link;",
  "     <https://example.org/a>.  Inline x squared.", "",
  "                                  y squared                             ",
  "     ", "Usage:", "", "     sample(x, y = 1)", "     ",
  "Arguments:", "", "       x: First ‘arg’.", "",
  "       y: Second arg.", "", "Value:", "", "     A value.", "",
  "Extra heading:", "", "     One paragraph.", "",
  "       first   line", "         second", "     ", "     Last.", "",
  "Examples:", "", "     x <- 1  # R comment", "     sample(x)",
  "     ## Not run: sample(0)", "     ")
```

Default title underlining uses underscore and backspace per character. The raw heading `Usage:` begins `"_\bU_\bs_\ba"`. Prose folds runs of whitespace, including the spaces in this inline `\code` and `\verb` fixture. Usage, examples, and preformatted text follow their own indentation and whitespace rules. Display equations use the text argument when present and center it on a separate line. `Rd2txt` with default `out=""` returns invisible `""`.

**R 4.6.2 regression fixture.** In a description, use `c("First paragraph.","","% comment","Second paragraph.")`. The rendered lines are `c("     First paragraph.","","     Second paragraph.")`. Removing the blank input line gives `"     First paragraph. Second paragraph."` A column-one `%` comment after the blank line preserves the paragraph boundary. The installed 4.6.2 release notes identify this fix.

### `tools::analyze_license`

The result is a plain list with these fields in order:

```text
is_empty, is_canonical, bad_components, is_standardizable, is_verified,
standardization, components, expansions, extensions, pointers,
is_FOSS, restricts_use, spdx
```

For the six accepted inputs below, `is_empty=FALSE`, `bad_components=character(0)`, `is_canonical=is_standardizable=is_verified=is_FOSS=TRUE`, and `restricts_use=FALSE`. `standardization` equals the input as shown. `components` is the input as a character scalar except for GPL, which has two components, `c("GPL-2","GPL-3")`.

| Input | `expansions` | `spdx` |
|---|---|---|
| `MIT + file LICENSE` | `list("MIT + file LICENSE")` | `"MIT"` |
| `Apache License (>= 2)` | `list("Apache License version 2.0")` | `"Apache-2.0"` |
| `GPL-2 \| GPL-3` | `list("GPL-2","GPL-3")` | `"GPL-2.0-only OR GPL-3.0-only"` |
| `BSD_3_clause + file LICENSE` | `list("BSD_3_clause + file LICENSE")` | `"BSD-3-Clause"` |
| `CC0` | `list("CC0")` | `"CC0-1.0"` |
| `Artistic-2.0` | `list("Artistic-2.0")` | `"Artistic-2.0"` |

The two `+ file LICENSE` rows have `pointers="LICENSE"` and a one-row data frame in `extensions`, with columns `components` and `extensible=TRUE`. The other rows have `NULL` in both fields.

For `MIT OR Apache-2.0`, `GPL-3.0-or-later`, and `Proprietary`, both `components` and `bad_components` equal the input. `is_empty`, `is_canonical`, `is_standardizable`, and `is_verified` are `FALSE`. `standardization=NA_character_`, `is_FOSS=restricts_use=NA`, `expansions=extensions=pointers=NULL`, and `spdx=""`. The empty input gives `is_empty=TRUE`, `components=NULL`, and `bad_components=character(0)`, with the remaining values as in the rejected cases.

`MIT|Apache License (== 2.0)` gives `standardization="MIT | Apache License (== 2.0)"` and `spdx="MIT OR Apache-2.0"`. `file LICENSE` alone is canonical and standardizable, has `is_verified=FALSE` and `pointers="LICENSE"`, and gives `spdx=NA_character_`.

## W9 — Multivariate statistics, smoothing, and optimization

Numeric results below are rounded where indicated. Use tolerances for floating-point fixtures. Compare classes, field order, dimensions, names, integer codes, and messages exactly.

### `dist`, `hclust`, and `cutree`

Use `x <- rbind(a=c(0,0),b=c(1,1),c=c(4,2),d=c(9,4))`. `dist(x)` has class `"dist"` and a double payload in lower-triangle column order: `ab, ac, ad, bc, bd, cd`. Attributes are `Size=4L`, `Labels=rownames(x)`, `Diag=FALSE`, `Upper=FALSE`, `method`, and `call`. Minkowski adds `p`. For an unnamed matrix, `Labels` is absent. `diag` and `upper` control display, with the same stored pair values.

| Method | Pair values |
|---|---|
| `euclidean` | `sqrt(c(2,20,97,10,73,29))` |
| `maximum` | `c(1,4,9,3,8,5)` |
| `manhattan` | `c(2,6,13,4,11,7)` |
| `canberra` | `c(2,2,2,0.9333333333,1.4,0.7179487179)` |
| `binary` | `c(1,1,1,0,0,0)` |
| `minkowski`, `p=3` | `c(1.259921050,4.160167646,9.256022375,3.036588972,8.138223044,5.104468722)` |

For rows `c(NA,2,0)` and `c(3,5,0)`, the six methods give `sqrt(13.5)`, `3`, `4.5`, `9/7`, `0`, and `3.434142728` in table order. An all-missing row gives `NA` against either. Canberra excludes zero-zero pairs and scales the sum by total columns divided by usable columns. Two all-zero rows give `NA` for Canberra and `0` for binary. Binary treats nonzero finite values as present and warns `treating non-finite values as NA` for infinities. Minkowski rejects nonpositive and nonfinite `p` with `distance(): invalid p`.

`hclust(dist(x),method)` returns class `"hclust"` and fields `merge,height,order,labels,method,call,dist.method`. For all eight methods below, merge rows are `(-1,-2),(-3,1),(-4,2)` and `order=c(4L,3L,1L,2L)`. Negative entries identify observations, and positive entries identify previous merge rows. The first height is `sqrt(2)`.

| Linkage | Second height | Third height |
|---|---:|---:|
| `complete` | 4.472135955 | 9.848857802 |
| `single` | 3.162277660 | 5.385164807 |
| `average` | 3.817206808 | 7.926008785 |
| `ward.D2` | 4.396968653 | 9.703951085 |
| `ward.D` | 4.618204556 | 10.380908648 |
| `mcquitty` | 3.817206808 | 7.290797790 |
| `median` | 3.463653417 | 6.248107741 |
| `centroid` | 3.463653417 | 6.920605765 |

Four observations with all distances equal to one produced the same complete-linkage merge order. For the complete tree of `x`, `cutree(hc,k=2)` gives `c(a=1L,b=1L,c=1L,d=2L)`. For `k=1:4`, its integer matrix columns are `(1,1,1,1)`, `(1,1,1,2)`, `(1,1,2,3)`, `(1,2,3,4)`. Columns have names `"1"` through `"4"`. A cut at height `sqrt(2)` includes the first merge. When supplied together, `k` takes precedence over `h`. A height cut on a tree with heights `c(2,1.55)` raises `the 'height' component of 'tree' is not sorted (increasingly)`. A count cut succeeds.

### `kmeans`, `prcomp`, and `cmdscale`

Use `a <- rbind(a=c(0,0),b=c(0,2),c=c(10,0),d=c(10,2)); colnames(a) <- c("u","v")`.

- `kmeans(a,a[c(1,3),])` has class `"kmeans"`. Ordered fields are `cluster,centers,totss,withinss,tot.withinss,betweenss,size,iter,ifault`. Cluster labels are `c(a=1L,b=1L,c=2L,d=2L)`. Center rows are `(0,1)` and `(10,1)`, with row names `"1","2"` and column names `"u","v"`. The sums are `totss=104`, `withinss=c(2,2)`, `tot.withinss=4`, `betweenss=100`; `size=c(2L,2L)`. Hartigan-Wong gives `iter=1L,ifault=0L`. Lloyd and Forgy give `iter=2L,ifault=NULL`. MacQueen gives `iter=1L,ifault=NULL`. Duplicate explicit centers raise `initial centers are not distinct`. An empty initial cluster errors with Hartigan-Wong; Lloyd warned and returned `NaN` center coordinates for the empty cluster.
- `prcomp(a)` has class `"prcomp"` and ordered fields `sdev,rotation,center,scale,x`. `sdev=c(5.77350269189626,1.15470053837925)`, `rotation=diag(c(1,-1))`, `center=c(u=5,v=1)`, `scale=FALSE`. The score columns are `c(-5,-5,5,5)` and `c(1,-1,1,-1)`. Component names are `"PC1","PC2"`. `summary()` changes the class to `"summary.prcomp"` and appends `importance`, whose rows are `"Standard deviation"`, `"Proportion of Variance"` (`0.96154,0.03846`), and `"Cumulative Proportion"` (`0.96154,1`). `predict(p,a[1:2,2:1])` matches `p$x[1:2,]` because prediction reorders named columns. `rank.=1` truncates the rotation and scores; this fixture retained both entries in `sdev`. `retx=FALSE` omits `x`. With `scale.=TRUE`, a zero or constant centered column raises `cannot rescale a constant/zero column to unit variance`.
- `cmdscale(dist(a))` returns a matrix. Its columns are approximately `(5,5,-5,-5)` and `(-1,1,-1,1)`. With `eig=TRUE`, `add=TRUE`, or `x.ret=TRUE`, the default return is a plain list with `points,eig,x,ac,GOF`. `eig=TRUE` gives eigenvalues near `c(100,4,0,0)`, `ac=0` when `add=FALSE`, and `GOF=c(1,1)`. `x.ret=TRUE` supplies the double-centered matrix of negative squared distances, with first row `c(-52,-48,48,52)` for this fixture. For three observations with distances `d12=1,d13=1,d23=4`, `add=TRUE,eig=TRUE,k=2` gives `ac=2`, eigenvalues near `(18,0,0)`, and one coordinate column near `(0,-3,3)`. The warning is `only 1 of the first 2 eigenvalues are > 0`.

### `density`, interpolation, empirical distribution, and smoothing

- `density(x)` defaults to the Gaussian kernel and `bw="nrd0"`. The class is `"density"`. Ordered fields are `x,y,bw,n,old.coords,call,data.name,has.na`. For `x <- c(-2,0,1,4)` and `density(x,n=4)`, `bw=1.14527091312819`, the grid is `c(-5.435812739,-1.145270913,3.145270913,7.435812739)`, and `y=c(0.0009699731842,0.133813566175,0.083002849193,0.0009688491679)`. The result has `n=4L,old.coords=FALSE,data.name="x",has.na=FALSE`. The result field `n` counts observations; the input `n` sets grid length. The default limits extend three bandwidths beyond the finite input range. `old.coords=TRUE` gives `y=c(0.0009785494646,0.133952842969,0.083105366837,0.0009774004632)` on the same grid. Missing input errors unless `na.rm=TRUE`. In `c(1,3,Inf)`, the infinite observation counts toward `n` and weight normalization but makes no finite-grid contribution. Thus its density values are `2/3` of those for `c(1,3)`. With an explicit bandwidth, one point is valid. `density(2,bw=1,n=5,from=0,to=4)$y[3]` is `0.398887286960692`.
- `approx` returns `list(x,y)`; `approxfun` returns a function that gives the same ordinates. They sort input and average tied ordinates by default, warning `collapsing to unique 'x' values`. `ties=min` takes the minimum. `ties="ordered"` assumes ordered input. For `x=c(3,1,2,2,NA)`, `y=c(30,10,20,24,99)`, and queries `c(0,1,1.5,2,2.5,3,4,NA)`, `rule=c(2,1)` gives linear results `c(10,10,16,22,26,30,NA,NA)`. `method="constant",f=.25` gives `c(10,10,13,22,24,30,NA,NA)`. In an open interval the constant result weights the left value by `1-f` and the right value by `f`. At a knot it gives the knot value. Rule 1 gives `NA` out of range; rule 2 gives the nearest endpoint. Explicit `yleft` and `yright` take precedence. With `na.rm=FALSE`, missing ordinates propagate through affected intervals.
- `spline` returns `list(x,y)`; `splinefun` returns a function with a `deriv` argument from 0 to 3. For `x=0:4`, `y=c(0,1,0,2,0)`, and `xout=c(-1,.5,2.5,4.5,5)`, observed results are below. Hyman uses `y=c(0,1,1.5,3,5)`.

| Method | Spline values, rounded |
|---|---|
| `fmm` | `c(-8.666666667,1.145833333,0.895833333,-4.9375,-13.666666667)` |
| `natural` | `c(-1.821428571,0.808035714,1.013392857,-1.660714286,-3.321428571)` |
| `periodic` | `c(2,0.40625,1.09375,0.40625,1)` |
| `hyman` | `c(-3.111111111,0.670138889,2.128472222,5.989583333,6.888888889)` |

Natural extrapolation is linear. Periodic extrapolation wraps. Unequal periodic endpoints warn `spline: first and last y values differ - using y[1L] for both`. Hyman requires monotonic ordinates, allows ties, and raises `'y' must be increasing or decreasing` on the nonmonotonic fixture. `splinefun` also accepts `monoH.FC`.

- `ecdf(c(3,1,2,2,NA))` returns a function with class `c("ecdf","stepfun","function")` and a `call` attribute. Its knots are `c(1,2,3)`. At `c(-Inf,0,1,1.5,2,2.99,3,Inf,NA)` it gives `c(0,0,.25,.25,.75,.75,1,1,NA)`. The default quantiles are `c(1,1.75,2,2.25,3)` with percentage names. All-missing input raises `'x' must have 1 or more non-missing values`.
- `lowess(c(4,1,3,2,5),c(8,2,4,4,10),f=.8)` returns `list(x=1:5,y=...)` with sorted `x` and `y` near `c(2.207994575,3.456781511,5.270649715,7.456781511,10.207994575)`. `iter=0` gives `c(2.239544545,3.427378965,5.145242070,7.427378965,10.239544545)`. The sorted `x` in the result is double in this fixture. Defaults are `f=2/3`, `iter=3L`, and `delta=.01*diff(range(x))`.
- `loess(y~x,data.frame(x=1:12,y=c(2,3,6,7,12,18,20,30,38,42,55,63)))` has class `"loess"` and ordered fields `n,fitted,residuals,enp,s,one.delta,two.delta,trace.hat,divisor,robust,pars,kd,call,terms,xnames,x,y,weights`. The tested default uses span `.75`, degree `2L`, surface `"interpolate"`, family `"gaussian"`, and one iteration. `kd` has fields `parameter,a,xi,vert,vval`. Prediction at `c(0,1,3.5,7,12,13,NA)` gives `c(NA,2.005038043,6.587378258,22.467948908,63.300222802,NA,NA)` with new data frame row names. `se=TRUE` at `c(1,3.5,7)` gives `fit`, `se.fit=c(1.649188527,1.018218102,1.130327119)` with the same row names, `residual.scale=1.895846718`, and `df=6.808299813`. Direct surface prediction at `c(0,13)` gives `c(1.713879757,73.847928956)`. A fit with `na.exclude` adds `na.action` and restores omitted positions as `NA` in prediction on training data.

### `nlminb` and `nlm`

Use `f <- function(x) sum((x-c(2,-1))^2)`, `g <- function(x) 2*(x-c(2,-1))`, and `H <- function(x) diag(2,2)`.

`nlminb` returns a plain list with `par,objective,convergence,iterations,evaluations,message`. `evaluations` is an integer vector named `"function","gradient"`. Names on `start` persist in `par`.

| Call after `start=c(0,0), objective=f` | `par` | Objective | Convergence | Iterations | Evaluations | Message |
|---|---|---:|---:|---:|---|---|
| Default | near `(2,-1)` | `3.019858e-30` | `0L` | `2L` | `(4,4)` | `X-convergence (3)` |
| `gradient=g,hessian=H` | `(2,-1)` | 0 | `0L` | `2L` | `(4,3)` | `X-convergence (3)` |
| `gradient=g,lower=c(0,0),upper=c(1,4)` | `(1,0)` | 2 | `0L` | `2L` | `(2,2)` | `both X-convergence and relative convergence (5)` |
| `control=list(iter.max=1)` | near `(2,-1)` | `3.019858e-30` | `1L` | `1L` | `(3,4)` | `iteration limit reached without convergence (10)` |
| `control=list(eval.max=1)` | `(0,0)` | 5 | `1L` | `1L` | `(1,2)` | `function evaluation limit reached without convergence (9)` |

`nlm` returns a plain list with `minimum,estimate,gradient,code,iterations`. `hessian=TRUE` inserts `hessian` before `code`. It drops names on the starting vector. Analytic derivatives are attributes on the objective result.

- `nlm(f,c(0,0))` gives `minimum=1.805121e-26`, `estimate` near `(2,-1)`, `gradient` near `(-1.201261e-13,-2.402523e-13)`, `code=1L`, `iterations=2L`. The requested Hessian is near `diag(2,2)`.
- `nlm(function(x) structure(f(x),gradient=g(x),hessian=H(x)),c(0,0),hessian=TRUE)` gives `minimum=6.162976e-32`, `estimate` near `(2,-1)`, `code=1L`, `iterations=1L`.
- `nlm(f,c(0,0),iterlim=1)` gives `minimum=4.503466e-13`, `estimate` near `(1.999999700,-1.000000600)`, `code=4L`, `iterations=1L`. With `gradtol=0,steptol=1` and the default iteration limit, the same first step stops with `code=2L`.
- `nlm(function(x) exp(-x),0,stepmax=.1,gradtol=0)` gives `estimate=.5`, `minimum=0.606530659712633`, `code=5L`, `iterations=5L`. A false analytic gradient `c(1,1)` for the quadratic raises `probable coding error in analytic gradient`.
