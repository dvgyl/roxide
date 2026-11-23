# Milestone 2 Numerical, PRNG & Linear Algebra Specification (Clean-Room)

Empirically verified against official **GNU R 4.6.2 (2026-10-29)** arm64 black-box oracle and published mathematical literature. Zero GNU R source code consulted.

## 1. Bit-Exact Reproducible PRNG (`set.seed`, `.Random.seed`, `RNGkind`)

Default `RNGkind()`: `c("Mersenne-Twister", "Inversion", "Rejection")`.  
`.Random.seed[1] = uniform_kind + 100 * normal_kind + 10000 * sample_kind`. Preserve all raw 32-bit state patterns (including negative integers and `i32::MIN`).

### Seeding (`set.seed(seed)`)
Let $s_0 = \text{seed}$ (32-bit), $s_i = (69069 \cdot s_{i-1} + 1) \bmod 2^{32}$. Discard $s_1 \dots s_{50}$:
- **Mersenne-Twister (`10403`):** `[10403, 624, s_52 .. s_675]` (discarding $s_{51}$ is essential!).
- **Wichmann-Hill (`10400`):** `[10400, x, y, z]` with $s_{51} \bmod 30269$, $s_{52} \bmod 30307$, $s_{53} \bmod 30323$ (replace 0 with 1).
- **L'Ecuyer-CMRG (`10407`):** `[10407, x0, x1, x2, y0, y1, y2]` starting at $s_{51}$, taking 6 words skipping values $\ge 4294944443$.

### Uniform Steps
- **MT19937:** Twist at cursor `624` in ascending index order (`0x9908b0df`), reset cursor to `0`, temper (`>> 11`, `(<< 7) & 0x9d2c5680`, `(<< 15) & 0xefc60000`, `>> 18`). Return `f64(y) * 2^-32`; if `y == 0`, return `0x1.00000000fffffp-33`.
- **Wichmann-Hill:** `x = 171*x % 30269`, `y = 172*y % 30307`, `z = 170*z % 30323`; return `fract((x/30269.0 + y/30307.0) + z/30323.0)`.
- **L'Ecuyer-CMRG:** $m_1 = 4294967087$, $m_2 = 4294944443$, $h = (1403580 x_1 - 810728 x_0) \bmod m_1$, $l = (527612 y_2 - 1370589 y_0) \bmod m_2$, shift triples left, $d = h - l$ (`+ m1` if $\le 0$), return `f64(d) * (1.0 / 4294967088.0)`.

### Default `rnorm()` (Inversion) & `sample.int()` (Rejection)
- **`rnorm()`:** Draw two consecutive uniforms $u_1, u_2$; evaluate `qnorm((floor(2^27 * u1) + u2) / 2^27)`.
- **`sample.int(n, size, replace)` (unweighted Rejection):** Let $b = \lceil \log_2 n \rceil$; concatenate $\lfloor b/16 \rfloor + 1$ 16-bit chunks `floor(65536 * U)` MSB-first via integer shifts, mask low $b$ bits, retry if $\ge n$. Without replacement: pool swap-remove for $n \le 10^7$ or $\text{size} > n/2$; hash-set retry for $n > 10^7$ and $\text{size} \le n/2$.

### Seed-42 Oracle Anchors
- `runif(5)` little-endian bytes: `0000e0521746ed3f`, `0000c09385fced3f`, `000000321c50d23f`, `000060e60693ea3f`, `000000e62d89e43f`.
- `sample(10)`: `1 5 10 8 2 4 6 9 7 3`; `sample(100, 10)`: `49 65 25 74 18 100 47 24 71 89`.

## 2. Special Functions, Distributions & R 4.6.2 `rhyper()` Fix

- **`rhyper(nn, m, n, k)`:** Form $N = m + n$ in `f64` (round fractional counts ties-to-even).
  - $N \le 2147483647$: Kachitvichyanukul–Schmeiser hypergeometric generator.
  - $N > 2147483647, k = 1$: `rbinom(1, 1, m / N)`.
  - $N > 2147483647, k \ne 1$: draw one uniform $U$ and evaluate `qhyper(U, m, n, k, lower.tail = FALSE)`.
- **Special functions & tails:** Lanczos + Stirling + `sinpi` reflection (`gamma`/`lgamma`/`lbeta`), lower series + upper continued fraction + Temme expansion (`pgamma`/`pbeta`), Wichura AS 241 + Mächler extreme log-tail expansion (`qnorm`), Loader deviance/Stirling (`dbinom`/`dpois`), Ahrens–Dieter (`rgamma`/`rpois`), BTPE (`rbinom`).

## 3. Clean-Room Linear Algebra & FFT

- **`qr()` & `lm.fit()`:** Input-order Householder QR with LINPACK-style relative column norm test `remaining_norm < tol * original_norm` (`tol = 1e-7`), deferring negligible columns to the right while preserving order of accepted columns (`qr(cbind(a=1:4, b=2*(1:4), c=1))$pivot == c(1, 3, 2)`, rank 2, `lm.fit` assigns `NA` to `"b"`).
- **`svd()`:** Golub–Kahan bidiagonalization + implicit QR / divide-and-conquer, descending singular values.
- **`eigen()`:** Symmetric/Hermitian reads **lower** triangle -> tridiagonal QR (descending); general -> balance + Hessenberg + shifted QR Schur -> right eigenvectors sorted by decreasing modulus.
- **`chol()`:** Reads **upper** triangle -> upper triangular $R$ with $R^T R = A$ (`pivot = TRUE` diagonal pivoting).
- **`solve()` & `det()`:** LU with partial row pivoting; log-determinant accumulates sign + sum of log abs diagonal.
- **`fft()`:** Unnormalized (`-` forward, `+` inverse), radix-2 for powers of 2 and Bluestein chirp-z for arbitrary sizes, multi-dimensional across array dims.
