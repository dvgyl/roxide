# GNU R 4.6.2 oracle specification for W3B–W8

Verified on `R version 4.6.2 (2026-10-29)`, `aarch64-apple-darwin23`. All checks used the installed `Rscript --vanilla` oracle through `env/env.sh`. Decimal results below use display precision. The hexadecimal matrix product gives the exact double. L-BFGS-B convergence code `51` remains unverified.

## W3B, W5B, W7B — Linear algebra and FFT

### Matrix products

All three operations return a matrix, including scalar results. Logical and integer inputs produce doubles. Complex multiplication uses ordinary multiplication, including `crossprod(1i) = matrix(-1+0i)`. Empty inner dimensions give zeros. `NA_real_ %*% 0` gives a matrix with `NA_real_`.

For a matrix `M` with `r` rows and `c` columns and a vector `v` of length `n`, the probes give these promotion rules:

| Expression | Shape assigned to `v` |
|---|---|
| `v %*% M` | Row if `n == r`. Otherwise column if `r == 1`. |
| `M %*% v` | Column if `n == c`. Otherwise row if `c == 1`. |
| `crossprod(v, M)` | Column if `n == r`. |
| `crossprod(M, v)` | Column if `n == r`. Otherwise row if `r == 1`. |
| `tcrossprod(v, M)` | Column if `c == 1`. Otherwise row if `n == c`. |
| `tcrossprod(M, v)` | Column if `c == 1`. Otherwise row if `r == 1` and `n == c`. |

Other cases in the promotion table give `non-conformable arguments`. For example, with `A <- matrix(1:6, 2)`, `tcrossprod(1:3, A)` is a `1 × 2` matrix with values `22, 28`. `tcrossprod(A, 1:3)` gives the error. Two vectors use an inner product for `%*%` and `crossprod`, and an outer product for `tcrossprod`. An omitted second argument uses the first input again.

The two output dimname components come from the input rows and columns used for the output axes. `%*%` takes left row names and right column names. `crossprod` takes the two sets of column names. `tcrossprod` takes the two sets of row names. Vector names are discarded. If only one axis has names, the other component is `NULL`. If neither axis has names, the result has no `dimnames` attribute. Character input gives `requires numeric/complex matrix/vector arguments`.

```r
sprintf("%a", c(-1, 1 + 2^-27) %*% c(1, 1 + 2^-27))
# "0x1.0000001p-26"
```

The hex result includes the `2^-54` term. It matched under `matprod = "default"`, `"internal"`, and `"blas"` on this build.

### QR factorization

```r
A <- matrix(c(1,2,3,4, 2,1,0,3, 3,3,3,7), 4,
            dimnames = list(letters[1:4], LETTERS[1:3]))
b <- setNames(c(2,1,4,0), letters[1:4])
```

`qr(A)` returns class `"qr"` with ordered fields `qr`, `rank`, `qraux`, `pivot`. The packed matrix keeps row names and uses pivot order for its column names. `rank` and the one-based `pivot` are integers. For this fixture, default `tol = 1e-7` gives rank `2L`, pivot `1:3`, and `qraux = c(1.18257418583506, 1.22220503661085, 1.54744234743723)`. `tol = 0` gives rank `3L`, and `tol = 1` gives rank `1L`. `LAPACK = TRUE` adds `useLAPACK = TRUE` as an attribute, gives rank `3L`, and pivots to `c(3L,1L,2L)` with names `C,A,B`. Its `qraux` is `c(1.34412360080584, 1.26758041916665, 1.38361140828428)`. An exactly dependent fixture, `cbind(x=1:4,y=2*(1:4),z=c(1,3,1,0))`, also reports rank `3L` with LAPACK and rank `2L` by default.

| Function | Return behavior |
|---|---|
| `qr.Q` | Unnamed `r × min(r,c)` matrix. `complete=TRUE` gives `r × r`. `Dvec` scales columns. A scalar `Dvec=1` supplies the first column scale and gives `NA` in later columns in the fixture. |
| `qr.R` | Upper trapezoid with zeros below the diagonal. Shape `min(r,c) × c`, or `r × c` with `complete=TRUE`. Keeps applicable row names and pivoted column names. |
| `qr.X` | Reconstructs original column order. Default width is `min(r,c)`. `complete=TRUE` defaults to width `r`. Named `ncol` sets the width directly. With `ncol > c`, the tested result adds columns and drops dimnames. |
| `qr.coef` | Coefficients in original column order. Uses design column names and right-hand-side column names. A vector input returns a vector. The default QR puts `NA` in dependent positions. |
| `qr.resid`, `qr.fitted` | Default QR returns residuals and fitted values with the shape and names of `y`. LAPACK QR gives `not supported for LAPACK QR`. |
| `qr.qty`, `qr.qy` | Apply \(Q^\mathsf{T}\) and \(Q\). Default QR preserves vector shape and names. LAPACK QR promotes a named vector to `r × 1`, with its names on the rows. Both preserve the dimnames of a matrix right-hand side. |
| `qr.solve` | Checks rank against `min(r,c)`. A deficient factorization gives `singular matrix 'a' in solve`. For a full-row-rank wide matrix, unused coefficients can be zero, as in `qr.solve(matrix(1:6,2),1:2)`. Omitting `b` requests an inverse and requires a square system after the rank check. |

