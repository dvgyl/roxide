# Milestone 2 Clean-Room Empirical Oracle Specifications

I verified these results with GNU R 4.6.2 (2026-10-29), `aarch64-apple-darwin23`, under `Rscript --vanilla`. The collation locale was `C.UTF-8`. I used the configured oracle through `env/env.sh`. I did not edit files, run Cargo, or inspect GNU R source. The fixtures below cover the requested cases. Rules inferred from multiple probes are marked as such.

## 1. Table input and type conversion

### `type.convert(x, as.is = TRUE)`

The conversion applies to the entire vector. The probes show this candidate order: logical, integer, double, complex, character. `tryLogical = FALSE` skips the logical candidate. For example, `c("1","1.0")` gives double `c(1,1)`, `c("1","1i")` gives complex `c(1+0i,0+1i)`, and `c("T","1")` stays character.

| Input | Type | Result |
|---|---|---|
| `"T"`, `"TRUE"` | logical | `TRUE` for each |
| `"F"`, `"FALSE"` | logical | `FALSE` for each |
| `"True"`, `"true"`, `"False"`, `"false"` | character | Original spelling |
| `c("TRUE","FALSE")`, `c("T","F")` | logical | `c(TRUE,FALSE)` for each |
| `c("T","false")`, `c("True","False")`, `c("true","false")` | character | Original vector |
| `"0"`, `"-0"`, `"+3"`, `" 3"`, `"007"` | integer | `0L`, `0L`, `3L`, `3L`, `7L` |
| `"3 "`, `"-0 "` | double | `3`, negative zero |
| `"2147483647"`, `"-2147483647"` | integer | `2147483647L`, `-2147483647L` |
| `"-2147483648"`, `"2147483648"` | double | `-2147483648`, `2147483648` |
| `"0x1A"`, `"1e2"`, `"1.0"` | double | `26`, `100`, `1` |
| `"Inf"`, `"-Inf"`, `"NaN"` | double | `Inf`, `-Inf`, `NaN` |
| `"1i"`, `"1+2i"`, `"0i"` | complex | `0+1i`, `1+2i`, `0+0i` |
| `"NaNi"`, `"1+NaNi"` | complex | Real parts `0`, `1`. Both imaginary parts are `NaN`. |
| `"1+NAi"`, `"NAi"`, `"i"`, `"+i"`, `"-i"`, `"1+2I"` | character | Original spelling |
| `"1 +2i"`, `" 1+2i"`, `"1+2i "` | complex | `1+2i` for each |
| `"1+ 2i"`, `"1 + 2i"`, `"1+-2i"` | character | Original spelling |

Append `c("NA","")` to any logical spelling or mixed set in the table to get these results:

| Candidate that succeeds | Default `na.strings = "NA"` | `na.strings = c("NA","")` |
|---|---|---|
| Logical | Same logical values, then `NA, NA` | Same result |
| Character | Same strings, then `NA_character_, ""` | Same strings, then `NA_character_, NA_character_` |

Whitespace-only fields also become missing in a successful numeric or logical conversion. A character result keeps them unless they match `na.strings` exactly. `"  T"`, `"TRUE "`, and `"  NA"` stay character. With `na.strings = character()`, `"NA"` stays character. `character(0)` gives `logical(0)` by default and `integer(0)` with `tryLogical = FALSE`.

With `dec = ","`, all three `numerals` modes give the same result for these inputs:

| Input | Result |
|---|---|
| `"1,25"`, `"1,0"`, `"1,2e2"` | Double `1.25`, `1`, `120` |
| `"1,2+3,4i"` | Complex `1.2+3.4i` |
| `"0x1,8p1"`, `"0x1p2"` | Double `3`, `4` |
| `"1.25"`, `"0x1.8p1"`, `"1.234,5"`, `"1,234.5"`, `"1,2,3"` | Original strings |

The next table uses `dec = "."`. `warn.loss` returns the `allow.loss` value. The warning text is `accuracy loss in conversion from "<token>" to numeric`.

