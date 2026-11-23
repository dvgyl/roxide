# Milestone 3 Wave 2 — Empirical Formal & Numerical Specification

Two results affect the Wave 2 acceptance tests. At `fuzz=0`, `hist()` accepts the first break and rejects the last break when `include.lowest=FALSE`, even with `right=TRUE`. Also, an ordinary lazy-load environment record preserves the environment lock but can discard individual binding locks. Both results came from direct oracle probes.

## 1. Histogram counts and break selection

**`hist()`**. Remove nonfinite observations. Let \(b_0,\ldots,b_m\) be the sorted breaks, with \(m\) bins. Compute one displacement \(d\) for the entire histogram.

| Number of bins | Displacement \(d\) |
|---|---|
| 1 or 2 | `fuzz * (max(x) - min(x))` |
| 3 or 4 | `fuzz * min(positive bin widths)` |
| 5 or more | `fuzz * median(diff(breaks))` |

Use the following **counting breaks**. Keep the original breaks for the result, midpoints, and density.

| `right` | `include.lowest` | First break | Interior breaks | Last break |
|---|---|---:|---:|---:|
| `TRUE` | `TRUE` | \(b_0-d\) | \(b_i+d\) | \(b_m+d\) |
| `TRUE` | `FALSE` | \(b_0+d\) | \(b_i+d\) | \(b_m+d\) |
| `FALSE` | `TRUE` | \(b_0-d\) | \(b_i-d\) | \(b_m+d\) |
| `FALSE` | `FALSE` | \(b_0-d\) | \(b_i-d\) | \(b_m-d\) |