Default QR gives `qr.coef(q,b) = c(A=.878048780487805,B=-.646341463414634,C=NA)` and `qr.fitted(q,b) = c(a=-.414634146341463,b=1.10975609756098,c=2.63414634146342,d=1.57317073170732)`. Residuals equal `b - fitted`. LAPACK gives coefficients `c(A=-6456253790960039,B=-6456253790960041,C=6456253790960040)` on the same near-singular fixture.

A row mismatch gives `'qr' and 'y' must have the same number of rows` for `qr.coef` and default `qr.qty`. LAPACK `qr.qty` gives `right-hand side should have 4 not 3 rows` for this fixture. Passing a matrix directly to `qr.Q`, `qr.R`, or `qr.X` gives `argument is not a QR decomposition`.

### Cholesky, solve, and determinants

```r
P <- matrix(c(4,2,2,3), 2, dimnames=list(c("r","s"),c("x","y")))
S <- matrix(c(1,2,2,4), 2, dimnames=dimnames(P))
```

- `chol(P)` returns the upper factor with column-major values `c(2,0,1,sqrt(2))` and the input dimnames. `chol(P,pivot=TRUE)` adds integer `pivot=1:2` and `rank=2L`. Pivoting permutes column names and keeps row names in their input order. `chol(S,pivot=TRUE)` gives values `c(2,0,1,0)`, column names `y,x`, `pivot=2:1`, and `rank=1L`, with warning `the matrix is either rank-deficient or not positive definite`. `chol(S)` gives `the leading minor of order 2 is not positive`. A nonsquare matrix gives `'a' must be a square matrix`. The factorization reads the upper triangle.
- `chol2inv` reads the upper triangle of the leading `size × size` block and returns a symmetric double matrix without dimnames or pivot attributes. `chol2inv(chol(P))` has values `c(.375,-.25,-.25,.5)`. At size zero the error is `'size' argument must be a positive integer`. An oversized request gives `'size' cannot exceed ncol(x) = 2` or the corresponding `nrow(x)` message. A zero diagonal gives `element (2, 2) is zero, so the inverse cannot be computed` for `chol(S,pivot=TRUE)`.
- `solve(P)` has the same inverse values with row names `x,y` and column names `r,s`. `solve(P,c(a=2,b=4))` gives `c(x=-.25,y=1.5)`. With a matrix right-hand side, output row names come from `colnames(P)` and column names come from `colnames(b)`. With `S`, the exact error is `Lapack routine dgesv: system is exactly singular: U[2,2] = 0`. With `diag(c(1,1e-17))`, it is `system is computationally singular: reciprocal condition number = 1e-17`. Both `tol=0` and `tol=1e-18` allow that diagonal system. Shape errors include `'a' (3 x 2) must be square`, `'b' (1 x 1) must be compatible with 'a' (2 x 2)`, and `no right-hand side in 'b'`.
- `determinant` returns class `"det"` with `modulus`, `sign` in that order. `modulus` is a double with logical attribute `logarithm`. `sign` is an integer. For `P` the values are `log(8), 1L`, or `8, 1L` with `logarithm=FALSE`. For `S` they are `-Inf, 1L`, or `0, 1L` without the logarithm. For `matrix(c(1,2,2,1),2)` they are `log(3), -1L`. An empty square matrix gives `0, 1L` with the logarithm. `det(P)` returns the scalar `8`. Errors include `'x' must be a square matrix` and `'determinant' not currently defined for complex matrices`.
- `backsolve` uses the upper triangle by default. `forwardsolve` uses the lower triangle. `transpose=TRUE` solves the transposed system for the selected triangle. Both use the leading `k` rows of the right-hand side. They return an unnamed vector or a matrix with no dimnames. For `P` and `b=c(2,4)`, the results are `c(-1/6,4/3)` and `c(.5,1)`. An invalid size gives `invalid 'k' argument`. A zero diagonal at position 1 gives `singular matrix in 'backsolve'. First zero in diagonal [1]`, including through `forwardsolve`.