| Input | `allow.loss` result, to 17 significant digits where needed | `warn.loss` | `no.loss` |
|---|---|---|---|
| `"9007199254740991"` | `9007199254740991` | Silent | Same double |
| `"9007199254740992"` | `9007199254740992` | Warns | Original string |
| `"9007199254740993"` | `9007199254740992` | Warns | Original string |
| `"9007199254740994"` | `9007199254740994` | Warns | Original string |
| `"9007199254740992e-10"` | Double | Warns | Original string |
| `"0.10000000000000001"` | `0.10000000000000001` | Warns | Original string |
| `"1.2345678901234567"` | `1.2345678901234567` | Warns | Original string |
| `"1.234567890123456789"` | `1.2345678901234571` | Warns | Original string |
| `"1e309"`, `"1e-400"` | `Inf`, `0` | Silent | Same doubles |
| `"9007199254740993i"` | `0+9007199254740992i` | Warns with the full token | Original string |
| `"1+9007199254740993i"` | `1+9007199254740992i` | Warns with `"+9007199254740993i"` | Original string |

The probes suggest a precision check on the accumulated significand, with a cutoff at `2^53`, before exponent scaling. For example, `no.loss` accepts `9007199254740991e300` as `Inf` and rejects `9007199254740992e-10`. The warning can survive fallback to character. `c("1","9007199254740993","x")` warns once. `c("9007199254740993","0i")` warns twice and becomes complex. Reversing that pair warns once.

### Reader defaults

| Reader | `header` | `sep` | `quote` | `dec` | `fill` | `comment.char` |
|---|---:|---|---|---|---|---|
| `read.table` | `FALSE` | `""` | Double and single quote | `"."` | `!blank.lines.skip` | `"#"` |
| `read.csv` | `TRUE` | `","` | Double quote | `"."` | `TRUE` | `""` |
| `read.csv2` | `TRUE` | `";"` | Double quote | `","` | `TRUE` | `""` |
| `read.delim` | `TRUE` | `"\t"` | Double quote | `"."` | `TRUE` | `""` |
| `read.delim2` | `TRUE` | `"\t"` | Double quote | `","` | `TRUE` | `""` |

Shared defaults are `strip.white = FALSE`, `blank.lines.skip = TRUE`, `check.names = TRUE`, `stringsAsFactors = FALSE`, `as.is = !stringsAsFactors`, `na.strings = "NA"`, `tryLogical = TRUE`, `colClasses = NA`, `nrows = -1`, `skip = 0`, `allowEscapes = FALSE`, and `flush = FALSE`.

| Case | Observed behavior |
|---|---|
| One fewer header field | For `"a b\n01 1 2\n02 3 4"`, omitted `header` and explicit `header=TRUE` both give integer columns `a=c(1L,3L)`, `b=c(2L,4L)` and row names `c("01","02")`. The inferred row-name field keeps character spelling unless a class is supplied. |
| Override row names | On that input, `row.names=NULL` gives automatic row names and retains a leading character column named `row.names`. `row.names=c("r1","r2")` also retains that column. A single column name or index selects and removes that column after conversion. |
| Invalid row names | Duplicate row names give `duplicate 'row.names' are not allowed`. Missing row names give `missing values in 'row.names' are not allowed`. A blank row-name field is `""` with the default `na.strings`. |
| Header width | With omitted `header`, an equal-width first record is data. With explicit `header=FALSE`, a short first data record errors under default `read.table` and fills under `fill=TRUE`. A header two fields short with `header=TRUE` gives `more columns than column names`. |
| Width lookahead | In the CSV probes, the width check used the first five records including the header, limited to the header plus `nrows` when a positive `nrows` left fewer records. An early overlong row can trigger row-name inference even if the first data row has header width. A later overlong row can wrap into another output row with default CSV settings. |
| Name repair | Header `a,a,a.1,1 x,if,,a-b,a b` becomes `a,a.2,a.1,X1.x,if.,X,a.b,a.b.1`. `check.names=FALSE` preserves all original names. The inferred `row.names` column can have the same name as a repaired header column. |
| Quote escaping | With a separator such as `","`, `"a""b"` reads as `a"b`. With `sep=""`, the same field splits into `"a"` and `"b"`. With `sep=""`, `"a\"b"` reads as `a"b` with either `allowEscapes` setting. With `sep=","`, `";"`, or `"\t"`, that example gives `incomplete final line found by readTableHeader on 'text'` during lookahead. In a later CSV row it warns `EOF within quoted string` and consumes subsequent lines into the field. |
| Multiline fields | A quoted field retains literal newlines, including blank lines inside the field. It counts as one record for `nrows`. |
| Whitespace and comments | In separated character fields, `strip.white=TRUE` strips unquoted surrounding whitespace and preserves quoted whitespace. It makes unquoted `  NA  ` match the default missing string. `read.table` skips `#` comments outside quotes. CSV defaults retain `#` as data. |
| Fill and blank lines | A missing field in a short CSV row enters inference as `""`. It stays `""` in a character column and becomes `NA` in a numeric column. With `blank.lines.skip=FALSE`, a blank CSV record gives a filled row. Default `read.table` errors on a short row, for example `line 2 did not have 3 elements`. |
| `nrows` and `skip` | Positive `nrows` limits data records. `nrows=0`, `-1`, and `-2` read all records in the probes. `skip` counts physical lines before parsing, including comments and blank lines. A header-only CSV gives logical zero-length columns unless classes are supplied. Empty input gives `no lines available in input` unless column names are supplied. |