First check the two outer counting breaks. Accept equality at the first. Accept equality at the last exactly when `include.lowest=TRUE`. For interior breaks, equality goes into the bin on the left when `right=TRUE` and into the bin on the right otherwise. Signal an error if any finite observation remains uncounted. This outer check matters when implementing the documented interval convention. [R histogram manual](https://stat.ethz.ch/R-manual/R-devel/library/graphics/html/hist.html)

For `x=c(0,1,1+1e-8,2)` and `breaks=0:2`, there are two bins. The default displacement is therefore **`2e-7`**. With `fuzz=0`, the interior break is `1` and the counts are `(2,2)`. With default fuzz, the interior counting break is `1.0000002` and the counts are `(3,1)`. For `breaks=c(0,1,2,4)`, the displacement is `1e-7` because the smallest positive width is `1`.

**`nclass.FD()`**. Let `z=signif(x,digits)`, with `digits=5` by default. Use R type-7 sample quantiles \(Q\) on `z`.

1. Set \(h=2[Q(.75)-Q(.25)]\).
2. If \(h=0\), try \(p=1/8,1/16,\ldots,1/512\) in order. Take the first positive \(h=[Q(1-p)-Q(p)]/(1-2p)\).
3. If all differences vanish, use \(h=3.5\,\mathrm{sd}(x)\), with the sample standard deviation of the **original** data.
4. Return `1` for zero spread. Otherwise return \(\lceil(\max x-\min x)n^{1/3}/h\rceil\), also using the original range. [R class-count manual](https://stat.ethz.ch/R-manual/R-devel/library/grDevices/html/nclass.html)

For 99 zeros and one `100`, the first positive difference occurs at \(p=1/128\). The difference is `22.65625`, so \(h=22.65625/(1-2/128)\). The unrounded count is approximately `20.1669`. The oracle returned **`21`**. For ten ones, it returned **`1`**.

**`pretty()`**. The following grid-index rule matched all 8,000 cases in a generated oracle comparison. Let \(L,H\) be the finite extrema, \(M=\max(|L|,|H|)\), \(d=H-L\), \(N=\max(n,1)\), and \(\epsilon=2^{-52}\). Empty or entirely nonfinite input returns `numeric(0)`. Let \(h=\texttt{high.u.bias}\), \(f=\texttt{u5.bias}\), and \(m=\texttt{min.n}\). The defaults are `n=5`, `m=n%/%3`, `shrink.sml=.75`, `h=1.5`, `f=.5+1.5*h`, and `eps.correct=0`. [R pretty-break manual](https://stat.ethz.ch/R-manual/R-devel/library/base/html/pretty.html)

1. Treat the span as small if \(M=0\), or if
   \[
   d<M\,3N\epsilon\left[1+\max\left(\frac1{1+h},\frac{1.5}{1+f}\right)\right].
   \]
   For a small span, start with `1` if \(M=0\), \(9+M/10\) if \(M>10\), and \(M\) otherwise. Multiply by `shrink.sml/max(1,m)`. For an ordinary span, use \(d/N\). If subtraction overflows, divide the endpoints before subtracting.
2. Clamp this cell size \(c\) to `[f.min*DBL_MIN, .8*DBL_MAX]`. Here `DBL_MIN` is the smallest positive **normal** double. The default `f.min` is `2^-20`. Set \(b=10^{\lfloor\log_{10}c\rfloor}\), \(u=b\).
3. Try \(v=2b,5b,10b\) in order, using biases \(h,f,h\). Accept \(v\) as the new \(u\) exactly when \(v-c<\mathrm{bias}(c-u)\). Stop at the first failed comparison.
4. Start with \(i=\lfloor L/u+10^{-10}\rfloor\), \(j=\lceil H/u-10^{-10}\rceil\). With `eps.correct=2`, or `eps.correct=1` on an ordinary span, move each limit outward by \(\epsilon\) times its absolute value. A zero lower or upper limit becomes `-DBL_MIN` or `+DBL_MIN`, respectively. Extend the indices until \(iu\le L+10^{-10}u\) and \(ju\ge H-10^{-10}u\), using the corrected limits. Move indices with overflowing endpoint products inward.
5. If \(k=m-(j-i)>0\), add \(k\) intervals. If exactly one original endpoint was zero, keep that end fixed. Otherwise put \(\lceil k/2\rceil\) on the left when \(i\ge0\), and \(\lfloor k/2\rfloor\) on the left when \(i<0\). Put the remainder on the right.
6. With `bounds=TRUE`, generate `j-i+1` values between `min(L,i*u)` and `max(H,j*u)` using R’s length-based `seq.int` arithmetic. With `bounds=FALSE`, use the grid endpoints. With `eps.correct=0`, snap values within about `1e-14` of one interval length from zero to zero. Use overflow-safe sequence generation even when both endpoints are finite.

Default oracle results include `pretty(0) → c(-1,0)`, `pretty(2) → c(0,2)`, `pretty(1e-300) → c(0,1e-300)`, and `pretty(1e300) → c(9e299,1e300)`. For `c(-1.7e308,1.7e308)`, the oracle returned **seven finite values**.

## 2. Base graphics coordinates and layout

Use inches for the intermediate rectangles. Margin order is **bottom, left, top, right**. Rectangle order is **left, right, bottom, top**. [R graphics parameter manual](https://stat.ethz.ch/R-manual/R-devel/library/graphics/html/par.html)

- One margin line is `csi*mex` inches. Thus `omi=oma*csi*mex` and `mai=mar*csi*mex` when line margins control the region. Respect the last setter when the caller supplies inches or normalized regions directly.
- Subtract the outer margins from `din` to obtain the inner origin \((L_o,B_o)\) and size \((W,H)\). For `fig=(l,r,b,t)`, the figure origin is \((L_o+lW,B_o+bH)\) and `fin=((r-l)*W,(t-b)*H)`. **`fig` is relative to the inner region.**
- For figure size \((F_w,F_h)\), `plt=(L/Fw,1-R/Fw,B/Fh,1-T/Fh)` using the figure margins. `pin` equals figure size times the corresponding `plt` spans. Setting `fin` directly centers the requested figure in the inner region. Setting `pin` directly centers the requested plot in the figure.
- For a user coordinate \(x\), let \(q=x\) on a linear axis and \(q=\log_{10}x\) on a log axis. Its physical position is `plot_left + pin_width*(q-usr[1])/(usr[2]-usr[1])`. Apply the same formula vertically. A descending axis retains its sign.

For `plot.window`, validate finite explicit limits first. An explicit negative log limit errors. An explicit zero log limit warns and supplies `-320` in log space. High-level `plot()` omits nonpositive log data when deriving its limits.

For equal transformed limits \(t,t\), expand each side by \(0.4|t|\), or by `1` when \(t=0\). A tiny *nonzero* span, below approximately \(16\epsilon\max(|L|,|H|)\), uses `0.01*maxabs` on each side. Then style `"r"` subtracts `0.04*(H-L)` from the first limit and adds it to the second. Style `"i"` uses the expanded limits directly. Thus `x=0,y=2` gives **`usr=c(-1.08,1.08,1.136,2.864)`**.

Log underflow correction is directional. If exponentiating the first transformed endpoint gives zero, replace it with `log10(1.01*DBL_MIN)`. If exponentiating the second overflows, use `log10(.99*DBL_MAX)`. On the oracle, log `xlim=c(0,10)` with style `"r"` gave `(-307.6483341948061,13.84)`. Reversing the limits gave `(13.84,-332.84)`.

`mfrow` fills across rows. `mfcol` fills down columns. Both start at the upper left. Setting either resets `mex` to `1`. Base `cex` becomes `.66` if either dimension is at least three, `.83` for exactly `2 × 2`, and `1` otherwise.

**`layout()`**. Convert `lcm(a)` to `a/2.54` inches. Subtract fixed sizes from each available dimension. Let \(s_x,s_y\) be the residual inches divided by the sum of relative weights on each axis. With `respect=FALSE`, use the scales independently. With `TRUE`, use \(s=\min(s_x,s_y)\) for both. An axis with only fixed tracks imposes no scale constraint. Center the resulting rectangle. Each positive figure ID gets the bounding rectangle of its occupied cells. Row 1 is at the top. [R layout manual](https://stat.ethz.ch/R-manual/R-devel/library/graphics/html/layout.html)

Base graphics has a specific rule for **matrix-valued `respect` mixed with `lcm()`**. Start at the common scale \(s\). On the axis with spare space \(E\) inches, form the set \(U\) of all fixed tracks and all relative tracks without a marked cell. Multiply each current physical size in \(U\) by
\[
1+\frac{E}{s\sum_{i\in U}a_i}.
\]
Here \(a_i\) is the relative weight or the **numeric centimetre value** of `lcm()`. An empty \(U\) leaves the spare space for centering. On an 8 × 4 inch device, widths `c(lcm(2.54),3)`, heights `c(2,2)`, and only cell `[1,1]` marked gave column widths **`1.722022` and `5.166065` inches**. This rule matched the generated mixed-layout probes.

## 3. Grid units, layouts, and graphical parameters

The oracle returned these unit codes. Spelling and case matter. [R grid unit manual](https://stat.ethz.ch/R-manual/R-devel/library/grid/html/unit.html)

| Codes | Names in code order |
|---|---|
| `0–6` | `npc`, `cm`, `inches`, `lines`, `native`, `null`, `snpc` |
| `7–13` | `mm`, `points`, `picas`, `bigpts`, `dida`, `cicero`, `scaledpts` |
| `14–18` | `strwidth`, `strheight`, `strascent`, `strdescent`, `char` |
| `19–24` | `grobx`, `groby`, `grobwidth`, `grobheight`, `grobascent`, `grobdescent` |
| `103–106` | `mylines`, `mychar`, `mystrwidth`, `mystrheight` |
| `201–203` | `sum`, `min`, `max` |

For viewport axis size \(D\) inches and native scale \((a,b)\), use these conversions.

| Input \(v\) | Position in inches | Dimension in inches |
|---|---:|---:|
| `npc` | \(Dv\) | \(Dv\) |
| `native` | \(D(v-a)/(b-a)\) | \(Dv/(b-a)\) |
| `snpc` | \(v\min(W,H)\) | \(v\min(W,H)\) |

The inverse native position adds \(a\). The inverse native dimension has no origin term. For `yscale=c(10,4)`, native location `7` gives `.5 npc`, and native height `3` gives **`-.5 npc`**. Convert `native` directly to `npc` when possible. On a zero-width viewport these conversions still work, although converting a nonzero physical length back to a viewport fraction errors.

Physical factors are `1 inch = 2.54 cm = 25.4 mm = 72.27 points = 72 bigpts`, `1 pica = 12 points`, `1 dida = 1238/1157 points`, `1 cicero = 12 dida`, and `1 scaledpt = 1/65536 point`. A `char` is `fontsize*effective_cex/72` inches. A `line` multiplies that by `lineheight`. String units use the corresponding device metric. On PDF, `strheight("g")` and `strascent("g")` differ. Grob dimensions use the grob’s measurement method. For `grobx` and `groby`, the numeric argument is a **boundary angle in degrees**.

**`grid.layout()`**. Evaluate fixed tracks in the parent viewport. Pure `null` expressions supply relative weights. A mixed expression such as `1 inch + 1 null` resolves as a fixed one-inch track. In ordinary conversion outside layout, a standalone `null` resolves to zero inches. [R grid layout manual](https://stat.ethz.ch/R-manual/R-devel/library/grid/html/grid.layout.html)

- `respect=FALSE` distributes the remaining width and height independently among `null` tracks in proportion to their weights. When fixed tracks exhaust an axis, the unrespected `null` tracks on that axis get zero.
- `respect=TRUE` uses the common scale \(s\) defined above. For a respect matrix, a relative column is respected if any cell in that column is marked. Apply the same test to rows. Allocate respected relative tracks at \(s\). Distribute the remaining space on each axis among its unmarked relative tracks. Fixed tracks retain their converted size.
- Position unused space with `just`. For horizontal justification \(j_x\), the left offset is \(j_x(W-W_{\rm layout})\). Use the analogous bottom offset vertically. The default is `.5` on both axes. Preserve signed intermediate sizes for overfull respected layouts. In an 8 × 4 inch viewport, widths `(10 inches,1 null)`, heights `(1 null,1 null)`, and `respect=TRUE` produced widths `(10,-2)` and heights `(-2,-2)`.

**`gpar` inheritance**. Start with the device’s root parameters. At each viewport, `gTree`, or grob, inherit omitted parameters. Multiply `cex`, `alpha`, and `lex` through the active scopes. An explicitly supplied `col`, `fill`, `lwd`, `lty`, `fontsize`, `fontface`, `fontfamily`, or `lineheight` replaces its inherited value. Draw with `fontsize*cex`, `lwd*lex`, and `color_alpha*alpha`. Restore the previous parameters on scope exit. For example, parent `lwd=2,lex=3` and child `lwd=5,lex=2` give effective `lwd=5,lex=6` and stroke width `30` in device line-width units. [R grid graphical-parameter manual](https://stat.ethz.ch/R-manual/R-devel/library/grid/html/gpar.html)

## 4. Lazy-load environments and bytecode

**`PERSISTSXP`**. The observed XDR payload after tag `247` is a reserved `i32` zero, an `i32` string count, then that many ordinary serialized `CHARSXP` items. The lazy-load hook uses a string such as `env::1`. Maintain a database-wide environment cache. Each independently serialized record has its own ordinary serialization reference table. [R Internals, serialization formats](https://cran.r-project.org/doc/manuals/r-release/R-ints.html#Serialization-Formats)

1. On the first lookup, allocate an empty environment. Root it immediately and insert its handle under the key **before fetching the descriptor**. A recursive lookup returns that same handle.
2. Accept both observed `.rdx$references` forms. An integer `(offset,length)` addresses the descriptor directly. A list with `eagerKey` and named `lazyKeys` addresses the descriptor and separate binding records. A generated `srcfilecopy` used a separate lazy record for `lines`.
3. Decode `bindings`, `enclos`, `attributes`, `isS4`, and `locked`. Install the parent and bindings into the existing handle. Install a fetch promise for each lazy key, using the same database cache. Restore attributes and the S4 state. Apply the environment lock last.
4. For `GcPolicy::Torture`, root provisional environments, partially decoded values, repetition tables, and pending parent or binding values across every allocating operation. Publish the completed fetch only after its dependent records succeed. On failure, discard the provisional cache entries created by that fetch. A previously completed environment can stay cached.

The oracle preserved identity when two environments referenced each other and when a closure captured its own environment. For an ordinary persistent environment, `lockEnvironment(e,TRUE)` round-tripped with `environmentIsLocked(e)==TRUE` and **`bindingIsLocked("x",e)==FALSE`**. An active binding in the fixture came back as an ordinary value. The descriptor contains an environment lock flag. Applying it with `bindings=FALSE` reproduces this result.

**`BCODESXP` wire grammar**. Here `I` is a signed big-endian 32-bit integer. `S` is one complete ordinary R serialization item, including its own header. `Node(k)` consumes the payload after a compact node marker has already been read.

```text
Ordinary bytecode item =
    ordinary header with type 21
    I(repetition table capacity)
    Body

Body =
    S(instruction integer vector)
    I(constant count)
    Constant repeated constant-count times

Constant =
    I(21) Body
  | I(k) Node(k)                      k in {2, 6, 239, 240, 243, 244}
  | I(ordinary type marker) S

Child =
    I(0) S
  | I(k) Node(k)                      k in {2, 6, 239, 240, 243, 244}

Node(243) = I(repetition index)
Node(244) = I(repetition index) I(node kind) Node(node kind)
Node(2)   = S(tag) Child(CAR) Child(CDR)
Node(6)   = S(tag) Child(CAR) Child(CDR)
Node(239) = S(attributes) S(tag) Child(CAR) Child(CDR)
Node(240) = S(attributes) S(tag) Child(CAR) Child(CDR)
```

`2` and `239` represent pairlist nodes. `6` and `240` represent calls. The compact form always writes a tag item, including a `NIL` tag. A `NIL` child is the two-word sequence `0,254`. For `244`, register the rooted node shell at the zero-based index before decoding its fields. For `243`, retrieve the existing handle. Check lengths, indices, depth, and record boundaries before allocation or access.

A direct bytecode constant uses the enclosing repetition table. An ordinary bytecode item reached through a generic vector starts a new table. Both continue to use the enclosing stream’s ordinary reference table. Consume **all** constants before returning to the surrounding decoder. For a compiled closure body or promise expression, retain `consts[0]`. Valid first constants include `NULL`, symbols, literals, and calls.

An independent reader using this grammar consumed exactly the last byte of **14 generated XDR version 2 and 3 fixtures**. They covered attributed calls, attributed pairlists, shared nodes, nested compiled promises, and compiled closures inside generic vectors. This verifies the wire cases above. Arbitrary installed-package compatibility also depends on the ordinary serialization types that `S` can contain. I did not run a package-corpus test or a Rust garbage-collector torture test in this read-only task.