### Singular values, eigenvalues, and symmetry

`svd` returns `d,u,v`. `La.svd` returns `d,u,vt`, where `vt` is the conjugate transpose of `v`. These are ordinary lists. `d` is an unnamed double vector in descending order. Vector matrices omit input dimnames. If `nu` or `nv` is zero, omit that field. Positive requests are capped at the corresponding matrix dimension. Errors include `infinite or missing values in 'x'`, `a dimension is zero`, and `argument must be coercible to non-negative integer`.

Use the following sign fixtures. Each sign string describes a column of `u` or `v`, from top to bottom. Read rows of `vt` for the `v` signs in these real fixtures.

| Fixture | `d` | `u` signs | `v` signs |
|---|---|---|---|
| `B <- rbind(c(1,2,3),c(4,5,6),c(7,8,10))` | `17.4125051668086, .875161350110437, .196866521117430` | `---`, `++-`, `+-+` | `---`, `-++`, `+-+` |
| `rbind(B,c(2,0,1))` | `17.4883188934415, 1.68128829499466, .576170070734780` | `----`, `++--`, `+--+` | `---`, `-++`, `--+` |

`eigen` returns `values,vectors`, with class `"eigen"` when it computes vectors. `only.values=TRUE` returns a plain list with `vectors=NULL`. Symmetric eigenvalues sort by decreasing value. The general case sorts by decreasing modulus. For `diag(c(-8,3,1))`, the values are `c(3,1,-8)` with automatic symmetry and `c(-8,3,1)` with `symmetric=FALSE`. For `matrix(c(0,1,-1,0),2)`, automatic symmetry gives `c(1i,-1i)`. The first vector is `c(1,-1i)/sqrt(2)` and the second is its conjugate. Forcing `symmetric=TRUE` uses the lower triangle and gives real eigenvalues `c(1,-1)`. Errors include `non-square matrix in 'eigen'`, `0 x 0 matrix`, and `infinite or missing values in 'x'`.

`isSymmetric` requires a square matrix and tests conjugate symmetry for complex data. By default it checks dimnames, including names on the dimname list. `check.attributes=FALSE` bypasses that check. Equal `NA` entries at reflected positions pass in the tested matrix. At the default tolerance, `matrix(c(1,1e-14,0,1),2)` passes and `matrix(c(1,1e-12,0,1),2)` fails.

### Fast Fourier transforms

- `fft` returns complex data and preserves attributes, including names, dimensions, and dimnames. It transforms every dimension of an array. The inverse uses the opposite sign in the phase and leaves the result unscaled. `fft(1:4)` gives `c(10,-2+2i,-2,-2-2i)` in complex storage. `fft(array(1:8,c(2,2,2)))` has column-major values `c(36,-4,-8,0,-16,0,0,0)`, and a subsequent inverse gives `8 * array(1:8,c(2,2,2))` in complex storage. `fft(numeric())` gives `complex(0)`. Character input gives `non-numeric argument`.
- For `M <- matrix(1:6,2)`, `fft(M)` has values `c(21,-3,-6+3.46410161513775i,0,-6-3.46410161513775i,0)`. `mvfft(M)` transforms each column separately and gives `c(3,-1,7,-1,11,-1)`. It preserves matrix dimnames. Vectors and three-dimensional arrays give `vector-valued (multivariate) series required`.
- `nextn` returns the smallest product of powers of the supplied factors at least as large as the truncated input. `nextn(c(-2,0,1,2,7,11,17,NA,NaN))` gives `c(1L,1L,1L,2L,8L,12L,18L,NA_integer_,NA_integer_)`. Names are dropped. `nextn(c(2147483647,2147483648,1e12))` returns doubles `c(2147483648,2147483648,1e12)`. An empty factor set gives `no factors`. A factor of `1` or `NA` gives `invalid factors`.
- For `x=c(1,2,4)` and `y=c(3,5)`, `convolve(x,y,type="open")` gives `c(5,13,26,12)` and `"filter"` gives `c(13,26)`. With `conj=FALSE`, the same calls give `c(20,3,11,22)` and `c(3,11)`. For circular convolution, padding to length four gives `c(13,26,12,5)` with `conj=TRUE` and `c(3,11,22,20)` with `FALSE`. The circular default requires equal lengths and gives `length mismatch in convolution` otherwise. Complex correlation conjugates the second spectrum. Real inputs return real values. Open convolution of named `x` prepends an empty name in this fixture, and filter mode keeps the names of the retained positions.