`colClasses` uses input positions, including an inferred row-name field. An unnamed vector recycles. A named vector matches repaired names, leaves unspecified columns to inference, and warns `not all columns named in 'colClasses' exist` for unmatched names. `NA` requests inference, `"NULL"` drops a field, and `"character"` preserves numeric spelling and empty strings while applying `na.strings`. Explicit `"integer"` rejects `1.0` with `scan() expected 'an integer', got '1.0'`. Explicit `"logical"` also accepts `True`, `true`, `False`, and `false`. It rejects `t`, `f`, `0`, and `1`. In separated input the explicit scanner removed internal spaces in the probes, so `T R U E` became `TRUE` and `3 4` became `34L`. `"factor"` and `"Date"` worked. With `stringsAsFactors=TRUE`, inferred character columns become factors, including `""` as a level when it is present and `na.strings` leaves it intact.

## 2. Terms, model frames, and design matrices

### Formula algebra and `terms`

The table uses response `y` and data names `y,a,b,c,z`.

| Right side | `term.labels` in order |
|---|---|
| `a + b`, `a + a` | `a, b` and `a`, respectively |
| `a*b` | `a, b, a:b` |
| `a:b`, `a %in% b` | `a:b` |
| `a/b` | `a, a:b` |
| `(a+b)/c` | `a, b, a:b:c` |
| `a/(b+c)` | `a, a:b, a:c` |
| `a %in% (b+c)` | `a:b:c` |
| `(a+b+c)^2` | `a, b, c, a:b, a:c, b:c` |
| `(a+b+c)^3`, `(a*b*c)^2` | `a, b, c, a:b, a:c, b:c, a:b:c` |
| `a^2` | `a` |
| `(a+b)^0`, `(a+b)^1.5`, `(a+b)^-1` | Error `invalid power in formula` |
| `a*b-a` | `b, a:b` |
| `a-a`, `-a`, `1-a` | `character(0)`, intercept `1L` |
| `0+a+b`, `a+b-1` | `a, b`, intercept `0L` |
| `0+a+1`, `a+0-0` | `a`, intercept `1L` |
| `z+I(a+b)+offset(c)` | `z, I(a + b)`, `offset=4L` |
| `a+offset(b)+offset(c)` | `a`, `offset=c(3L,4L)` |
| `offset(b):a` | `character(0)`, `offset=2L` |
| `.`, `.-z` | `a, b, c, z` and `a, b, c`, respectively |

`.` expands in data-column order. `~ .` includes all columns. `cbind(y,z) ~ .` excludes both `y` and `z`. `terms(y ~ .)` without `data` gives `'.' in formula and no 'data' argument`.

