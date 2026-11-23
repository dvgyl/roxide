# Milestone 2 linear algebra and FFT specification

Verified with GNU R 4.6.2 on `aarch64-apple-darwin23`, using the bundled `libRblas` and LAPACK 3.12.1. All oracle calls used `Rscript --vanilla`.

Three results affect implementation. Pivoted Cholesky uses half of `.Machine$double.eps` in its default cutoff. Default QR can store reflectors beyond its reported rank. The first Householder reflector alone does not determine the signs of the final singular vectors or eigenvectors.

## W3B · Default QR and least squares

**Limited pivoting.** Use one-based indices below. For each column, save its original 2-norm and initialize its current norm to that value. Replace an original zero norm with `1` **only in the reference value used for the tolerance test**. Set `end = ncol` and use `tol = 1e-7` by default.

1. At position `l`, while `l <= end` and `current[l] < tol * original[l]`, move the entire column at `l` to the last physical column. Shift all intervening columns one position left. Move their norms and pivot indices with them. Decrement `end` and test the new column at `l`. Equality retains the column.
2. Process every position through `min(nrow,ncol)`, including positions beyond `end`. Return `rank = min(nrow,end)`. For `diag(c(1,1,0))`, `tol=0` gives rank 3, `tol=1` gives rank 2, and `tol=1+.Machine$double.eps` gives rank 0. In the last case the packed diagonal still begins `-1,-1`.
3. If `l == nrow`, skip reflector construction and retain the current norm in `qraux[l]`. Otherwise take `x = A[l:nrow,l]`. If its norm is zero, skip the reflector. Set `s = -norm(x)` when `x[1] < 0`, and `s = norm(x)` otherwise. Both signs of zero take the positive branch.
4. Compute the reciprocal `1/s` once. Multiply each entry of `x` by that reciprocal, then add `1` to the first entry to obtain `v`. Store `-s` on the diagonal, `v[-1]` below it, and `v[1]` in `qraux[l]`. For each trailing column segment `y`, compute `t = -dot(v,y)/v[1]` and update `y = y + t*v`.
5. For each nonzero trailing current norm `c`, use the **updated** leading entry to compute `u = abs(y[1])/c` and `w = max(0, fma(-u,u,1))`. The rule `w >= 1e-6` selects `c * sqrt(w)`. Below that threshold, recompute the norm of the remaining tail. Keep the original reference norm for subsequent pivot tests. For columns `(1,0)` and `(1,x)`, `x=.001` selects recomputation and `x=.0010000005` selects the downdate.

**Bit verification.** An independent implementation matched every binary64 entry of `$qr` and `$qraux`, including signed zeros, and every `$pivot` and `$rank` on 18 fixtures (including canonical full-rank matrices, two dependent designs, a downdate boundary fixture, and 13 seeded three-column matrices with 2 through 14 rows). Accumulate squared norms and dot products in index order with fused multiply-add (`f64::mul_add`). Use `t.mul_add(v[i], y[i])` for the column update.

**Multiple right-hand sides.** Let `k` be the reported rank. Apply the first `min(k,nrow-1)` reflectors in increasing index order to each response column to obtain the effects $E=Q^\mathsf{T}Y$. Back-substitute in the leading `k × k` upper triangle to obtain the active coefficients. To obtain residuals, zero the first `k` rows of `E` and apply the reflectors in reverse order. `lm.fit` computes fitted values as `Y - residuals`. The low-level `.lm.fit` returns coefficients in pivot order with zero alias rows. `lm.fit` restores design-column order and puts `NA` in the alias rows.

Fixture: `D = cbind(a=0,b=c(1,0,0,0),c=0,d=c(0,1,0,0),e=c(1,1,0,0))`, `Y = cbind(c(1,2,4,3),c(1,-1,2,5))`, rank `2L`, pivot `c(2L,4L,1L,3L,5L)`:

| `lm.fit(D,Y)` field | First response | Second response |
|---|---|---|
| `coefficients` | `NA,1,NA,2,NA` | `NA,1,NA,-1,NA` |
| `effects` | `-1,-2,4,3` | `-1,1,2,5` |
| `residuals` | `0,0,4,3` | `0,0,2,5` |
| `fitted.values` | `1,2,0,0` | `1,-1,0,0` |

## W3B · LAPACK factorizations

**LU, `rcond`, and `solve`.** Use partial row pivoting. A zero diagonal in `U` gives an exact-singularity error in `solve`, including when `tol=0`. For a nonsingular real system with positive `tol`, `solve` rejects the system when its estimated reciprocal **one-norm** condition number is strictly below `tol`. Equality passes (`diag(c(1, 2^-52))` passes default `tol = .Machine$double.eps`; next smaller binary64 fails).