## W6A — Generalized linear models and families

### Family objects

All eight constructors return class `"family"`. The ordered field prefix is:

```text
family link linkfun linkinv variance dev.resids aic mu.eta
initialize validmu valideta
```

`binomial`, `poisson`, `Gamma`, and `inverse.gaussian` append `simulate`. `quasi` appends `varfun`. All append `dispersion` last. The binomial initializers have type `"language"`. The other initializers have type `"expression"`.

| Family | Default link | Variance at mean `m` | `dispersion` | `validmu` |
|---|---|---|---|---|
| `gaussian` | `identity` | `1` | `NA_real_` | Always `TRUE` |
| `binomial` | `logit` | `m*(1-m)` | `1` | All finite and strictly between zero and one |
| `poisson` | `log` | `m` | `1` | All finite and positive |
| `Gamma` | `inverse` | `m^2` | `NA_real_` | All finite and positive |
| `inverse.gaussian` | `1/mu^2` | `m^3` | `NA_real_` | Always `TRUE` in the probes |
| `quasi` | `identity` | `1` by default | `NA_real_` | Depends on `variance` |
| `quasibinomial` | `logit` | `m*(1-m)` | `NA_real_` | Same as binomial |
| `quasipoisson` | `log` | `m` | `NA_real_` | Same as Poisson |

The `family` field contains the constructor name with the case shown. `quasi` accepts `"constant"`, `"mu"`, `"mu^2"`, `"mu^3"`, and `"mu(1-mu)"` as variance names. It returns the selected name in `varfun`. The three positive-mean variants accept `Inf` in `validmu` and return `NA` for `NA` alone. The quasi families return `NA_real_` from `aic`.

With response `y`, mean `m`, and prior weight `w`, the following unit deviance formulas matched the probes:

| Family or variance | `dev.resids` |
|---|---|
| Gaussian or constant | `w*(y-m)^2` |
| Binomial | `2*w*(y*log(y/m) + (1-y)*log((1-y)/(1-m)))`, with zero-multiplier log terms counted as zero |
| Poisson | `2*w*(y*log(y/m) - y + m)`, with the zero-multiplier log term counted as zero |
| Gamma | `2*w*((y-m)/m - log(y/m))` for positive `y` |
| Inverse Gaussian | `w*(y-m)^2/(y*m^2)` |

The quasi variants use the corresponding deviance. A direct `Gamma()$dev.resids` call at `y=0`, `m=.2`, `w=1` returns `-2`. The `quasi(variance="mu^2")` counterpart returns `0`. The Gamma initializer rejects zero responses.

The Gaussian, Gamma, inverse Gaussian, and constant-quasi initializers start at `y` on valid inputs. Poisson and quasi-Poisson start at `y+.1`. Binomial and quasi-binomial start vector responses at `(w*y+.5)/(w+1)`. For a success/failure matrix they use the row total as `n`, convert `y` to the success proportion, multiply weights by `n`, and start at `(n*y+.5)/(n+1)`. A zero-total row has response and weight zero and start `.5`. A factor response treats the first level as failure. Fractional successes in the binomial vector case warn `non-integer #successes in a binomial glm!`.

Observed response errors:

```text
y values must be 0 <= y <= 1
negative values not allowed for the 'Poisson' family
negative values not allowed for the 'quasiPoisson' family
non-positive values not allowed for the 'Gamma' family
positive values only are allowed for the 'inverse.gaussian' family
```

Poisson simulation warns `ignoring prior weights` for nonunit weights. Inverse Gaussian simulation on this installation gives `need CRAN package 'SuppDists' for simulation from the 'inverse.gaussian' family`.

### Link objects

`make.link` returns class `"link-glm"` with ordered fields `linkfun`, `linkinv`, `mu.eta`, `valideta`, `name`. Here `e` is the linear predictor, and `eps` is `.Machine$double.eps`.

| Link | `linkfun(m)` | `linkinv(e)` and derivative `mu.eta(e)` |
|---|---|---|
| `logit` | `log(m/(1-m))` | Logistic inverse and derivative. At `e=±Inf`, inverse is approximately `eps` and `1-eps`, and derivative is exactly `eps`. |
| `probit` | `qnorm(m)` | Normal distribution function with the predictor clipped at approximately `±8.13`. The derivative has floor `eps`. |
| `cloglog` | `log(-log1p(-m))` | `-expm1(-exp(e))`, bounded by `eps` and `1-eps`. Derivative is `exp(e)*exp(-exp(e))` in the central range and `eps` at the probed extremes. |
| `identity` | `m` | `e`. Derivative is an unnamed vector of ones, including at `NA`. |
| `log` | `log(m)` | Both inverse and derivative are `pmax(exp(e),eps)`. |
| `sqrt` | `sqrt(m)` | `e^2`, derivative `2*e`. |
| `1/mu^2` | `1/m^2` | `1/sqrt(e)`, derivative `-1/(2*e^1.5)` for positive `e`. |
| `inverse` | `1/m` | `1/e`, derivative `-1/e^2`. |

