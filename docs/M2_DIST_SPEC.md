# Roxide M2 W2 distribution specification

The oracle identified itself as **GNU R 4.6.2 (2026-10-29), `aarch64-apple-darwin23`**. I sourced `env/env.sh` before each oracle run. All numerical fixtures below come from the installed binary. I made no repository edits and ran no Cargo commands.

**Clean-room provenance.** I used the mathematical sections of the DiDonato–Morris report for the beta algorithm. During literature searches, search results also displayed source excerpts, including part of the Poisson paper’s listing appendix. I did not use those excerpts to produce implementation instructions. The `rpois` handoff below contains the published method reference and black-box observations. A builder who needs an independent derivation of its rejection tests should use the paper’s mathematical sections.

## W2A · Continuous distributions

### Incomplete beta

Keep both \(x\) and \(y=1-x\) when the caller can supply them independently. This matters for F probabilities and upper-tail inverses. For example, a positive, representable \(y\) can correspond to an \(x\) that rounds to exactly one.

Use the following decision table for positive, finite shapes and interior arguments. A **swap** exchanges \((a,x)\) with \((b,y)\) and exchanges the two result tails. The table follows the authors’ mathematical report, sections II–III. The report includes a **fifth** method, `basym`, for two large shapes near the mean. [DiDonato and Morris, mathematical report, pp. 3–17](https://www.statisticshowto.com/wp-content/uploads/2020/09/pdf.pdf#page=7) · [1992 paper](https://doi.org/10.1145/131766.131776)

| Region after the initial swap | Evaluation |
|---|---|
| \(\min(a,b)\le1\) | First swap if \(x>1/2\). Apply the next three rows in order. |
| Both shapes \(\le1\) | Use `bpser(a,b,x)` if \(a\ge\min(0.2,b)\) or \(x^a\le0.9\). Otherwise evaluate the complementary tail. Use its power series if \(x\ge0.3\). For smaller \(x\), shift its first shape up by 20 and use `bgrat`. |
| Exactly one shape \(>1\) | Use `bpser(a,b,x)` if \(b\le1\), or if \(x<0.1\) and \((bx)^a\le0.7\). Otherwise evaluate the complementary tail. Use its power series if \(x\ge0.3\). |
| Remaining complementary tail | Use `bgrat(b,a,y)` directly for \(b\ge15\). Otherwise add `bup(b,a,y,20)` to `bgrat(b+20,a,y)`. |
| Both shapes \(>1\) | Swap if \(x>a/(a+b)\). Set \(\lambda=ay-bx\). |
| \(b<40,\ bx\le0.7\) | Use `bpser(a,b,x)`. |
| \(b<40,\ bx>0.7\) | Set \(N=\lceil b\rceil-1,\ s=b-N\). Add `bup(s,a,y,N)` to \(I_x(a,s)\). Evaluate the latter by series for \(x\le0.7\). Otherwise use `bgrat`, shifting \(a\) up by 20 with `bup` if \(a<15\). |
| \(b\ge40,\ \min(a,b)>100,\ \lambda\le0.03\min(a,b)\) | Use the two-large-shape expansion `basym`, report equation (16). |
| Remaining \(b\ge40\) | Use the weighted continued fraction `bfrac`, report equation (15). |

The following forms specify the numerical kernels.

- **`bpser`.** Start with \(c_0=1\). Form \(c_n=c_{n-1}x(1-b/n)\). Evaluate \(I_x=x^a[1+a\sum_{n\ge1}c_n/(a+n)]/[aB(a,b)]\). Compute the prefactor in logs. Compare powers in the decision table using their logarithms.
- **`bup`.** This returns \(I_x(a,b)-I_x(a+N,b)\). Sum \(N\) positive terms. The first is \(x^ay^b/[aB(a,b)]\). Multiply term \(j\), starting at \(j=0\), by \(x(a+b+j)/(a+1+j)\) to get the next term. Scale the sum if the first term underflows.
- **`bgrat`.** Set \(T=a+(b-1)/2\) and \(u=-T\log x\). Let \(c_j\) be the coefficient of \(z^{2j}\) in \((\sinh z/z)^{b-1}\). The expansion is
  \[
  I_x(a,b)\sim\frac{1}{T^bB(a,b)}
     \sum_{j\ge0}\frac{c_j\,\Gamma(b+2j,u)}{(2T)^{2j}}.
  \]
  Here \(\Gamma(s,u)\) is the **upper, unnormalized** incomplete gamma function. Initialize from the upper gamma tail. Advance by \(\Gamma(s+1,u)=s\Gamma(s,u)+u^se^{-u}\). Scale before adding a `bup` contribution. [Report, equations (14)–(14.9)](https://www.statisticshowto.com/wp-content/uploads/2020/09/pdf.pdf#page=8)
- **`bfrac`.** Multiply \(x^ay^b/B(a,b)\) by \(1/(\beta_1+\alpha_2/(\beta_2+\cdots))\). Set \(\beta_1=a(1+\lambda)/(a+1)\). For \(n\ge1\),
  \[
  \alpha_{n+1}=
  \frac{(a+n-1)(a+b+n-1)n(b-n)x^2}{(a+2n-1)^2},
  \quad
  \beta_{n+1}=n+\frac{n(b-n)x}{a+2n-1}
   +\frac{a+n}{a+2n+1}[1+\lambda+n(1+y)].
  \]
  Modified Lentz iteration evaluates the denominator. Initialize \(f=C=\beta_1,\ D=0\). At each step set \(D=1/(\beta+\alpha D)\), \(C=\beta+\alpha/C\), and \(f\leftarrow fCD\). Protect zero denominators. Return the log prefactor minus \(\log f\). [Report, equation (15)](https://www.statisticshowto.com/wp-content/uploads/2020/09/pdf.pdf#page=9)
- **`basym`.** Use report equations (16)–(16.6) for the coefficients and stopping test. Its leading normal term alone does not provide double precision. Compute the exponential factor from the deviations about \(a/(a+b)\) and \(b/(a+b)\). Direct subtraction of three large `lgamma` values loses precision here. [Report, equation (16)](https://www.statisticshowto.com/wp-content/uploads/2020/09/pdf.pdf#page=11) · [NIST DLMF §8.18](https://dlmf.nist.gov/8.18)

For log output, retain the computed log tail \(L\). Compute its log complement with `log1p(-exp(L))` for \(L<-\log 2\), and `log(-expm1(L))` otherwise. For a raw complement use `-expm1(L)`. A raw probability that has already rounded to zero or one cannot recover the other log tail.

### Incomplete gamma and both inverse functions

For `pgamma`, set \(z=x/\mathrm{scale}\). The following is a clean-room numerical design. The R manual describes its own `pgamma` algorithm as unpublished. Its internal switching thresholds cannot be inferred from matching outputs. [R gamma manual](https://stat.ethz.ch/R-manual/R-devel/library/stats/html/GammaDist.html)

| Region | Calculation |
|---|---|
| \(z<a+1\) | Lower series. Start with \(t=S=1\). For \(n=1,2,\ldots\), multiply \(t\) by \(z/(a+n)\) and add it to \(S\). Then \(\log P=a\log z-z-\log\Gamma(a+1)+\log S\). |
| \(z\ge a+1\) | Upper continued fraction. Use \(b_0=z+1-a,\ b_n=b_0+2n,\ \alpha_n=n(a-n)\). For denominator \(D=b_0+\alpha_1/(b_1+\cdots)\), \(\log Q=a\log z-z-\log\Gamma(a)-\log D\). |
| Very small \(a\), small \(z\), requested upper tail | Avoid taking the complement of a rounded lower series. Set \(h=a\log z-\log\Gamma(1+a)\). Then \(Q=-\operatorname{expm1}(h)-e^h\sum_{n\ge1}(-z)^na/[n!(a+n)]\). Use a `lgamma1p` kernel for \(h\). |
| Large \(a\), \(z\) near \(a\) | Use Temme’s uniform expansion. A proposed initial dispatch is \(a\ge100,\ |z/a-1|\le0.25\). With \(d=(z-a)/a\), set \(\eta=\operatorname{sign}(d)\sqrt{2(d-\log(1+d))}\). Evaluate the difference by series near zero. The upper tail starts with \(\tfrac12\operatorname{erfc}(\eta\sqrt{a/2})\). Add the correction series in DLMF §8.12. Its first coefficient is \(1/d-1/\eta\), with limit \(-1/3\). Use the lower-tail form directly on the other side. |

Use a scaled complementary error function for extreme log tails. Terminate a convergent series using a bound on the remainder. For a positive series with decreasing term ratio \(r<1\), the remaining sum is at most the next term divided by \(1-r\). For the Temme expansion, use its small-\(\eta\) coefficient expansions and fall back if the terms increase before reaching the target accuracy. [NIST DLMF §8.12](https://dlmf.nist.gov/8.12)

**`qbeta` and `qgamma`.** Keep the target as a log probability \(\ell\). For beta, use symmetry to invert a small tail and preserve both coordinates. A lower-tail initial value is \(\log x_0=[\ell+\log a+\log B(a,b)]/a\). For a central probability and two large shapes, start at the beta mean plus a normal quantile times the beta standard deviation. For gamma, the lower power-law seed is \(\log z_0=[\ell+\log\Gamma(a+1)]/a\). In the central region use the Wilson–Hilferty seed \(a[1-1/(9a)+\Phi^{-1}(p)/(3\sqrt a)]^3\) when it is positive. For a far upper gamma tail, start by solving \(z-(a-1)\log z=-\ell-\log\Gamma(a)\). Multiply by `scale` after inversion.

For either inverse, bracket the root of \(g=\log T-\ell\), where \(T\) is the selected tail. With density \(f\), \(g'=\pm\exp(\log f-\log T)\), positive for a lower tail. Then \(g''=g'[(\log f)'-g']\). The log-density derivative is \((a-1)/x-(b-1)/(1-x)\) for beta and \((a-1)/z-1\) for unit-scale gamma. Try the Halley step \(2gg'/(2g'^2-gg'')\). Use a Newton step or bracket subdivision if the proposal leaves the bracket. Use `log(x)` or log odds as the search coordinate near an endpoint. Stop when the residual meets the probability tolerance or the bracket contains adjacent floating-point values.

**Extreme-tail fixtures.** Input `lp = log(1e-300)` printed as `-690.77552789821368`. Each row used `log.p=TRUE` for both calls.

| Parameters | Tail | Quantile | Log probability at returned quantile |
|---|---|---:|---:|
| Gamma `shape=2.5, rate=1.5` | Lower | `1.0778025935277289e-120` | `-690.77552789821368` |
| Same | Upper | `466.88019521767563` | `-690.7755278982138` |
| Beta `2,3` | Lower | `4.0824829046386795e-151` | `-690.77552789821368` |
| Same | Upper | `1` | `-Inf` |
| Beta `2,200` | Upper | `0.96919993666614423` | `-690.77552789821368` |

The last two rows test different requirements. One checks rounding to the support endpoint. The other checks a representable upper-tail inverse at the same probability.

### Distribution definitions and numerical fixtures

R exposes noncentral distributions through `ncp` on the usual function names. For example, call `pchisq(x,df,ncp=λ)` and `pt(x,df,ncp=δ)`. Preserve whether the caller supplied `ncp`. Explicit `ncp=0` can select different boundary behavior.

The table uses \(x=0.5\). Columns give the density, lower cumulative distribution function (CDF), and lower 0.75 quantile. Values use `sprintf("%.17g", ...)`.

| R family and parameters in argument order | Density | CDF | Quantile |
|---|---:|---:|---:|
| `unif(-2,3)` | `0.20000000000000001` | `0.5` | `1.75` |
| `exp(rate=1.5)` | `0.70854982911152209` | `0.52763344725898531` | `0.92419624074659368` |
| `gamma(shape=2.5,rate=1.5)` | `0.34619922631227434` | `0.086930185455604533` | `2.2085599212764171` |
| `beta(2,3)` | `1.5000000000000004` | `0.6875` | `0.54367828541908025` |
| `t(df=5)` | `0.32791853132274656` | `0.68085056417953549` | `0.72668684380042292` |
| `f(df1=4,df2=9)` | `0.66329088497329536` | `0.26301012780744953` | `1.6252977982628947` |
| `chisq(df=5)` | `0.036615940788976876` | `0.0078767067673704057` | `6.6256797638292513` |
| `cauchy(location=0,scale=2)` | `0.14979288761590148` | `0.57797913037736937` | `2` |
| `logis(location=0,scale=2)` | `0.12306704136879916` | `0.56217650088579807` | `2.1972245773362196` |
| `lnorm(meanlog=0,sdlog=.5)` | `0.61045530419018323` | `0.08282851900169852` | `1.4010821118543542` |
| `weibull(shape=1.5,scale=2)` | `0.33093633846922332` | `0.1175030974154046` | `2.486567769750343` |
| `chisq(5,ncp=2)` | `0.01486628615295185` | `0.003106989597270896` | `9.2637958944914942` |
| `t(5,ncp=2)` | `0.12971333983184069` | `0.065999808390747317` | `3.0968891239656671` |
| `f(4,9,ncp=2)` | `0.42557880427719419` | `0.1390022971483236` | `2.4425276868975891` |
| `beta(2,3,ncp=2)` | `1.7106060012207864` | `0.51649876421994356` | `0.64188180991684995` |

For implementation, the following reductions fix the parameter conventions.

- **Exponential, Weibull, logistic, Cauchy, lognormal.** The exponential log survival is \(-rx\). The Weibull log survival is \(-(x/s)^a\). For logistic and Cauchy, standardize \(z=(x-m)/s\). Their CDFs are \(1/(1+e^{-z})\) and \(1/2+\arctan(z)/\pi\). Use the reciprocal-argument arctangent for a small Cauchy tail. Lognormal uses the normal distribution of \(\log x\), with density Jacobian \(1/x\). Uniform uses the interval `[min,max]`, including both endpoints for density.
- **Central chi-square, t, F.** Chi-square is gamma with shape \(\nu/2\) and scale 2. The smaller central t tail is \(\tfrac12 I_{\nu/(\nu+x^2)}(\nu/2,1/2)\). The sign of \(x\) selects the tail. For F, evaluate the beta CDF at \(u=d_1x/(d_2+d_1x)\) with shapes \(d_1/2,d_2/2\). Compute \(1-u\) independently. The density Jacobian is \(d_1d_2/(d_2+d_1x)^2\).
- **Noncentral chi-square, beta, F.** For \(J\sim\operatorname{Pois}(\lambda/2)\), average central chi-square with `df+2J`, or beta with first shape `a+J`. Apply the F transformation above for noncentral F. For large \(\lambda\), start at the Poisson mode and sum in both directions. Evaluate the requested component tail directly. For `df=0`, the \(J=0\) component is an atom at zero. [R beta manual](https://stat.ethz.ch/R-manual/R-devel/library/stats/html/Beta.html)
- **Noncentral t.** With \(V\sim\chi^2_\nu\), let \(s=\sqrt{V/\nu}\). The lower CDF is \(E[\Phi(xs-\delta)]\), the upper CDF is \(E[\Phi(\delta-xs)]\), and the density is \(E[s\phi(xs-\delta)]\). These give a clean-room quadrature reference for either sign of \(\delta\). Invert with a bracket. At `df=Inf`, use a unit normal centered at `ncp` for `d/p/q`.
- **Studentized range.** For \(k=\texttt{nmeans}\), define \(H(w)=k\int\phi(t)[\Phi(t+w)-\Phi(t)]^{k-1}\,dt\). For finite `df`, the CDF is \(E[H(q\sqrt{V/\mathrm{df}})^{\texttt{nranges}}]\). The power belongs **inside** the expectation because the ranges share the denominator. With `df=Inf`, use \(H(q)^{\texttt{nranges}}\). R documents 16-point Legendre quadrature and about four decimal places of quantile accuracy. The oracle gave `ptukey(3,4,10) = 0.78888907595259672`, its log upper tail `-1.5553715772554648`, and `qtukey(.75,4,10) = 2.8307455440805644`. [R studentized-range manual](https://stat.ethz.ch/R-manual/R-devel/library/stats/html/Tukey.html)

### Boundary behavior

For valid regular continuous parameters, lower quantiles at raw `p=c(0,1)` or log `p=c(-Inf,0)` return the two support endpoints. Upper-tail quantiles reverse them. The endpoints are `[min,max]` for uniform, `[0,1]` for beta, `[-Inf,Inf]` for t, Cauchy, and logistic, and `[0,Inf]` for the other families above.

For positive-support regular families, negative \(x\) gives density and lower CDF zero. At zero the lower CDF is zero. Gamma and Weibull densities at zero are `Inf`, `1/scale`, or `0` according as shape is below, equal to, or above one. Central chi-square gives `Inf`, `0.5`, or `0` at the corresponding `df=2` threshold. Central F gives `Inf`, `1`, or `0` at `df1=2`. Beta gives `Inf`, `b`, or `0` at zero according to `a`, with the reflected rule at one. Lognormal density at zero is zero. The regular lower CDF at `Inf` is one.

| Case | Observed exceptions that affect dispatch |
|---|---|
| Exponential `rate=0` | At finite \(x\ge0\), density and CDF are zero. At `x=Inf`, both are `NaN`. For `p=c(0,.5,1)`, quantiles are `0,Inf,Inf`. `rexp` returns `NaN`. |
| Gamma `rate=0` | For `shape=2`, density at `Inf` is zero and CDF there is `NaN`. At positive finite \(x\), both are zero. At zero the density is `Inf` for `shape=.5`. For positive shape, `rgamma` returns `Inf` without advancing the generator. Supplying both `rate` and `scale` raises an R-level error. |
| Gamma `shape=0`, central chi-square `df=0` | Density at zero is `Inf`. Lower CDF at zero is **zero**, then one for positive \(x\). Interior quantiles and random draws are zero. The lower quantile at one is `Inf`. |
| Chi-square `df=0,ncp=2` | `pchisq(0,0,2) = 0.36787944117144233`. The oracle gives `qchisq(.25,0,2) = 1.4930612143579066e-308` despite the atom. Explicit `ncp=0` gives `pchisq(0,0,0) = 1` and `qchisq(.5,0,0) = NaN`. |
| Central chi-square `df=Inf` | At `x=c(-1,0,1,Inf)`, density is `0,0,0,0` and CDF is `0,0,NaN,1`. At `p=c(0,.5,1)`, quantiles are `0,Inf,Inf`. Explicit `ncp=2` gave `NaN` throughout these probes. |
| t and F degrees of freedom | Central `qt(c(0,.5,1),0)` gives `-Inf,NaN,Inf`. With `ncp=2`, all three are `NaN`. Zero numerator degrees of freedom gave `NaN` for F `d/p/q`. With both degrees infinite, `df(1,Inf,Inf)=Inf`, `pf(1,Inf,Inf)=.5`, and interior central quantiles equal one. |
| Noncentral F | `df(Inf,4,9,ncp=2)` gives `NaN` with a warning. `pf(Inf,4,9,ncp=2)` gives one. With `df1=Inf,df2=9,ncp=2`, both `pf(1,...)` and `qf(.5,...)` give `NaN`. |
| Degenerate central beta | `pbeta(-1,0,2)=1`. For both shapes zero, the lower CDF is `.5` even at `-Inf`, and is one at \(x=1\). `qbeta(c(.25,.5,.75),0,0)` gives `0,.5,1`. For both shapes infinite, density at `.5` is `Inf` and the CDF there is one. |
| Degenerate location and scale | For uniform `(1,1)`, density is `NaN`, CDF steps at one, and all quantiles equal one. For zero-scale Cauchy and logistic, density and CDF are `NaN`. All Cauchy quantiles equal the location. Logistic retains infinite endpoint quantiles. For lognormal `sdlog=0`, the density is infinite at `exp(meanlog)` and the CDF steps there. |
| Studentized range | Positive \(q\) requires `df>=2`, `nmeans>=2`, `nranges>=1`. Fractional `nmeans=2.5,nranges=1.5` was accepted. `ptukey(0,1,0)` returns zero. `qtukey` with invalid parameters returned `NaN` even at endpoint probabilities. |

**Upper-tail compatibility.** For `lower.tail=FALSE,log.p=TRUE`, the oracle gave `-20.907656596778793` for both `pbeta(1-1e-12,2,3,ncp=2)` and `pf(1e10,4,9,ncp=2)`. It gave `-28.482531799586919` for `pt(1e6,5,ncp=2)`. Preserve these as **oracle compatibility fixtures**. An independently evaluated small-tail integral can be much more accurate than these values.

### Gamma random generation

Use the configured uniform, standard-normal, and standard-exponential generators. `rexp(1)` below means the unit exponential generator used by R. Replacing it with a single `-log(U)` changes the stream. The R manual identifies Ahrens–Dieter GS below shape one and GD at and above one. [R gamma manual](https://stat.ethz.ch/R-manual/R-devel/library/stats/html/GammaDist.html) · [GS paper](https://doi.org/10.1007/BF02293108) · [GD paper](https://doi.org/10.1145/358315.358390)

**GS, \(0<a<1\).** Set \(b=1+a/e\). Draw \(p=bU\). For \(p<1\), propose \(x=\exp(\log(p)/a)\) and accept if a fresh unit exponential is at least \(x\). Otherwise propose \(x=-\log((b-p)/a)\) and accept if a fresh unit exponential is at least \((1-a)\log x\). Repeat after rejection. Multiply the accepted value by scale. At `a=.25`, the independent calculation matched all 500 oracle outputs and the complete final `.Random.seed`.

**GD, \(a\ge1\).** Set \(v=a-.5,\ s=\sqrt v,\ d=\sqrt{32}-12s\). Draw a standard normal \(t\) and set \(x=s+t/2\). Accept \(x^2\) if \(t\ge0\). Otherwise draw \(U\) and accept if \(dU\le t^3\). Next, for \(x>0\), accept if \(\log(1-U)\le Q(t)\), where
\[
Q(t)=q_0-st+t^2/4+2v\log(1+t/(2s)),\qquad
q_0=\tfrac12\log(2\pi)+v\log v-v-\log\Gamma(a).
\]
Use a Stirling remainder for \(q_0\) at large \(a\). Use `log1p` and a local series when cancellation affects \(Q\).

For the remaining proposals, use the following constants.

| Shape | \(b\) | \(\sigma\) | \(c\) |
|---|---:|---:|---:|
| \(a\le3.686\) | \(0.463+s+0.178v\) | \(1.235\) | \(0.195/s-0.079+0.16s\) |
| \(3.686<a\le13.022\) | \(1.654+0.0076v\) | \(1.68/s+0.275\) | \(0.062/s+0.024\) |
| \(a>13.022\) | \(1.77\) | \(0.75\) | \(0.1515/s\) |

Draw \(E\) from the unit exponential, then \(u=2U-1\). Set \(t=b+\operatorname{sign}(u)\sigma E\). Retry if \(t<-0.71874483771719\). Accept \((s+t/2)^2\) if \(Q(t)>0\) and \(c|u|\le\operatorname{expm1}(Q(t))\exp(E-t^2/2)\). Otherwise repeat this proposal.

On the arm64 oracle, using fused multiply-add for the product in \(b\) and for \(b+\operatorname{sign}(u)\sigma E\) matched **5,000 consecutive draws at each of shapes `1,2.5,4,15,60`**, including every final state. Separate multiplication and addition preserved those final states but changed 73 returned doubles across the five runs. This establishes the tested streams. Rare rejection boundaries and very large shapes remain unverified.

## W2B · Discrete distributions

### Quantile decisions

Let \(\epsilon=2^{-52}\), \(F(k)=P(X\le k)\), and \(S(k)=P(X>k)\). Apply parameter and probability endpoint rules before the interior search. The inequalities below matched boundary probes on the installed oracle.

| Family | Interior rule |
|---|---|
| `qbinom`, `qpois`, `qnbinom`, raw lower | First \(k\) with \(F(k)\ge p(1-8\epsilon)\). |
| Same, raw upper | First \(k\) with \(S(k)<p(1+8\epsilon)\). The strict comparison matters. |
| Same, log lower | First \(k\) with \(\log F(k)\ge\ell(1+2\epsilon)\). |
| Same, log upper | First \(k\) with \(\log S(k)<\ell(1-2\epsilon)\). |
| `qgeom` | With success probability \(g\) and requested probability converted to log survival \(L\), return \(\max(0,\lceil L/\log(1-g)-1-10^{-12}\rceil)\). The adjustment is in **count units**. This matched all four tail/log combinations over four success probabilities and six jump locations. |
| `qhyper` | Convert to raw lower probability and multiply by \(1-1000\epsilon\). Search the internally accumulated mass. Its rounding can differ from `phyper(k,...)`. |
| `qwilcox`, `qsignrank` | Use an **absolute** adjustment of \(10\epsilon\) on the raw probability scale. For lower probability \(\le.5\), accumulate from zero until the sum is at least \(p-10\epsilon\). For \(p>.5\), accumulate the reflected tail until its sum strictly exceeds \(1-p+10\epsilon\), then reflect the index about the support maximum. |

For `qhyper`, the following initialization reproduced transitions that direct calls to `phyper` missed. Let \(l=\max(0,k-n)\) and \(h=\min(k,m)\). Define
\[
C(N,j)=-\log(N+1)-\operatorname{lbeta}(N-j+1,j+1).
\]
Start the mass at \(\exp[C(m,l)+C(n,k-l)-C(m+n,k)]\). Advance from \(j\) to \(j+1\) with ratio \((m-j)(k-j)/[(j+1)(n-k+j+1)]\). Use log masses when the starting value underflows. The ordinary recurrence matched the measured transitions for `(m,n,k)=(6,9,5),(5,7,9),(5,4,2),(3,3,1)`. For `(1,1,1)`, the last tested probability returning zero was `.5*(1+1001*eps)`. The next representable probability returned one.

**Jump fixtures.** With the parameters below and \(p=F(2)\), all seven quantiles gave `2,2,3` at `p*(1-1e-12)`, `p`, and `p*(1+1e-12)`. That three-point test alone cannot distinguish the adjustments. For `qbinom` with `size=1,prob=.5`, `.5*(1+8*eps)` returned zero and `.5*(1+9*eps)` returned one. For `qsignrank` with `n=1`, `.5+10*eps` returned zero and `.5+11*eps` returned one.

| Family | Mass at 2 | \(F(2)\) | \(\log S(2)\) | \(q(.75)\) | Upper quantile at `log.p=-1000` |
|---|---:|---:|---:|---:|---:|
| `binom(8,.25)` | `0.31146240234374994` | `0.67854309082031228` | `-1.1348917748326539` | 3 | 8 |
| `pois(3)` | `0.22404180765538773` | `0.42319008112684348` | `-0.55024249677722104` | 4 | 280 |
| `nbinom(2,.4)` | `0.17280000000000001` | `0.52479999999999993` | `-0.74401951093370178` | 4 | 1970 |
| `geom(.3)` | `0.14700000000000002` | `0.65699999999999992` | `-1.0700248318161969` | 3 | 2803 |
| `hyper(6,9,5)` | `0.41958041958041953` | `0.71328671328671323` | `-1.2492725635555995` | 3 | 5 |
| `wilcox(3,4)` | `0.057142857142857141` | `0.11428571428571428` | `-0.12136085700426744` | 8 | 12 |
| `signrank(4)` | `0.0625` | `0.1875` | `-0.20763936477824449` | 7 | 10 |

For the same parameters, at `x=100` the log upper tails of Poisson, negative binomial, and geometric were `-260.36481085848794`, `-47.870107119533792`, and `-36.024169337811969`.

**Endpoint exception.** `qhyper`, `qwilcox`, and `qsignrank` returned **`NaN` with a warning** at `p=-Inf,log.p=TRUE`, for either `lower.tail` setting. At log `p=0` they returned the appropriate support endpoint. Raw `p=0` and finite log `p=-1000` also produced valid results for these parameters. A common log-probability endpoint handler would change this behavior.

### Masses, support, and parameter validation

- **Binomial, Poisson, negative binomial, geometric.** Use Loader’s deviance and Stirling correction for binomial and Poisson masses. For interior integer \(k\), the binomial CDF is \(I_{1-g}(n-k,k+1)\), Poisson CDF is \(Q(k+1,\lambda)\), and negative binomial CDF is \(I_g(s,k+1)\). Negative binomial counts failures before `size=s` successes, with \(g=s/(s+\mu)\). Geometric counts failures before the first success and has \(\log S(k)=(k+1)\log(1-g)\). [Loader’s mathematical report](https://www.r-project.org/doc/reports/CLoader-dbinom-2002.pdf) · [R binomial manual](https://stat.ethz.ch/R-manual/R-devel/library/stats/html/Binomial.html)
- **Rank distributions.** Signed-rank masses are the coefficients of \(\prod_{i=1}^n(1+z^i)/2^n\), with support `0..n*(n+1)/2`. Wilcoxon masses count choices of \(n\) distinct ranks from `1..m+n`. Subtract \(n(n+1)/2\) from their sum. Divide counts by \(\binom{m+n}{n}\). Its support is `0..m*n`. Both distributions are symmetric.
- **Noninteger observations.** At `x=2.25`, the first five mass functions in the fixture table returned zero and warned `non-integer x = 2.250000`. The two rank mass functions returned zero silently. All seven accepted `2+1.9e-9` as two. The measured acceptance boundary for `dpois` agrees with a tolerance of \(10^{-9}\max(1,|x|)\) about the nearest integer. CDF flooring has different behavior. For example, `pbinom(2-1e-8,8,.25)` still equals `pbinom(2,8,.25)`.
- **Degenerate binomial and Poisson.** Binomial `size=0` and Poisson `lambda=0` give a point mass at zero. For `size=4,prob=0`, `qbinom(1,4,0)=4`. For `size=4,prob=1`, `qbinom(0,4,1)=0`. Interior probabilities give the atom. `qpois(1,0)=0`.
- **Negative binomial and geometric.** Negative binomial `size=0,prob=.4` gives a point mass at zero for `d/p/q`. `rnbinom(1,0,.4)` gives integer `NA` and preserves the generator state. With `size=0,mu=1`, random output is double `NaN`. Positive size with `prob=1` or `mu=0` gives zero. `size=Inf,mu=1` agrees with Poisson(1) in the probes. `prob=0` is invalid for both families. Geometric `prob=1` gives zero, but **`rgeom` still consumes a unit exponential**.
- **Hypergeometric.** Support is \([\max(0,k-n),\min(k,m)]\). Mass outside it is zero. Fractional population counts require function-specific handling. `dhyper(1,1.5,2,2)` returned `NaN`. `phyper(1,1.5,2,2)` returned `0.83333333333333315`. `qhyper(.5,1.5,2,2)` returned one. [R hypergeometric manual](https://stat.ethz.ch/R-manual/R-devel/library/stats/html/Hypergeometric.html)
- **Zero rank sample size.** `dwilcox`, `pwilcox`, and `qwilcox` gave `NaN` for `m=0,n=2`. Their signed-rank counterparts gave `NaN` for `n=0`. The corresponding random calls returned zero.

### Random generation and state fixtures

Each row resets `set.seed(42)` with `RNGkind("Mersenne-Twister","Inversion","Rejection")`. The last column counts equivalent consecutive `runif(1)` calls to reach the observed next uniform.

| Call | Returned values in order | Uniform draws consumed |
|---|---|---:|
| `rgamma(3,.5)` | `0.76051683481073429, 0.29369752276972355, 1.8663435236133064` | 14 |
| `rgamma(3,1)` | `1.9392957806530899, 0.18041909987670399, 0.53443285859527778` | 7 |
| `rgamma(3,2.5)` | `4.4087097953554784, 1.2811171935588865, 2.0682924802028513` | 7 |
| `rbinom(3,59,.5)` | `35,35,27` | 3 |
| `rbinom(3,60,.5)` | `29,34,40` | 14 |
| `rpois(3,9.99)` | `14,15,8` | 3 |
| `rpois(3,10)` | `14,8,11` | 6 |
| `rnbinom(3,2,.4)` | `4,4,4` | 9 |
| `rgeom(3,.3)` | `1,1,0` | 8 |
| `rwilcox(3,3,4)` | `9,8,7` | 16 |
| `rsignrank(3,4)` | `7,6,7` | 12 |
| `rmultinom(1,10,c(2,3,5))` | `4,4,2` | 2 |
| `rhyper(3,1.5e9,1e9,1)` | `0,0,1` | 3 |
| `rhyper(3,1.5e9,1e9,5)` | `1,1,4` | 3 |

Implementation rules for these streams follow.

1. **`rbinom`.** Handle `n=0`, `p=0`, and `p=1` first. For `n < 2147483647`, set \(r=\min(p,1-p)\). For \(nr<30\), invert from mass \((1-r)^n\), multiplying successive masses by \((n-j)r/[(j+1)(1-r)]\). Reflect the result if \(p>1/2\). At \(nr\ge30\), use the published BTPE triangle, parallelogram, and two exponential proposal regions. Each proposal uses two uniforms. Preserve the immediate triangle acceptance and the rejection path. For `n=2147483647,p=.2`, ten draws and the final state matched `qbinom(runif(10),n,.2,lower.tail=FALSE)`. [Kachitvichyanukul and Schmeiser, 1988](https://doi.org/10.1145/42372.42381)
2. **`rpois`.** Zero mean returns zero. The Ahrens–Dieter paper specifies inversion below 10 and a modified normal proposal with rejection at and above 10. The fixtures discriminate the branches. The detailed rejection implementation requires an independent reading of the mathematical derivation. [Ahrens and Dieter, 1982](https://doi.org/10.1145/355993.355997) · [R Poisson manual](https://stat.ethz.ch/R-manual/R-devel/library/stats/html/Poisson.html)
3. **`rnbinom` and `rgeom`.** Generate each mixture in draw order. For negative binomial, draw \(G\) from gamma with shape `size` and scale `(1-prob)/prob`, then draw Poisson(\(G\)). For geometric, draw a unit exponential \(E\), then Poisson with mean \(E(1-p)/p\). Scalar interleaving matched all 100 outputs and the full final state for each tested family. Generating all mixture means first changes the stream.
4. **`rwilcox` and `rsignrank`.** For Wilcoxon, sample **`n` ranks** without replacement using the configured `sample.kind`, sum them, and subtract `n*(n+1)/2`. This matched 200 outputs and final states for each of `m=3,5,8` with `n=4`. For signed rank, draw one uniform per rank `i=1..n` and include rank `i` when the uniform rounds to one. The `U>.5` comparison matched the sampled stream. An exact `U=.5` tie was not forced.
5. **`rmultinom`.** Normalize nonnegative weights with positive sum. In input order, draw each count from a binomial with the remaining size and the category’s share of the remaining weight. Assign the residual to the last category. The conditional-binomial construction matched five columns and the final state for weights `(2,3,5)`. Zero size with valid weights returned all zeros and preserved state. All-zero weights raised `no positive probabilities`.
6. **`rhyper`.** Form the population sum without signed 32-bit overflow. At `N>2147483647`, `k=1` matched Bernoulli generation through `rbinom`. For `k=5`, it matched upper-tail `qhyper` applied to one uniform per draw. Each comparison covered 100 outputs and the full final state. To test the **switch itself**, use `k=30`. At `N=2147483647`, with `m=floor(.6*N)` and seed 19, 86 of 100 draws differed from fallback inversion and the final states differed. At `N=2147483648`, all outputs and the final state matched. Small `k` can give identical streams on both sides of the switch. [Existing Roxide specification](/Users/devg/Documents/Roxide/docs/MATH_SPEC.md:31)

For `m=1.5e9,n=1e9,k=5`, `dhyper(c(-1,0,5,6),...)` gave `0, 0.010239999938559997, 0.077759999792639992, 0`. The corresponding CDF was `0, 0.010239999938559997, 1, 1`. The quantiles at `c(0,.5,1)` were `0,3,5`.

**Verification limit.** The fixtures establish observable behavior on the installed arm64 oracle. The gamma sampler and several mixture generators also passed independent stream comparisons. The continuous CDF and inverse blueprints still need differential testing in the Rust implementation, particularly at beta region boundaries, at very large shapes, and in noncentral upper tails.