For `rcond`, compute the requested norm of the original matrix (`"O"` = max abs col sum; `"I"` = max abs row sum). `DGECON` uses the `DLACN2` one-norm estimator with triangular solves (`U^{-1}L^{-1}` for `"O"`, transpose for `"I"`, omitting row permutation), starting with uniform vector, alternating sign-vector and coordinate-vector probes, and final alternating vector with linearly increasing magnitudes; preserve `(1 / inverse_norm_estimate) / original_norm`.

| Input in R notation | `rcond(x,"O")` | `rcond(x,"I")` |
|---|---:|---:|
| `matrix(c(1,2,3,7),2)` | `0x1.6c16c16c16c16p-7` | `0x1.6c16c16c16c17p-7` |
| Hilbert `H[i,j] = 1/(i+j-1)`, size 6 | `0x1.277d119df1f25p-25` | `0x1.277d119df20d7p-25` |
| `matrix(c(1,2,2,4),2)` | `0` | `0` |
| `matrix(c(-1,1,1,-1,1,-3,-3,-3,0),3)` | `1/3` | `1/5` |

**Cholesky.** `chol` reads the upper triangle. For `pivot=TRUE`, select the largest remaining diagonal after the previous updates. For `tol < 0`, the stopping threshold is `d_stop = n * 2^-53 * max_i(A_ii)` (`2^-53` = unit roundoff, half of `.Machine$double.eps`). A nonpositive initial maximum gives rank zero; a positive first pivot is accepted even when an explicit `tol` exceeds it; subsequent pivots stop on `pivot <= cutoff`, returning the accepted count and issuing the rank warning. `chol2inv` computes $R^{-1}R^{-\mathsf{T}}$ from upper triangular $R$.

**LAPACK QR (`DGEQP3`).** Selects the largest remaining column norm and swaps columns (ties go to first candidate). R reports `rank = min(nrow, ncol)` even for zero trailing diagonals. For nonzero tail, `DLARFG` uses `beta = -copysign(hypot(alpha, norm(tail)), alpha)`, stores `tail / (alpha - beta)` below diagonal, returns `tau = (beta - alpha) / beta` (with safe scaling). For `(-0, 4)`, LAPACK QR stores diagonal `+4` and tail `-1`, whereas default LINPACK QR stores `-4` and `+1`.

## W5B and W7B · Singular vectors and eigenvectors

Let `B = rbind(c(1,2,3),c(4,5,6),c(7,8,10))` and `M = rbind(B,c(2,0,1))`, `S = crossprod(X)`:

| `X` | `svd(X)$d` | Columns of `u` | Columns of `v` | Columns of `eigen(S,TRUE)$vectors` |
|---|---|---|---|---|
| `B` | `17.41250516680859, .8751613501104368, .1968665211174301` | `---`, `++-`, `+-+` | `---`, `-++`, `+-+` | `---`, `+--`, `+-+` |
| `M` | `17.48831889344151, 1.681288294994658, .5761700707347804` | `----`, `++--`, `+--+` | `---`, `-++`, `--+` | `---`, `+--`, `--+` |

For both fixtures, `t(La.svd(X)$vt)` is identical to `svd(X)$v`.

**General real matrices (`DGEEV`).** Return right eigenvectors in eigenvalue order, sorted by decreasing modulus. Normalize each vector to unit 2-norm and make a largest-magnitude component of a complex vector real. Conjugate pairs `a + ib`, `a - ib` are unpacked from adjacent real LAPACK columns `a, b` (positive imaginary eigenvalue first). Near-real exception: when `|Im| <= ~1e-18` (e.g. `1e-18`), R treats the pair as real.

## W7B · FFT and `nextn`

- Forward transform: $\exp(-2\pi i j k / n)$; inverse: $\exp(+2\pi i j k / n)$, unnormalized (forward followed by inverse scales by total length).
- N-D arrays: transform each axis in increasing dimension order (`1, 2, 3, ...`, axis 1 unit-stride column-major).
- **`nextn(n, factors = c(2, 3, 5))`:** Preserve `factors` input order! Test candidates `m >= floor(n)` by dividing out each factor in order as many times as possible before advancing to the next factor, accepting when remainder reaches `1` (`nextn(36, c(4, 6)) == 64L`, `nextn(36, c(6, 4)) == 36L`, `nextn(97) == 100L`, `nextn(1000) == 1000L`).