`valideta` is always `TRUE` for the first five links in the table. For `sqrt` and `1/mu^2`, it requires all predictors to be finite and positive. For `inverse`, it requires all to be finite and nonzero. `logit` rejects empty numeric inputs with `Argument eta must be a nonempty numeric vector` or `Argument mu must be a nonempty numeric vector`. `make.link("bad")` gives `‘bad’ link not recognised`. The constructors accepted recognized links outside their usual defaults, including `Gamma("logit")` and `binomial("inverse")`.

### Model objects and printed summaries

`glm.fit` returns an ordinary list with the first 20 fields below. `glm` returns class `c("glm","lm")` with this default field order:

```text
coefficients residuals fitted.values effects R rank qr family
linear.predictors deviance aic null.deviance iter weights prior.weights
df.residual df.null y converged boundary model call formula terms data
offset control method contrasts xlevels
```

`model=FALSE` and `y=FALSE` remove those fields. `x=TRUE` adds `x` before `call`, after `model` if present. Dependent design columns receive `NA` coefficients. Defaults from `glm.control()` are `epsilon=1e-8`, `maxit=25`, `trace=FALSE`.

`summary.glm` returns class `"summary.glm"` and these ordered fields:

```text
call terms family deviance aic contrasts df.residual null.deviance
df.null iter deviance.resid coefficients aliased dispersion df
cov.unscaled cov.scaled
```

`correlation=TRUE` appends `correlation,symbolic.cor`. `df` is `c(rank,df.residual,number_of_design_columns)`. Aliased coefficients are absent from the coefficient matrix. `cov.scaled` is `dispersion * cov.unscaled`. Default dispersion is `1` for binomial and Poisson. In the estimated-dispersion fixtures it is `sum(fit$weights * fit$residuals^2) / fit$df.residual`. An exact Gaussian fit with zero residual degrees of freedom gave `NaN`. Estimated dispersion uses the `t value` and `Pr(>|t|)` columns. Fixed dispersion uses `z value` and `Pr(>|z|)`.

```r
d <- data.frame(x=-2:3, y=c(0,1,0,1,0,1), n=c(1,0,2,3,2,5))
```

All rows below have residual and null degrees of freedom `4L,5L`, `converged=TRUE`, and `boundary=FALSE`. AIC is the Akaike information criterion.

| Fit | Intercept, slope | Deviance | AIC | Null deviance | Dispersion | `iter` |
|---|---|---:|---:|---:|---:|---:|
| `y~x`, binomial | `-.180660381193315, .361320762386630` | `7.79002682484381` | `11.7900268248438` | `8.31776616671934` | `1` | `4L` |
| `n~x`, Poisson | `.369106005772807, .387957314465342` | `3.11618157212258` | `20.8160424175533` | `8.12829320675870` | `1` | `5L` |
| `n~x`, Gaussian | `1.78095238095238, .771428571428571` | `4.41904761904762` | `21.1922507955001` | `14.8333333333333` | `1.10476190476190` | `2L` |
| `n+.5~x`, Gamma with log link | `.704213226625009, .295923957715139` | `1.22128155989203` | `22.6842001340561` | `2.76513013208007` | `.203548633941494` | `5L` |
| `n~x`, quasi-Poisson | Same as Poisson | `3.11618157212258` | `NA_real_` | `8.12829320675870` | `.539741501452266` | `5L` |

`control=list(maxit=1)` in the logistic fixture gives `iter=1L`, `converged=FALSE`, `boundary=FALSE`, and warning `glm.fit: algorithm did not converge`.

These are the summaries at `digits=4`, with trailing spaces omitted:

```text
Call:
glm(formula = y ~ x, family = binomial(), data = d)

Coefficients:
            Estimate Std. Error z value Pr(>|z|)
(Intercept)  -0.1807     0.8923  -0.202    0.840
x             0.3613     0.5174   0.698    0.485

(Dispersion parameter for binomial family taken to be 1)

    Null deviance: 8.3178  on 5  degrees of freedom
Residual deviance: 7.7900  on 4  degrees of freedom
AIC: 11.79

Number of Fisher Scoring iterations: 4
```