`terms` has class `c("terms","formula")`. For `y ~ b:a + c + a + b`, the `variables` attribute is the call `list(y,b,a,c)`. Default term labels are `c,a,b,b:a`, with integer `order=c(1L,1L,1L,2L)`. With `keep.order=TRUE`, they are `b:a,c,a,b`, with `order=c(2L,1L,1L,1L)`. Variables follow first encounter. Default term sorting is stable by degree. The names within each interaction follow variable order.

`factors` is an integer matrix with variables as rows and terms as columns. It is `integer(0)` when there are zero terms. For interactions, the observed coding rule is:

- `0` means the variable is outside the term.
- `1` means a preceding term contains all the other variables in this term. Single-variable terms also use `1`.
- `2` means the variable is present and the preceding-term condition fails. For `~ a:b`, both codes are `2`. For `~ a/b`, the `a:b` column has codes `a=2, b=1`. A preceding term can contain additional variables beyond the required set.

The other base attributes are integer `intercept` and `response`, `class`, and `.Environment`. `response` is `1L` for a two-sided formula and `0L` for a one-sided formula. `.Environment` is the original formula environment. `offset` stores indices into `variables`. `specials`, when requested, is a named list of those indices, or `NULL` for an absent special. `dataClasses` and `predvars` appear on terms returned by `model.frame`. A removed term can still have a variable in `variables`. For example, `model.frame(y~x-w, data)` errors if `w` is unavailable.

For an original formula `y ~ a*b + c`, `update.formula` retained the original environment and returned:

| Update | Result |
|---|---|
| `. ~ . - b + z` | `y ~ a + c + z + a:b` |
| `. ~ . - 1` | `y ~ a + b + c + a:b - 1` |
| `log(.) ~ . + I(a^2)` | `log(y) ~ a + b + c + I(a^2) + a:b` |
| `z ~ .` | `z ~ a + b + c + a:b` |
| `. ~ .^2` | `y ~ a + b + c + a:b + a:c + b:c + a:b:c` |

### `model.frame` and missing values

The probes show that evaluation looks in `data` first and then in the formula environment. It evaluates formula variables before row selection, applies `subset`, then applies the missing-value action. Extra arguments become columns such as `(weights)` and `(offset)` in argument order. Formula offsets appear as columns named `offset(...)`. `model.offset` sums the explicit offset and formula offsets. `drop.unused.levels=FALSE` keeps unused factor levels. `TRUE` removes them.

For `y ~ poly(z,2) + log(x) + f + o + ch + I(z^2) + ell + offset(z)`, with `z=10:14` and extra `weights=w`, the `dataClasses` entries in column order are:

```text
numeric, nmatrix.2, numeric, factor, ordered, character,
numeric, logical, numeric, numeric
```

`predvars` is a `list(...)` call in formula variable order. The `poly` entry becomes `poly(z, 2, coefs = list(alpha = c(12,12), norm2 = c(1,5,10,14)))`, even when the frame keeps only two rows. The extra weights column is in `dataClasses`.

For row names `one,two,three,four,five`, make `y[2]`, `x[3]`, and `w[4]` missing and include all three fields in the frame:

| Action | Frame rows | `attr(mf,"na.action")` | `naresid` and `napredict` |
|---|---|---|---|
| `na.omit` | `one, five` | `structure(c(two=2L,three=3L,four=4L), class="omit")` | Return the supplied object |
| `na.exclude` | `one, five` | Same named integer vector, class `"exclude"` | Insert `NA` at the omitted positions |
| `na.fail` | Error | `missing values in object` | No frame returned |
| `na.pass`, `NULL` | All five | `NULL` | Return the supplied object |

On complete data all four actions yield `NULL` for the attribute. The default option is `na.action="na.omit"`. Positions are relative to the subset. For `subset=c(5,3,1)` with missing `x[3]`, the action is `structure(c(three=2L), class="exclude")` and the retained rows are `five,one`.

