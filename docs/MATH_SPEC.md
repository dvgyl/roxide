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

---

## Milestone 2 W1 numerical oracle results

Oracle: GNU R 4.6.2 (2026-10-29), `aarch64-apple-darwin23`. All probes used the installed binary. No repository files were changed. Cargo was not run.

Three details affect bit parity directly. The two-word generators use a scale one binary64 step below the rounded reciprocal of `2^32 − 1`. An explicit uniform-kind switch uses `floor(U * (2^32 − 1))`. On this arm64 build, AS 241 needs fused multiply-add operations in both the central argument and the Horner polynomials. With those operations, all 1,000 seed-42 inversion results matched exactly.

### 1. Uniform generators

Let `s[i]` denote the existing `69069` seed recurrence in [MATH_SPEC.md](/Users/devg/Documents/Roxide/docs/MATH_SPEC.md). Interpret generator words as unsigned 32-bit integers during arithmetic. Store the same bit patterns as signed R integers. The headers below select Inversion and Rejection.

| Kind | Initial `.Random.seed` for seed 42 | `.Random.seed` after five uniforms |
|---|---|---|
| Marsaglia-Multicarry | `10401 -2133391687 507561766` | `10401 1454224590 728335676` |
| Super-Duper | `10402 -2133391687 507561767` | `10402 -1396781172 -891106197` |

**Marsaglia-Multicarry.** Initialize `(a,b) = (s[51],s[52])`. Replace each zero word with `1`, including when restoring a state. For one draw, update both words, then combine them.

```text
a = 36969 * (a & 65535) + (a >> 16)
b = 18000 * (b & 65535) + (b >> 16)
w = ((a << 16) | (b & 65535)) mod 2^32
```

**Super-Duper.** Initialize `(a,b) = (s[51],s[52])`. Replace zero `a` with `1`. Set the low bit of `b`, including when restoring a state. Use a logical right shift. The left shift discards bits above bit 31.

```text
a = a XOR (a >> 15)
a = a XOR (a << 17)
b = 69069 * b mod 2^32
w = a XOR b
```

For both generators, return `f64(w) * 0x1.00000000fffffp-32`. If `w == 0`, return `0x1.00000000fffffp-33`. Each recurrence matched 100 consecutive outputs and the final state. Using `1.0 / 4294967295.0` changed all 100 outputs for each generator. Explicit states that produced `w == 0` also confirmed the endpoint rule.