```text
Call:
glm(formula = n ~ x, family = poisson(), data = d)

Coefficients:
            Estimate Std. Error z value Pr(>|z|)
(Intercept)   0.3691     0.3972   0.929   0.3528
x             0.3880     0.1849   2.099   0.0358 *
---
Signif. codes:  0 ‘***’ 0.001 ‘**’ 0.01 ‘*’ 0.05 ‘.’ 0.1 ‘ ’ 1

(Dispersion parameter for poisson family taken to be 1)

    Null deviance: 8.1283  on 5  degrees of freedom
Residual deviance: 3.1162  on 4  degrees of freedom
AIC: 20.816

Number of Fisher Scoring iterations: 5
```

## W8A — Hypothesis tests

Use `x <- c(1,4,6,9,12)` and `y <- c(2,3,7,8,10)`. The return class is `"htest"` except for KS, which returns `c("ks.test","htest")`. Intervals carry a numeric `conf.level` attribute, normally `.95`. The field order is:

```text
t.test       statistic parameter p.value conf.int estimate null.value stderr alternative method data.name
wilcox.test  statistic parameter p.value null.value alternative method data.name [conf.int estimate]
chisq.test   statistic parameter p.value method data.name observed expected residuals stdres
fisher 2x2   p.value conf.int estimate null.value alternative method data.name
fisher rxc   p.value alternative method data.name
ks.test      statistic p.value alternative method data.name exact
shapiro.test statistic p.value method data.name
prop.test    statistic parameter p.value estimate null.value conf.int alternative method data.name
binom.test   statistic parameter p.value conf.int estimate null.value alternative method data.name
cor.test     statistic parameter p.value estimate null.value alternative method data.name [conf.int]
```

Square brackets mark fields that are conditional in these probes. A `NULL` value in the table below is a field present in the list. The table uses `—` for a field absent from the return list.

| Call | Exact `method` | Named statistic, parameter | `p.value` | Estimate and interval |
|---|---|---|---:|---|
| `t.test(x,mu=3)` | `One Sample t-test` | `t=1.777208469864`, `df=4` | `.150172090081346` | `mean of x=6.4`, `[1.088347755626,11.711652244374]` |
| `t.test(x,y)` | `Welch Two Sample t-test` | `t=.163846384104`, `df=7.604058740420` | `.874122834510820` | `mean of x=6.4`, `mean of y=6`, `[-5.281108240225,6.081108240225]` |
| `t.test(x,y,var.equal=TRUE,alternative="greater")` | ` Two Sample t-test` with one leading space | `t=.163846384104`, `df=8` | `.436957929290335` | Same means, `[-4.139735308050,Inf]` |
| `t.test(x,y,paired=TRUE,alternative="less")` | `Paired t-test` | `t=.666666666667`, `df=4` | `.729265130362208` | `mean difference=.4`, `[-Inf,1.679108071796]` |
| `wilcox.test(x,mu=3)` | `Wilcoxon signed rank exact test` | `V=13`, `NULL` | `.1875` | — |
| `wilcox.test(x,y,conf.int=TRUE)` | `Wilcoxon rank sum exact test` | `W=13`, `NULL` | `1` | `difference in location=1`, `[-6,7]`, level `.968253968253968` |
| `wilcox.test(x,y,exact=FALSE,conf.int=TRUE)` | `Wilcoxon rank sum test with continuity correction` | `W=13`, `NULL` | `1` | `difference in location=.999998029266397`, `[-6.000052149410,6.999974335545]` |
| `wilcox.test(x,y,paired=TRUE,exact=FALSE)` | `Wilcoxon signed rank test with continuity correction` | `V=10`, `NULL` | `.571607644953332` | — |
| `chisq.test(c(12,8,10))` | `Chi-squared test for given probabilities` | `X-squared=.8`, `df=2` | `.670320046035639` | — |
| `chisq.test(matrix(c(8,2,1,5),2))` | `Pearson's Chi-squared test with Yates' continuity correction` | `X-squared=3.809523809524`, `df=1L` | `.0509619369677633` | — |
| `fisher.test(matrix(c(8,2,1,5),2))` | `Fisher's Exact Test for Count Data` | — | `.0349650349650349` | `odds ratio=15.469687462887`, `[1.008849380397,1049.79144613176]` |
| `fisher.test(matrix(c(3,1,2,2,4,1),2))` | `Fisher's Exact Test for Count Data` | — | `.776223776223776` | — |
| `ks.test(x,"punif",0,15)` | `Exact one-sample Kolmogorov-Smirnov test` | `D=.2` | `.9616` | — |
| `ks.test(x,y)` | `Exact two-sample Kolmogorov-Smirnov test` | `D=.2` | `1` | — |
| `ks.test(x,y,alternative="less",exact=FALSE)` | `Asymptotic two-sample Kolmogorov-Smirnov test` | `D^-=.2` | `.818730753077982` | — |
| `shapiro.test(x)` | `Shapiro-Wilk normality test` | `W=.991167491316520` | `.983619061209332` | — |
| `prop.test(7,10)` | `1-sample proportions test with continuity correction` | `X-squared=.9`, `df=1L` | `.342781711147911` | `p=.7`, `[.353670723511,.919052175790]` |
| `prop.test(c(7,3),c(10,10))` | `2-sample test for equality of proportions with continuity correction` | `X-squared=1.8`, `df=1` | `.179712494879` | `prop 1=.7`, `prop 2=.3`, `[-.101673089053,.901673089053]` |
| `binom.test(7,10)` | `Exact binomial test` | `number of successes=7`, `number of trials=10` | `.34375` | `probability of success=.7`, `[.347547149940,.933260488822]` |
| `cor.test(x,y)` | `Pearson's product-moment correlation` | `t=6.379031393753`, `df=3L` | `.00779937207614279` | `cor=.965058345326117`, `[.557290835692,.997778048947]` |
| `cor.test(x,y,method="spearman")` | `Spearman's rank correlation rho` | `S=4.44089209850063e-15`, `NULL` | `.0166666666666667` | `rho=1` |
| `cor.test(x,y,method="kendall")` | `Kendall's rank correlation tau` | `T=10`, `NULL` | `.0166666666666666` | `tau=1` |