For the excluded rows `2:4`, padding `1:2` gives `c(1L,NA,NA,NA,2L)`. If the input is named `one,five`, the result has all five names in original order. Padding `matrix(1:4,2)` gives a `5 × 2` integer matrix with columns `c(1L,NA,NA,NA,2L)` and `c(3L,NA,NA,NA,4L)`. Row names are restored when the input has them. Column names and higher array dimensions are preserved. A matrix with column names only retains that naming scheme.

### `model.matrix` columns and attributes

Use this fixture:

```r
d <- expand.grid(f = factor(c("a","b","c")),
                 g = factor(c("U","V")), KEEP.OUT.ATTRS = FALSE)
d$x <- 1:6
d$o <- ordered(letters[1:5])[c(1:5,1)]
```

`Int` below means the literal column name `(Intercept)`. The `assign` attribute is integer.

| Formula | Columns in order | `assign` |
|---|---|---|
| `~ f` | `Int, fb, fc` | `0,1,1` |
| `~ 0+f` | `fa, fb, fc` | `1,1,1` |
| `~ f+g` | `Int, fb, fc, gV` | `0,1,1,2` |
| `~ 0+f+g` | `fa, fb, fc, gV` | `1,1,1,2` |
| `~ f:g` | `Int, fa:gU, fb:gU, fc:gU, fa:gV, fb:gV, fc:gV` | `0,1,1,1,1,1,1` |
| `~ 0+f:g` | `fa:gU, fb:gU, fc:gU, fa:gV, fb:gV, fc:gV` | `1,1,1,1,1,1` |
| `~ g+f:g-1` | `gU, gV, gU:fb, gV:fb, gU:fc, gV:fc` | `1,1,2,2,2,2` |
| `~ f*g` | `Int, fb, fc, gV, fb:gV, fc:gV` | `0,1,1,2,3,3` |
| `~ f+x+f:x` | `Int, fb, fc, x, fb:x, fc:x` | `0,1,1,2,3,3` |
| `~ o` | `Int, o.L, o.Q, o.C, o^4` | `0,1,1,1,1` |
| `~ 0+o` | `oa, ob, oc, od, oe` | `1,1,1,1,1` |
| `~ 1` | `Int` | `0` |
| `~ 0` | Zero columns | `integer(0)` |

The result is a double matrix with model-frame row names. The intercept is all ones. Code `1` selects contrast columns for a factor. Code `2` selects one indicator column per level. The default unordered contrast is `contr.treatment` with the first level as reference. The default ordered contrast is `contr.poly`. With no intercept, the earliest term containing a categorical variable uses full indicators for its first categorical variable. Numeric columns still use their original values. Interaction columns are products, with the first variable changing fastest.

`contrasts` is a named list in variable order, including `g,f` for `~ g+f:g-1`. Its values remain `"contr.treatment"` or `"contr.poly"` even where a term uses full indicators. A supplied named contrast matrix is stored as a matrix in the list. A supplied contrast function name stays a string. Numeric-only models have no `contrasts` attribute. Character variables become factors. Logical variables use levels `FALSE,TRUE`, so `~ l` names the contrast column `lTRUE`. A one-level factor errors even with `~ 0+f`. Offsets add no design columns.

## 3. Quantiles and linear models

### Exact `quantile` output

For `x=c(1,2,3,4,10)` and `probs=c(0,.25,.33,.5,1/3,.999999,1)`, the names for all types are:

```r
c("0%", "25%", "33%", "50%", "33.33333%", "99.9999%", "100%")
```

The values below use `sprintf("%.17g", value)`.

| Type | 0 | .25 | .33 | .5 | 1/3 | .999999 | 1 |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | 1 | 2 | 2 | 3 | 2 | 10 | 10 |
| 2 | 1 | 2 | 2 | 3 | 2 | 10 | 10 |
| 3 | 1 | 1 | 2 | 2 | 2 | 10 | 10 |
| 4 | 1 | 1.25 | 1.6500000000000001 | 2.5 | 1.6666666666666665 | 9.9999700000000011 | 10 |
| 5 | 1 | 1.75 | 2.1500000000000004 | 3 | 2.1666666666666665 | 10 | 10 |
| 6 | 1 | 1.5 | 1.98 | 3 | 2 | 10 | 10 |
| 7 | 1 | 2 | 2.3200000000000003 | 3 | 2.333333333333333 | 9.9999759999999966 | 10 |
| 8 | 1 | 1.6666666666666667 | 2.0933333333333337 | 3 | 2.1111111111111112 | 10 | 10 |
| 9 | 1 | 1.6875 | 2.1074999999999999 | 3 | 2.125 | 10 | 10 |