**Knuth-TAOCP and Knuth-TAOCP-2002.** Each `.Random.seed` has 102 integers in this order: header, 100 state words, cursor. The initial cursor is `100`. Pass `unsigned(s[50]) % 1073741821` to the appropriate Knuth initializer. For seed 42, this argument is `268773787`. The published 1997 `ran_start` gives the kind-4 array directly. For kind 6, use the 2002 `ran_start`, including its ten advances of 199 terms each. These initializers reproduced all 100 words for six seeds per kind, including negative seeds. [Knuth, *Seminumerical Algorithms*, §3.6](https://studylib.net/doc/27876668/the-art-of-computer-programming---volume-2--seminumerical...) · [Knuth’s account of the 2002 revision](https://cs.stanford.edu/~knuth/news02.html)

The refill position matters. On refill, let the saved words be `X[0..99]`. Extend the sequence with `X[j] = (X[j−100] − X[j−37]) mod 2^30`. Save **`X[1009..1108]`** as the new block. Return its first word and save cursor `1`. Subsequent draws return the word at the cursor and increment it. Restored cursors `−1`, `0`, `100`, `101`, and `1000` all caused a refill. Restored cursors `1` and `99` selected those zero-based positions. An all-zero array caused a fresh initialization with different results on repeated trials.

Return `f64(word) * 0x1.0000000000002p-30`. A zero word returns `0x1.00000000fffffp-33`. The refill and scale reproduced 105 consecutive draws and the complete state across the second refill for each kind.

The complete seed-42 Knuth states can be checked with SHA-256. Hash all 102 signed integers as little-endian 32-bit words, including the header and cursor.

| Kind | Initial state | State after `runif(5)` |
|---|---|---|
| `10404` | `7cb71bb15d201c8a56627d81b9f34f92e738e4c155a714ac414be35647b5bcdd` | `2c356830b91346af2c27a2f1437d0b94da6b961ccac035e18f83bf9aa3e92865` |
| `10406` | `1ba1063442bec585f8c6b6befbf41e2f3d8e7fc2e5efd25fab3855197b23310c` | `9acf225f11aec29266cc722a1a7484ebfb10f39888a5d004a2e614de1bfe7dfe` |

**Seed-42 `sprintf("%a", runif(5))`**

```text
Marsaglia-Multicarry
0x1.4ae2f8814ae2ep-2  0x1.c50d82c9c50d6p-2  0x1.4c905f614c905p-2  0x1.33513c1f33513p-1  0x1.699d0a79699cfp-1

Super-Duper
0x1.8bb6e4598bb6dp-1  0x1.ad498bb1ad497p-1  0x1.0b6a53e90b6a4p-2  0x1.926d8f59926d7p-2  0x1.99700f9d996ffp-2

Knuth-TAOCP
0x1.08734d0000002p-6  0x1.79f24c0000003p-4  0x1.19e1d6e000002p-1  0x1.1a6b4ac000002p-3  0x1.06a4819000002p-2

Knuth-TAOCP-2002
0x1.05742b0000002p-3  0x1.111be74800002p-1  0x1.cb4f915000004p-2  0x1.0d7b50a000002p-3  0x1.34f4ea2800002p-1
```

### 2. Explicit `RNGkind()` switch

Draw **one** uniform `U` from the previous generator. Compute `floor(U * 4294967295.0)` in binary64. Pass its 32-bit pattern to the destination generator’s ordinary initializer. An explicit request for the current uniform kind also reinitializes it.

This rule reproduced the complete destination state and the next five hexadecimal outputs for all 49 ordered pairs of the seven built-in uniform generators, with starting seeds `42`, `−17`, and `1`. All 147 comparisons passed.

For example, seed-42 MT gives `U = 0x1.d461752ep-1`. The derived seed has signed value `−365905258`. Switching immediately to Marsaglia-Multicarry gives:

```text
initial state   10401 320834453 1978554194
five uniforms   0x1.19e15c7119e15p-3  0x1.04b310b904b3p-1  0x1.34c3a16d34c39p-1  0x1.5a8112995a81p-3  0x1.ae6a5431ae6a4p-4
final state     10401 409934566 158180675
```

### 3. Normal and exponential generators

The following vectors use seed-42 MT. Counts give the number of uniforms consumed by **each** output. For legacy normal kind `0`, initialize MT and set `.Random.seed[1] = 10003L`. The oracle rejects the legacy name as a `set.seed()` argument.

```text
Ahrens-Dieter                 counts  1 1 1 1 1
-0x1.45e8e10022989p+0  -0x1.6a26f6983c373p+0   0x1.7a872094a75e9p-1  -0x1.c9410ab35ee05p-1  -0x1.48c53170abac3p-2

Box-Muller                    counts  2 0 2 0 2
 0x1.3d8b112921205p-2  -0x1.78a26dfb7128dp-3  -0x1.190c47c8507c9p-3   0x1.301720cea12f4p-1  -0x1.70c01633c17c6p-1

Kinderman-Ramage              counts  3 2 2 2 2
-0x1.97937d1158d6ep-1   0x1.49a7ef1e8a601p+0   0x1.6f55b773052ddp-1  -0x1.b0b31e9b8c8afp-2   0x1.219fb6ea2b9b5p-1

Buggy Kinderman-Ramage        counts  3 2 2 2 2
-0x1.97937d1158d6ep-1   0x1.49a7ef1e858f2p+0   0x1.6f55b772ff289p-1  -0x1.b0b31e9b8faabp-2   0x1.219fb6ea236ep-1

rexp(5), rate = 1             counts  3 1 1 1 1
 0x1.96319c41a33e1p-3   0x1.5260dcccp-1        0x1.224b798p-2        0x1.38de374p-5        0x1.e4886a1p-2
```

**Box–Muller.** Draw the angle uniform first. Set `theta = (2π) * U1` and `r = sqrt(−2 * log(U2))`. Return `r * cos(theta)` and cache `r * sin(theta)`. The next call returns the cached value and consumes zero uniforms. Darwin’s paired sine/cosine evaluation reproduced all 10,000 outputs. Separate scalar sine and cosine calls differed on nine cached outputs. Restoring `.Random.seed` alone after consuming a cached value did not restore that value.

**Kinderman–Ramage.** Let `A = 2.216035867166471`. Draw selector `a` once. If `a < 0.884070402298758`, draw `b` and return `A * (fma(c,a,b) − 1)`. Use `c = 1.131131635444180` for kind 5 and `1.13113163544180` for kind 0.

For the remaining regions, draw a fresh pair `(b,c)` on each attempt. Keep the original selector on rejection. Set `m = min(b,c)`, `M = max(b,c)`, and `g(t) = 0.398942280401433 * exp(−t²/2) − 0.180025191068563 * (A − abs(t))`. For the three interior regions, return `t` if `b < c` and `−t` otherwise. [Kinderman and Ramage, 1976](https://www.tandfonline.com/doi/abs/10.1080/01621459.1976.10480965)

| Selector range, lower bound inclusive | Candidate and acceptance |
|---|---|
| `a >= 0.973310954173898` | `v = (A*A) − 2*log(c)`. Round `A*A` before subtraction. Accept if `b*b*v <= A*A`. Return `sqrt(v)` for `a < 0.986655477086949`, otherwise `−sqrt(v)`. |
| `0.958720824790463 <= a < 0.973310954173898` | `t = fma(−0.630834801921960,m,A)`. Accept if `M <= 0.755591531667601` or `0.034240503750111 * abs(b−c) <= g(t)`. |
| `0.911312780288703 <= a < 0.958720824790463` | `t = fma(1.105473661022070,m,0.479727404222441)`. Accept if `M <= 0.872834976671790` or `0.049264496373128 * abs(b−c) <= g(t)`. |
| `0.884070402298758 <= a < 0.911312780288703` | `t = fma(−0.595507138015940,m,0.479727404222441)`. Kind 5 retries if `t < 0`. Otherwise it accepts if `M <= 0.805577924423817` or `0.053377549506886 * abs(b−c) <= g(t)`. Kind 0 accepts solely on `M <= 0.805577924423817`. |

The central branch consumes two uniforms. Each other branch consumes `1 + 2 × attempts`. The candidate reproduced 20,000 outputs for each kind. A separate 10,000-output check per kind also verified consumption.

**Ahrens–Dieter.** Use algorithm FL with 32 strips. The tables can be generated from quantiles. Let `B[i] = Φ⁻¹(0.5 + i/64)` for `i = 0..31`. For `i = 0..30`, let `T[i] = (B[i+1]² − B[i]²)/2` and `H[i] = (B[i+1] − B[i])/(1 − T[i])`. Calculate each table from the unrounded values, then round each stored entry to **seven significant decimal digits**. Generate tail widths from `D[j] = Φ⁻¹(2^(−6−j)) − Φ⁻¹(2^(−7−j))`, with the same rounding. [Ahrens and Dieter, 1973](https://tugraz.elsevierpure.com/de/publications/a-combinatorial-method-for-the-generation-of-normally-distributed/)

1. Draw `u`. Use a negative sign if `u > 0.5`. Set `v = 32 * (2*u − I[u > 0.5])`, `i = min(31,floor(v))`, then subtract `i` from `v`.
2. For `i > 0`, set `j = i−1` and `base = B[j]`. If `v > T[j]`, return the signed value of `base + (v−T[j])*H[j]`. **Round the product before adding `base`.**
3. Otherwise draw `w = U * (B[j+1]−base)` and set `z = w*(base+w/2)`. If `v > z`, accept `base+w` with the selected sign. Otherwise draw `x`. If `v < x`, draw a new `v` and return to the strip fast test in step 2. Otherwise set `z = x`, draw a new `v`, and repeat the acceptance test.
4. For `i == 0`, start with `base = B[31]` and `j = 0`. While `2*v < 1`, add `D[j]` to `base`, increment `j`, and double `v`. Set `w = (2*v−1)*D[j]` and `z = w*(base+w/2)`. Draw `v`. Accept `base+w` with the selected sign if `v > z`. Otherwise draw `x`. If `v >= x`, set `z = x` and repeat from the draw of `v`. If `v < x`, draw a new `w = U*D[j]`, recompute `z`, and repeat.

The candidate matched 15,000 seed-42 values. A separate 10,000-value check matched both values and draw counts. Directed tail probes matched through a first MT uniform of `2^-32`.

**One Ahrens–Dieter endpoint remains unresolved.** When MT produces a raw zero, its uniform fixup gives `0x1.00000000fffffp-33`. With the next uniform near `0.99`, the oracle consumed two draws and returned `0x1.8ebc976003827p+2`. Extending the mathematical width table by one more quantile difference gives a different result. This endpoint needs a separate compatibility rule before claiming full bit parity.

**Exponential.** Use Ahrens–Dieter algorithm SA. Let `L = log(2)` and `Q[k] = Σᵢ₌₁ᵏ Lⁱ/i!`, with `Q[16] = 1`. Draw `u` and set `a = 0`. While `2*u <= 1`, add `L` to `a` and double `u`. Set `v = 2*u−1`. If `v <= L`, return `a+v`. Otherwise take the smallest `k >= 2` with `v <= Q[k]`, draw **`k` additional uniforms**, and return `fma(minimum,L,a)`. The initial doubling loop consumes no additional uniforms. A directed first uniform of exactly `0.5` consumed 17 uniforms in total. The fused result matched 15,000 draws. Separate multiplication and addition changed 195 of them. The probes did not distinguish last-bit variants of every `Q[k]` constant. [Ahrens and Dieter, 1972](https://doi.org/10.1145/355604.361593)

### 4. AS 241 and inversion on arm64

Use Wichura’s double-precision AS 241 coefficients. Evaluate each numerator and denominator from highest degree to lowest with `acc.mul_add(r, coefficient)`. In the central region, set `q = p−0.5` and **`r = (−q).mul_add(q, 0.180625)`**. Evaluate the final result as `(q * numerator) / denominator`. For the tails, use `r = sqrt(−log(tail_probability))`. Use the published intermediate polynomial at `r−1.6` for `r <= 5` and the outer polynomial at `r−5` through `r = 27`. [Wichura, Algorithm AS 241](https://csg.sph.umich.edu/abecasis/gas_power_calculator/algorithm-as-241-the-percentage-points-of-the-normal-distribution.pdf)

| Central argument | Numerator Horner | Denominator Horner | Mismatches in `set.seed(42); rnorm(1000)` |
|---|---|---|---:|
| Separate | Separate | Separate | 523 |
| Separate | Fused | Fused | 90 |
| Fused | Separate | Fused | 373 |
| Fused | Fused | Separate | 291 |
| Fused | Fused | Fused | **0** |

The fully fused candidate also matched 5,000 additional probabilities, including outer-tail inputs through `r = 27`. The seed-42 sample alone cannot distinguish every individual Horner stage. Some high-degree stages produced the same final bits when individually evaluated without fusion.

```text
p                 qnorm(p)
0.075            -0x1.7085226d3e524p+0
0.01             -0x1.29c5c4630ff0ep+1
1e-10            -0x1.97203597a2155p+2
1e-11            -0x1.ad2f7bbec4805p+2
1e-30            -0x1.6ed94a4dacdc9p+3
1e-300           -0x1.286074064c26ep+5
0x1.c7ea3p-1054  -0x1.30b9331b88e97p+5
```

The final row tests the transition beyond AS 241. Continuing the fused outer polynomial there gives `−0x1.30b9331b88e98p+5`, one representable step from the oracle. Thus the extreme-tail branch also matters for ordinary subnormal probabilities.

```text
set.seed(42); sprintf("%a", rnorm(10))
 0x1.5ef721feab7dap+0  -0x1.21201e6483b04p-1   0x1.73d7ef2bc4ec3p-2   0x1.4406913e568bfp-1   0x1.9df883eacc22bp-2
-0x1.b2af9edeab018p-4   0x1.82f31b0a2f519p+0  -0x1.83b9322414bbap-4   0x1.025bb550495p+1    -0x1.00e07fc6da1f9p-4
```

### 5. `sample.int`

**Unweighted Rounding.** Use `1 + floor(n*U)` through `n = 2^25−1` for both Knuth generators. For the other five built-in generators, use it through `n = 2^31−1`. Above the applicable bound, use `U = (floor(2^25*U1) + U2)/2^25`. Eight outputs and the full final state matched on both sides of the bounds and at `n = 2^40`.

**Unweighted Rejection.** The existing 16-bit chunk rule matched 50 outputs and the complete final state at each of `n = 1, 2, 3, 65535, 65536, 65537, 2^31, 2^40`. At `n = 1`, a draw still consumes one uniform. At `n = 65536`, an attempt consumes two.

**Weighted sampling with replacement.** Normalize the weights to probabilities `p[i]`. Select Walker’s alias method exactly when **more than 200 entries satisfy `n*p[i] > 0.1`**. Both inequalities are strict. A directed `n = 256` probe kept 200 entries above the boundary and placed one more entry just below, exactly at, and just above `0.1/n`. Only the last case selected Walker’s method.

| Seed 42, `n = 256`, equal weights on first `m` entries | `sample.int(256,5,TRUE,prob)`, Rejection | Uniforms consumed |
|---|---|---:|
| `m = 200` | `184 189 60 168 130` | 5 |
| `m = 201` | `15 23 74 146 49` | 10 |

With Walker and Rejection, select the column with the rejection index generator, then draw a separate uniform for the alias decision. With Walker and Rounding, one uniform supplies both the column and the fractional part. The latter gave `168 163 74 190 110` for `m = 201` and consumed five uniforms.

**Default hash selection.** For unweighted sampling without replacement, the default uses hashing when **`n > 10^7` and `size <= n/2`**. The comparison used complete output vectors and complete final states against explicit `useHash` settings. At size `8000`, `n = 10000000` matched `FALSE` and `n = 10000001` matched `TRUE`. At `n = 10000002`, size `5000001` matched `TRUE` and size `5000002` matched `FALSE`.