Additional field and calculation checks:

- The four t-test standard errors are `1.91311264697090`, `2.44131112314674`, `2.44131112314674`, and `.6`. Their named null values are `mean=3`, `difference in means=0` for both two-sample tests, and `mean difference=0` for paired. Other named null values include `location=3` for the one-sample Wilcoxon, `location shift=0` for the two-sample Wilcoxon, `odds ratio=1` for Fisher, `p=.5` for one-sample proportions, `probability of success=.5` for binomial, and `correlation=0`, `rho=0`, `tau=0` for the three correlation tests. Two-sample `prop.test` without supplied probabilities has `null.value=NULL`.
- `wilcox.test(c(0,1,1,2),exact=TRUE)` returns `V=9,p=.25` with no warning. `exact=FALSE` returns `V=6,p=.173568166555921`. Paired exact testing on `x,y` gives `V=10,p=.75`. `wilcox.test(x,y,exact=FALSE,correct=FALSE)` gives `p=.916814948528089` with method `Wilcoxon rank sum test`.
- For the goodness-of-fit test, `observed=c(12,8,10)`, `expected=c(10,10,10)`, `residuals=c(.632455532034,-.632455532034,0)`, and `stdres=c(.774596669241,-.774596669241,0)`. For the `2 × 2` test, `expected` in column order is `c(5.625,4.375,3.375,2.625)`, `residuals` is `c(1.001387925720,-1.135467178868,-1.292786253136,1.465881824640)`, and `stdres` is `c(2.472280184803,-2.472280184803,-2.472280184803,2.472280184803)`. The `2 × 2` call warns `Chi-squared approximation may be incorrect`.
- `alternative` normally stores `"two.sided"`, `"less"`, or `"greater"`. Printed wording uses `not equal to`, `less than`, or `greater than` with the named null value. KS stores `"two-sided"` or a complete phrase. For two samples, the phrases are `"the CDF of x lies below that of y"` and `"the CDF of x lies above that of y"`. One-sample KS uses `"the null hypothesis"` in place of `"that of y"`. The greater statistic is `D^+`. Two-sample KS accepted ties with `exact=TRUE` in the probe. One-sample KS with ties defaults to asymptotic and warns `ties should not be present for the one-sample Kolmogorov-Smirnov test`.
- `data.name` uses deparsed inputs in the fixtures. Examples are `"x"`, `"x and y"`, `"matrix(c(8, 2, 1, 5), 2)"`, `"7 and 10"` for binomial, and `"7 out of 10, null probability 0.5"` for proportions.

## W7A — Optimization, roots, and integration

`optim` returns `par,value,counts,convergence,message` in order. `counts` is an integer vector named `c("function","gradient")`. `hessian=TRUE` appends `hessian`, a double matrix with both dimname axes taken from the parameter names.