Defaults are `type=7L`, `digits=7`, and `names=TRUE`. The default `fuzz` is `0` for type 7 and `4*.Machine$double.eps` for the other types. At `digits=4`, the `1/3` label is `"33.33%"` and both final labels are `"100%"`. At `digits=9`, the `1/3` label is `"33.3333333%"`.

### `lm` and `summary.lm`

I chose a fixture with nonzero residuals and one exact alias. The latter is a coefficient the fit cannot determine independently.

```r
d <- data.frame(y = c(3,1,6,2,8,4,9,5), a = 0:7,
                f = factor(rep(c("lo","hi"),4), levels=c("lo","hi")),
                row.names = letters[1:8])
d$b <- 2*d$a
full <- lm(y ~ a + f, d)
aliased <- lm(y ~ a + b + f, d)
```

Both fits have class `"lm"` and fields in this order:

```text
coefficients, residuals, effects, rank, fitted.values, assign, qr,
df.residual, contrasts, xlevels, call, terms, model
```

- `coefficients`, `residuals`, `effects`, and `fitted.values` are named double vectors. `rank=3L` and `df.residual=5L` for both. The coefficient names follow the design columns. The residual and fitted names are `a` through `h`.
- Full-rank `assign` is `0:2` and `qr$pivot` is `1:3`. The aliased fit has `assign=0:3`, `qr$pivot=c(1L,2L,4L,3L)`, and the additional coefficient `b=NA_real_` between `a` and `fhi`.
- `qr` has class `"qr"` and fields `qr,qraux,pivot,tol,rank`. The matrix and `qraux` are double, `pivot` and `rank` are integer, and `tol=1e-7`. The matrix column names follow pivot order. In both fits, `effects` names start with `(Intercept),a,fhi` and continue with five empty strings.
- `contrasts=list(f="contr.treatment")` and `xlevels=list(f=c("lo","hi"))`. `call` is a call, `terms` has class `c("terms","formula")`, and `model` is a data frame. `x=TRUE,y=TRUE` append `x,y`; `model=FALSE,qr=FALSE` remove those entries. With missing rows, `na.action` follows `df.residual`. An offset field follows `na.action` if present, or `df.residual` otherwise.
- With weights the initial fields are `coefficients,residuals,fitted.values,effects,weights,rank`. `singular.ok=FALSE` gives `singular fit encountered` for the aliased fixture. A matrix response gives class `c("mlm","lm")`.

Both summaries have class `"summary.lm"` and fields `call,terms,residuals,coefficients,aliased,sigma,df,r.squared,adj.r.squared,fstatistic,cov.unscaled`. Both store the same double coefficient matrix:

| Row | `Estimate` | `Std. Error` | `t value` | `Pr(>|t|)` |
|---|---:|---:|---:|---:|
| `(Intercept)` | 3.9499999999999975 | 0.44665422868254601 | 8.8435298410829812 | 0.00030721756396277832 |
| `a` | 0.84999999999999998 | 0.10246950765959603 | 8.2951506200625271 | 0.00041574763730213305 |
| `fhi` | -4.3499999999999979 | 0.46957427527495599 | -9.2637101924991221 | 0.00024639396419376712 |

`aliased` is a named logical vector in original coefficient order. It is all `FALSE` for `full` and has `b=TRUE` for `aliased`. The summaries have `sigma=0.6480740698407863`, `r.squared=0.9621621621621621`, `adj.r.squared=0.9470270270270269`, and `fstatistic=c(value=63.57142857142852,numdf=2,dendf=5)`. Integer `df` is `c(3L,5L,3L)` or `c(3L,5L,4L)`. The entries give rank, residual degrees of freedom, and the original coefficient count. Both `cov.unscaled` matrices use the coefficient matrix row names on both axes. Rounded entries by row are:

```text
 0.475  -0.075  -0.175
-0.075   0.025  -0.025
-0.175  -0.025   0.525
```

A weighted summary adds `weights` after `terms` and stores residuals multiplied by `sqrt(weights)`. A nonempty fit with omitted rows appends `na.action`. With `na.exclude`, the raw fit and summary residuals contain the retained rows. The `residuals`, `fitted`, and `predict` methods restore excluded positions. An intercept-only fit gives both R-squared values as zero and omits `fstatistic`. `y~0` has `numeric(0)` coefficients, a `0 × 4` summary coefficient matrix, and a `0 × 0` `cov.unscaled`. Its summary also omits `fstatistic` and, in the probe with deleted rows, `na.action`.

With default print options, `print(summary(aliased))` prints `Coefficients: (1 not defined because of singularities)` and inserts the `b` row with four `NA` cells. The full-rank heading is `Coefficients:`. Both print these footer values:

```text
Residual standard error: 0.6481 on 5 degrees of freedom
Multiple R-squared:  0.9622,	Adjusted R-squared:  0.947
F-statistic: 63.57 on 2 and 5 DF,  p-value: 0.0002785
```

The printer shows individual residuals for 1–5 residual degrees of freedom and `Min`, `1Q`, `Median`, `3Q`, `Max` above five. At zero degrees of freedom the fixture prints `ALL 3 residuals are 0: no residual degrees of freedom!`. Weighted fits use `Weighted Residuals:`. When the summary has an omission action for one row, it prints `  (1 observation deleted due to missingness)` after the residual standard error.

### `anova.lm` and `dummy.coef`

`anova(full)` and `anova(aliased)` return identical tables. They have class `c("anova","data.frame")`, heading `c("Analysis of Variance Table\n","Response: y")`, and the following columns. `Df` is integer and the other columns are double.

| Row | `Df` | `Sum Sq` | `Mean Sq` | `F value` | `Pr(>F)` |
|---|---:|---:|---:|---:|---:|
| `a` | 1 | 17.357142857142861 | 17.357142857142861 | 41.326530612244873 | 0.0013528508572715648 |
| `f` | 1 | 36.042857142857116 | 36.042857142857116 | 85.816326530612102 | 0.00024639396419376712 |
| `Residuals` | 5 | 2.1000000000000019 | 0.42000000000000037 | `NA_real_` | `NA_real_` |

The term sums of squares are sequential. The printed table starts with `Analysis of Variance Table`, a blank line, and `Response: y`. The residual F and p cells print blank. The `a` row prints `41.327`, `0.0013529`, and `**`. The `f` row prints `85.816`, `0.0002464`, and `***`. Intercept-only and no-column fits have only the `Residuals` row. `anova(lm(y~a,d), full)` has columns `Res.Df,RSS,Df,Sum of Sq,F,Pr(>F)`. Its first row has `Res.Df=6`, `RSS≈38.1428571428571`, and four missing test cells. The second has `Res.Df=5`, `RSS≈2.1`, `Df=1`, and the same test values as the single-model `f` row.

`dummy.coef(full)` is a named list with class `"dummy_coef"` and `matrix=FALSE`. Its entries, rounded here, are `(Intercept)=c("(Intercept)"=3.95)`, `a=c(a=0.85)`, and `f=c(lo=0,hi=-4.35)`. The aliased fit inserts `b=c(b=0)` by default and `b=c(b=NA_real_)` with `use.na=TRUE`. In the fixture `lm(y~a*f,d)`, the interaction entry is `"a:f"=c(lo=0,hi=-0.3)`. Factor-by-factor names use level combinations such as `lo:A,hi:A,lo:B,hi:B` with the first factor changing fastest. The intercept-only fit returns a plain named list. The no-column fit returns `list()`.