For `f <- function(x) sum((x-c(1,-2))^2)` and `par=c(x=0,y=0)`, using default controls and numerical gradients where applicable:

| Method | `par` | `value` | `counts` | `convergence` |
|---|---|---:|---|---:|
| Nelder–Mead | `.999889549005,-2.000012440038` | `1.23541768553422e-8` | `69L,NA` | `0L` |
| BFGS | `.999999999999777,-2.00000000000011` | `6.35713544493184e-26` | `9L,3L` | `0L` |
| CG | `1.000000410364,-2.000000820729` | `8.41994451603953e-13` | `26L,11L` | `0L` |
| L-BFGS-B | `1,-2` | `0` | `4L,4L` | `0L` |
| SANN, after `set.seed(42)` | `.991150323405,-2.000087365386` | `7.83244085550476e-5` | `10000L,NA` | `0L` |
| Brent, scalar `(x-1)^2` on `[-4,4]` | `1` | `0` | `NA,NA` | `0L` |

On these calls, `message` is `NULL` except for L-BFGS-B, which gives `CONVERGENCE: REL_REDUCTION_OF_F <= FACTR*EPSMCH`. With bounds `[-.5,.5]` on both dimensions, it gives `par=c(.5,-.5)`, `value=2.5`, `counts=c(2L,2L)`, and `CONVERGENCE: NORM OF PROJECTED GRADIENT <= PGTOL`.

- `maxit=1` with the analytic gradient gave code `1L` for Nelder–Mead, BFGS, CG, and L-BFGS-B. L-BFGS-B gave `message="NEW_X"`. SANN gave `0L`.
- Nelder–Mead gives `10L`, `counts=c(9L,NA)`, `message=NULL` for `optim(c(0,0), function(x) if(all(x==0)) 0 else 1, control=list(beta=1,maxit=35,reltol=0))`.
- L-BFGS-B gives `52L` with incompatible bounds and message `ERROR: NO FEASIBLE SOLUTION`. With `control=list(lmm=0)` it gives `ERROR: M .LE. 0`. An objective `function(x) 0` with gradient `function(x) c(2,1)` gives `52L` and `ERROR: ABNORMAL_TERMINATION_IN_LNSRCH`, with counts `21L,21L`.
- Brent requires one parameter and finite bounds. The errors are `method = "Brent" is only available for one-dimensional optimization` and `'lower' and 'upper' must be finite values`. Supplying bounds with BFGS warns `bounds can only be used with method L-BFGS-B (or Brent)` and runs L-BFGS-B.

| Function | Verified return and failure behavior |
|---|---|
| `optimize` | `optimize(function(x)(x-1)^2,c(-4,4))` gives `list(minimum=1,objective=0)`. With the negated objective and `maximum=TRUE`, the first field is `maximum=1`. A reversed `interval` vector works. Equal endpoints give `'xmin' not less than 'xmax'`. |
| `uniroot` | Ordered fields `root,f.root,iter,init.it,estim.prec`. For `function(x)x^2-2,c(0,2)` they are `1.41421331999550,-6.85547298484579e-7,6L,NA_integer_,6.10351562506661e-5`. An exact endpoint root gives `iter=0L,estim.prec=0`. For `function(x)x-3,c(0,2),extendInt="upX"` they are `3,0,7L,6L,3`. An unbracketed root gives `f() values at end points not of opposite sign`. For `maxiter=1`, the square-root fixture warns `_NOT_ converged in 1 iteration`. `check.conv=TRUE` turns the warning into an error. |
| `integrate` | Class `"integrate"` with `value,abs.error,subdivisions,message,call` in order. Integrating `function(x)x^2` over `[0,1]` gives `1/3,3.70074341541719e-15,1L,"OK"` and a matched call. Default print is `0.3333333 with absolute error < 3.7e-15`. For `dnorm` over `[-Inf,Inf]` the result is `.999999999794159,9.36112408670003e-5,3L,"OK"` and default print is `1 with absolute error < 9.4e-05`. |
| `integrate` failures | A scalar constant callback gives `evaluation of function gave a result of wrong length`. Nonfinite output gives `non-finite function value`. With `function(x)1/x`, `[0,1]`, `subdivisions=2`, and `stop.on.error=FALSE`, the result contains `value=8.40577600851064`, `abs.error=9.35056037314017`, `subdivisions=2L`, and `message="maximum number of subdivisions reached"`. It prints `failed with message ‘maximum number of subdivisions reached’`. With default `stop.on.error`, the same message is an error. |
