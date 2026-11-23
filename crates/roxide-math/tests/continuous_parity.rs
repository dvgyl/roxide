//! Check continuous distribution kernels against the configured GNU R binary.

use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Command, Stdio};

use roxide_math::core::{Flags, Tail};
use roxide_math::dist::continuous::*;
use roxide_math::rng::{Kinds, Rng};

const INF: f64 = f64::INFINITY;
const LOWER: Tail = Tail {
    lower: true,
    log: true,
};
const UPPER: Tail = Tail {
    lower: false,
    log: true,
};

fn oracle(script: &str) -> Option<Vec<f64>> {
    let Some(path) = std::env::var_os("ROXIDE_ORACLE_RSCRIPT") else {
        eprintln!("Set ROXIDE_ORACLE_RSCRIPT to run the live comparisons.");
        return None;
    };
    let mut child = Command::new(path)
        .args(["--vanilla", "/dev/stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let output = std::thread::scope(|scope| {
        let writer = scope.spawn(move || {
            stdin.write_all(b"options(warn=-1)\n").unwrap();
            stdin.write_all(script.as_bytes()).unwrap();
        });
        let output = child.wait_with_output().unwrap();
        writer.join().unwrap();
        output
    });
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Some(
        String::from_utf8(output.stdout)
            .unwrap()
            .split_whitespace()
            .map(|word| word.parse::<f64>().unwrap())
            .collect(),
    )
}

fn literal(x: f64) -> String {
    if x.is_infinite() {
        return if x > 0.0 { "Inf" } else { "-Inf" }.to_owned();
    }
    let bits = x.to_bits();
    let sign = if x.is_sign_negative() { "-" } else { "" };
    let fraction = bits & 0x000f_ffff_ffff_ffff;
    let exponent = ((bits >> 52) & 0x7ff) as i32;
    if exponent == 0 {
        format!("{sign}0x0.{fraction:013x}p-1022")
    } else {
        format!("{sign}0x1.{fraction:013x}p{}", exponent - 1023)
    }
}

fn relative_error(found: f64, expected: f64) -> f64 {
    if found == expected || (found.is_nan() && expected.is_nan()) {
        0.0
    } else if !found.is_finite() || !expected.is_finite() || expected == 0.0 {
        INF
    } else {
        ((found - expected) / expected).abs()
    }
}

struct Case {
    family: &'static str,
    expression: String,
    actual: f64,
    tolerance: f64,
}

#[test]
fn densities_probabilities_and_quantiles() {
    let mut cases = Vec::new();
    let lp = 1e-300_f64.ln();
    macro_rules! grid {
        ($family:literal, $r:literal, $args:expr, [$($a:expr),+];
            $d:ident, $p:ident, $q:ident; $xs:expr, $lps:expr, $tol:expr, $extreme:expr) => {{
            let mut flags = Flags::default();
            for x in $xs {
                for log in [false, true] {
                    cases.push(Case {
                        family: $family, expression: format!("d{}({}, {},log={})",
                            $r, literal(x), $args, if log { "TRUE" } else { "FALSE" }),
                        actual: $d(x, $($a),+, log, &mut flags), tolerance: $tol[0],
                    });
                }
            }
            for log in [false, true] {
                for lower in [false, true] {
                    let lower_r = if lower { "TRUE" } else { "FALSE" };
                    let log_r = if log { "TRUE" } else { "FALSE" };
                    let tail = Tail { lower, log };
                    for x in $xs {
                        cases.push(Case {
                            family: $family, expression: format!("p{}({}, {},lower.tail={},log.p={})",
                                $r, literal(x), $args, lower_r, log_r),
                            actual: $p(x, $($a),+, tail, &mut flags), tolerance: $tol[1],
                        });
                    }
                    for prob in [0.0, 0.125, 0.25, 0.5, 0.75, 0.875, 1.0] {
                        let prob = if log { f64::ln(prob) } else { prob };
                        cases.push(Case {
                            family: $family, expression: format!("q{}({}, {},lower.tail={},log.p={})",
                                $r, literal(prob), $args, lower_r, log_r),
                            actual: $q(prob, $($a),+, tail, &mut flags), tolerance: $tol[2],
                        });
                    }
                    if log {
                        for prob in $lps {
                            cases.push(Case {
                                family: $family, expression: format!("q{}({}, {},lower.tail={},log.p=TRUE)",
                                    $r, literal(prob), $args, lower_r),
                                actual: $q(prob, $($a),+, tail, &mut flags), tolerance: $extreme,
                            });
                        }
                    }
                }
            }
        }};
    }
    let elementary = 1e-14;
    let central = 1e-12;
    let strict = [elementary; 3];
    let iterative = [central; 3];
    // The oracle uses approximations with distinct error bounds in these regions.
    let extreme_t = 2e-9;
    let extreme_beta = 3e-10;
    let tukey_tolerance = 3e-7;
    let heavy = [-INF, -1.0, 0.0, 0.5, 2.0, 25.0, INF];
    let positive = [-INF, 0.0, 0.125, 0.5, 2.0, 25.0, INF];
    let unit = [-INF, 0.0, 0.125, 0.5, 0.875, 1.0, INF];
    grid!("uniform", "unif", "-2,3", [-2.0, 3.0];
        dunif, punif, qunif; [-INF, -2.0, -1.0, 0.5, 2.0, 3.0, INF], [lp], strict, elementary);
    grid!("exponential", "exp", "1.5", [1.5];
        dexp, pexp, qexp; positive, [lp], strict, elementary);
    grid!("lognormal", "lnorm", "0,.5", [0.0, 0.5];
        dlnorm, plnorm, qlnorm; positive, [lp], strict, elementary);
    grid!("logistic", "logis", "0,2", [0.0, 2.0];
        dlogis, plogis, qlogis; heavy, [lp], strict, elementary);
    grid!("Cauchy", "cauchy", "0,2", [0.0, 2.0];
        dcauchy, pcauchy, qcauchy; heavy, [lp], strict, elementary);
    grid!("Weibull", "weibull", "1.5,2", [1.5, 2.0];
        dweibull, pweibull, qweibull; positive, [lp], strict, elementary);
    grid!("gamma", "gamma", "2.5,scale=1/1.5", [2.5, 1.0 / 1.5];
        dgamma, pgamma, qgamma; positive, [lp], iterative, central);
    grid!("gamma", "gamma", "1e-12,scale=1", [1e-12, 1.0];
        dgamma, pgamma, qgamma; positive, [lp], iterative, central);
    grid!("gamma", "gamma", "1e7,scale=1", [1e7, 1.0];
        dgamma, pgamma, qgamma; [9_900_000.0, 9_999_000.0, 1e7, 10_001_000.0, 10_100_000.0], [lp], iterative, central);
    grid!("beta", "beta", "2,3", [2.0, 3.0];
        dbeta, pbeta, qbeta; unit, [lp], iterative, central);
    grid!("beta", "beta", "2,200", [2.0, 200.0];
        dbeta, pbeta, qbeta; unit, [lp], iterative, central);
    grid!("beta", "beta", ".1,.2", [0.1, 0.2];
        dbeta, pbeta, qbeta; unit, [lp], iterative, central);
    grid!("beta", "beta", ".4,16", [0.4, 16.0];
        dbeta, pbeta, qbeta; unit, [lp], iterative, central);
    grid!("beta", "beta", "8,10000", [8.0, 10_000.0];
        dbeta, pbeta, qbeta; [0.0001, 0.0008, 0.001, 0.005], [lp], iterative, extreme_beta);
    grid!("beta", "beta", "1000,2000", [1000.0, 2000.0];
        dbeta, pbeta, qbeta; [0.30, 0.33, 0.333, 0.35], [lp], iterative, central);
    grid!("chi-square", "chisq", "5", [5.0];
        dchisq, pchisq, qchisq; positive, [lp], iterative, central);
    grid!("chi-square", "chisq", "0", [0.0];
        dchisq, pchisq, qchisq; positive, [lp], iterative, central);
    grid!("Student's t", "t", "5", [5.0];
        dt, pt, qt; heavy, [lp], iterative, extreme_t);
    grid!("Student's t", "t", "2", [2.0];
        dt, pt, qt; heavy, [lp], iterative, central);
    grid!("Student's t", "t", "1", [1.0];
        dt, pt, qt; heavy, [lp], iterative, central);
    grid!("Student's t", "t", "Inf", [INF];
        dt, pt, qt; heavy, [lp], iterative, central);
    grid!("F", "f", "4,9", [4.0, 9.0];
        df, pf, qf; positive, [lp], iterative, central);
    grid!("F", "f", "Inf,9", [INF, 9.0];
        df, pf, qf; positive, [lp], iterative, central);
    grid!("F", "f", "4,Inf", [4.0, INF];
        df, pf, qf; positive, [lp], iterative, central);
    grid!("F", "f", "Inf,Inf", [INF, INF];
        df, pf, qf; [0.0, 0.5, 1.0, 2.0, INF], [lp], iterative, central);
    grid!("noncentral chi-square", "chisq", "5,ncp=2", [5.0, 2.0];
        dnchisq, pnchisq, qnchisq; positive, [lp], iterative, central);
    grid!("noncentral chi-square", "chisq", "0,ncp=2", [0.0, 2.0];
        dnchisq, pnchisq, qnchisq; positive, [], [2e-12; 3], central);
    grid!("noncentral beta", "beta", "2,3,ncp=2", [2.0, 3.0, 2.0];
        dnbeta, pnbeta, qnbeta; unit, [], iterative, central);
    grid!("noncentral F", "f", "4,9,ncp=2", [4.0, 9.0, 2.0];
        dnf, pnf, qnf; positive, [], iterative, central);
    grid!("noncentral t", "t", "5,ncp=2", [5.0, 2.0];
        dnt, pnt, qnt; heavy, [], [3e-12, central, central], central);
    for (nmeans, df, nranges) in [(4.0, 10.0, 1.0), (2.5, 80.0, 1.5), (4.0, INF, 2.0)] {
        for tail in [LOWER, UPPER, Tail::default()] {
            let args = format!(
                "{},{},{},lower.tail={},log.p={}",
                literal(nmeans),
                literal(df),
                literal(nranges),
                if tail.lower { "TRUE" } else { "FALSE" },
                if tail.log { "TRUE" } else { "FALSE" }
            );
            for x in [0.0, 1.0, 3.0, 5.0, INF] {
                cases.push(Case {
                    family: "studentized range",
                    expression: format!("ptukey({},{args})", literal(x)),
                    actual: ptukey(x, nmeans, df, nranges, tail, &mut Flags::default()),
                    tolerance: tukey_tolerance,
                });
            }
            for p in [0.25_f64, 0.75, 0.95] {
                let p = if tail.log { p.ln() } else { p };
                cases.push(Case {
                    family: "studentized range",
                    expression: format!("qtukey({},{args})", literal(p)),
                    actual: qtukey(p, nmeans, df, nranges, tail, &mut Flags::default()),
                    tolerance: tukey_tolerance,
                });
            }
        }
    }
    let mut script = String::new();
    for case in &cases {
        script.push_str(&format!(
            "cat(sprintf(\"%.17g\", {}), \"\\n\")\n",
            case.expression
        ));
    }
    let Some(expected) = oracle(&script) else {
        return;
    };
    assert_eq!(expected.len(), cases.len());
    let mut stats = BTreeMap::new();
    let mut failures = BTreeMap::new();
    let mut rounded_endpoints = 0;
    for (case, expected) in cases.iter().zip(expected) {
        let (count, max): &mut (usize, f64) = stats.entry(case.family).or_default();
        *count += 1;
        // R rounds some F quantiles to zero and returns a subnormal for the chi-square atom.
        // The independent tests check the F log tail and the exact location of the atom.
        let f_rounding = case.expression.starts_with("qf(")
            && expected == 0.0
            && case.actual > 0.0
            && case.actual < 1e-140;
        let atom_rounding = case.expression.starts_with("qchisq(")
            && case.actual == 0.0
            && expected > 0.0
            && expected < f64::MIN_POSITIVE;
        if f_rounding || atom_rounding {
            rounded_endpoints += 1;
            continue;
        }
        let error = relative_error(case.actual, expected);
        *max = max.max(error);
        if error > case.tolerance {
            let failed: &mut Vec<String> = failures.entry(case.family).or_default();
            failed.push(format!(
                "{}: expected {expected:.17e}, got {:.17e} ({error:.3e})",
                case.expression, case.actual
            ));
        }
    }
    for (family, (count, max)) in stats {
        eprintln!("{family}: {count} cases, max relative error {max:.3e}");
    }
    eprintln!("{rounded_endpoints} endpoint differences within absolute bounds");
    for (family, failed) in &failures {
        eprintln!("{family}: {} failures", failed.len());
        for entry in failed.iter().take(12) {
            eprintln!("  {entry}");
        }
    }
    assert!(failures.is_empty());
}

#[test]
fn exponential_and_gamma_streams() {
    let Some(expected) = oracle(
        r#"
        set.seed(42L); cat(sprintf("%.17g", rexp(6, 1.5)), "\n")
        for (shape in c(.5,1,2.5)) {
            set.seed(42L); cat(sprintf("%.17g", rgamma(3,shape)), "\n")
        }
    "#,
    ) else {
        return;
    };
    let mut rng = Rng::seeded(42, Kinds::default());
    let mut flags = Flags::default();
    let mut actual: Vec<_> = (0..6).map(|_| rexp(&mut rng, 1.5, &mut flags)).collect();
    for shape in [0.5, 1.0, 2.5] {
        let mut rng = Rng::seeded(42, Kinds::default());
        actual.extend((0..3).map(|_| rgamma(&mut rng, shape, 1.0, &mut flags)));
    }
    assert_eq!(actual, expected);
    assert_eq!(flags.bits(), 0);
}

#[test]
fn documented_noncentral_upper_tails() {
    let Some(expected) = oracle(
        r#"
        cat(sprintf("%.17g", c(
            pbeta(1-1e-12, 2, 3, ncp=2, lower.tail=FALSE, log.p=TRUE),
            pf(1e10, 4, 9, ncp=2, lower.tail=FALSE, log.p=TRUE),
            pt(1e6, 5, ncp=2, lower.tail=FALSE, log.p=TRUE))))
    "#,
    ) else {
        return;
    };
    let actual = [
        pnbeta(1.0 - 1e-12, 2.0, 3.0, 2.0, UPPER, &mut Flags::default()),
        pnf(1e10, 4.0, 9.0, 2.0, UPPER, &mut Flags::default()),
        pnt(1e6, 5.0, 2.0, UPPER, &mut Flags::default()),
    ];
    assert_eq!(expected.len(), actual.len());
    for (x, expected) in actual.into_iter().zip(expected) {
        assert!(
            relative_error(x, expected) <= 1e-12,
            "Expected {expected:.17e}, got {x:.17e}"
        );
    }
}

#[test]
fn extreme_log_quantiles() {
    let p = 1e-300_f64.ln();
    let mut flags = Flags::default();
    let q = qf(p, 4.0, 9.0, LOWER, &mut flags);
    assert!(q > 0.0 && q < 1e-140);
    assert!((pf(q, 4.0, 9.0, LOWER, &mut flags) - p).abs() < 1e-11);
    let q = qnchisq(p, 5.0, 2.0, UPPER, &mut flags);
    assert!((pnchisq(q, 5.0, 2.0, UPPER, &mut flags) - p).abs() < 1e-9);
    let q = qnbeta(p, 2.0, 3.0, 2.0, LOWER, &mut flags);
    assert!((pnbeta(q, 2.0, 3.0, 2.0, LOWER, &mut flags) - p).abs() < 1e-10);
    let q = qnf(p, 4.0, 9.0, 2.0, LOWER, &mut flags);
    assert!((pnf(q, 4.0, 9.0, 2.0, LOWER, &mut flags) - p).abs() < 1e-10);
    let q = qtukey(p, 4.0, 10.0, 1.0, LOWER, &mut flags);
    assert!(q > 0.0 && q < 1e-90);
    assert!((ptukey(q, 4.0, 10.0, 1.0, LOWER, &mut flags) - p).abs() < 1e-10);
    let q = qtukey(p, 4.0, 10.0, 1.0, UPPER, &mut flags);
    assert!((ptukey(q, 4.0, 10.0, 1.0, UPPER, &mut flags) - p).abs() < 1e-10);
    assert!(qnt(p, 5.0, 2.0, LOWER, &mut flags) < -1e58);
    assert!(qnt(p, 5.0, 2.0, UPPER, &mut flags) > 1e60);
}

#[test]
fn parameter_flags_and_atoms() {
    let mut flags = Flags::default();
    assert!(dunif(0.0, 1.0, 1.0, false, &mut flags).is_nan());
    assert!(flags.has(Flags::NAN_PRODUCED));
    flags = Flags::default();
    assert!(pgamma(f64::NAN, 2.0, 1.0, LOWER, &mut flags).is_nan());
    assert_eq!(flags.bits(), 0);
    assert_eq!(dchisq(0.0, 0.0, false, &mut flags), INF);
    assert_eq!(pchisq(0.0, 0.0, LOWER, &mut flags), -INF);
    assert_eq!(pnchisq(0.0, 0.0, 2.0, LOWER, &mut flags), -1.0);
    assert_eq!(qnchisq(0.25, 0.0, 2.0, Tail::default(), &mut flags), 0.0);
    assert_eq!(qbeta(0.5, 0.0, 0.0, Tail::default(), &mut flags), 0.5);
    assert_eq!(punif(1.0, 1.0, 1.0, Tail::default(), &mut flags), 1.0);
}

#[test]
fn sampler_distributions() {
    let mut rng = Rng::seeded(42, Kinds::default());
    let mut flags = Flags::default();
    let n = 16_384;
    let mut check = |name: &str,
                     bound: (f64, f64),
                     point: f64,
                     expected_cdf: f64,
                     mean: Option<(f64, f64)>,
                     sample: fn(&mut Rng, &mut Flags) -> f64| {
        let mut count = 0;
        let mut sum = 0.0;
        for _ in 0..n {
            let x = sample(&mut rng, &mut flags);
            assert!(
                x.is_finite() && (bound.0..=bound.1).contains(&x),
                "{name}: {x}"
            );
            sum += x;
            count += i32::from(x <= point);
        }
        let actual_cdf = f64::from(count) / f64::from(n);
        assert!(
            (actual_cdf - expected_cdf).abs() < 0.018,
            "{name}: {actual_cdf}"
        );
        if let Some((expected, tolerance)) = mean {
            let actual = sum / f64::from(n);
            assert!((actual - expected).abs() < tolerance, "{name}: {actual}");
        }
    };
    let positive = (0.0, INF);
    let unit = (0.0, 1.0);
    let real = (-INF, INF);
    check(
        "uniform",
        (-2.0, 3.0),
        0.5,
        0.5,
        Some((0.5, 0.06)),
        |r, f| runif(r, -2.0, 3.0, f),
    );
    check(
        "exponential",
        positive,
        0.5,
        0.5276334473,
        Some((2.0 / 3.0, 0.04)),
        |r, f| rexp(r, 1.5, f),
    );
    check(
        "lognormal",
        positive,
        1.0,
        0.5,
        Some((0.125_f64.exp(), 0.04)),
        |r, f| rlnorm(r, 0.0, 0.5, f),
    );
    check("logistic", real, 0.0, 0.5, Some((0.0, 0.16)), |r, f| {
        rlogis(r, 0.0, 2.0, f)
    });
    check("Cauchy", real, 0.5, 0.5779791304, None, |r, f| {
        rcauchy(r, 0.0, 2.0, f)
    });
    check(
        "Weibull",
        positive,
        0.5,
        0.1175030974,
        Some((1.805490586, 0.07)),
        |r, f| rweibull(r, 1.5, 2.0, f),
    );
    check(
        "gamma",
        positive,
        0.5,
        0.0869301855,
        Some((5.0 / 3.0, 0.08)),
        |r, f| rgamma(r, 2.5, 2.0 / 3.0, f),
    );
    check("beta", unit, 0.5, 0.6875, Some((0.4, 0.02)), |r, f| {
        rbeta(r, 2.0, 3.0, f)
    });
    check(
        "chi-square",
        positive,
        5.0,
        0.5841198130,
        Some((5.0, 0.13)),
        |r, f| rchisq(r, 5.0, f),
    );
    check(
        "Student's t",
        real,
        0.5,
        0.6808505642,
        Some((0.0, 0.06)),
        |r, f| rt(r, 5.0, f),
    );
    check(
        "F",
        positive,
        0.5,
        0.2630101278,
        Some((9.0 / 7.0, 0.07)),
        |r, f| rf(r, 4.0, 9.0, f),
    );
    check(
        "noncentral chi-square",
        positive,
        7.0,
        0.5776841323,
        Some((7.0, 0.2)),
        |r, f| rnchisq(r, 5.0, 2.0, f),
    );
    check("noncentral beta", unit, 0.5, 0.5164987642, None, |r, f| {
        rnbeta(r, 2.0, 3.0, 2.0, f)
    });
    check(
        "noncentral t",
        real,
        2.0,
        0.4637067224,
        Some((2.378832155, 0.1)),
        |r, f| rnt(r, 5.0, 2.0, f),
    );
    check(
        "noncentral F",
        positive,
        0.5,
        0.1390022971,
        Some((27.0 / 14.0, 0.13)),
        |r, f| rnf(r, 4.0, 9.0, 2.0, f),
    );
    assert_eq!(flags.bits(), 0);
}

#[test]
fn gamma_rejection_streams_and_final_states() {
    let Some(expected) = oracle(
        r#"
        for (shape in c(.25,1,2.5,4,15,60)) {
            set.seed(42L)
            n <- if (shape < 1) 500L else 5000L
            cat(sprintf("%.17g", rgamma(n,shape)), "\n")
            cat(sprintf("%d", .Random.seed), "\n")
        }
    "#,
    ) else {
        return;
    };
    let mut expected = expected.into_iter();
    let mut flags = Flags::default();
    for shape in [0.25, 1.0, 2.5, 4.0, 15.0, 60.0] {
        let mut rng = Rng::seeded(42, Kinds::default());
        let n = if shape < 1.0 { 500 } else { 5000 };
        for i in 0..n {
            let actual = rgamma(&mut rng, shape, 1.0, &mut flags);
            assert_eq!(
                Some(actual.to_bits()),
                expected.next().map(f64::to_bits),
                "Gamma shape {shape}, draw {i}"
            );
        }
        let mut seed = Vec::new();
        rng.encode(&mut seed);
        for word in seed {
            assert_eq!(
                Some(f64::from(word)),
                expected.next(),
                "Gamma shape {shape}"
            );
        }
    }
    assert!(expected.next().is_none());
    assert_eq!(flags.bits(), 0);
}

#[test]
fn studentized_range_independent_checks() {
    let mut flags = Flags::default();
    for df in [2.0, 10.0, 80.0, INF] {
        for q in [0.125, 1.0, 3.0, 5.0, 100.0] {
            let log_upper = std::f64::consts::LN_2
                + pt(q * std::f64::consts::FRAC_1_SQRT_2, df, UPPER, &mut flags);
            let actual = ptukey(q, 2.0, df, 1.0, UPPER, &mut flags);
            assert!(
                (actual - log_upper).abs() < 1e-11,
                "df {df}, q {q}: {actual} vs {log_upper}"
            );
        }
    }
    assert_eq!(flags.bits(), 0);

    let Some(expected) = oracle(
        r#"
        h <- function(w) 4*integrate(function(t)
            dnorm(t)*(pnorm(t+w)-pnorm(t))^3, -12, 12,
            rel.tol=5e-13, abs.tol=1e-15)$value
        for (q in c(1,3,5)) {
            cdf <- integrate(function(s) {
                vapply(q*s,h,numeric(1))*20*s*dchisq(10*s^2,10)
            }, 0, 5, rel.tol=5e-13, abs.tol=1e-15)$value
            cat(sprintf("%.17g", cdf), "\n")
        }
    "#,
    ) else {
        return;
    };
    assert_eq!(expected.len(), 3);
    for (q, expected) in [1.0, 3.0, 5.0].into_iter().zip(expected) {
        let actual = ptukey(q, 4.0, 10.0, 1.0, Tail::default(), &mut flags);
        assert!(
            relative_error(actual, expected) <= 1e-12,
            "q {q}: {actual} vs {expected}"
        );
    }
}

#[test]
fn extreme_gamma_beta_f_and_t_tails() {
    let mut flags = Flags::default();
    let values = [
        pgamma(
            f64::from_bits(0x7fe4_d288_ce70_3afc),
            2.0_f64.powi(1016),
            1.0,
            UPPER,
            &mut flags,
        ),
        pbeta(0.5, 0.5, f64::from_bits(1), LOWER, &mut flags),
        qf(0.5, 0.25, 1e308, Tail::default(), &mut flags),
        qnf(0.5, 0.25, 1e308, 2.0, Tail::default(), &mut flags),
        dt(0.0, 1e-310, true, &mut flags),
        dnt(1e12, 5.0, 1e12, true, &mut flags),
        dnf(1.0, INF, 5.0, 2.0, false, &mut flags),
        dnt(0.0, 5.0, INF, true, &mut flags),
        pnt(0.0, 5.0, -INF, Tail::default(), &mut flags),
        ptukey(INF, INF, 10.0, 1.0, Tail::default(), &mut flags),
        qtukey(0.0, 2.0, 10.0, INF, Tail::default(), &mut flags),
        qtukey(1.0, 2.0, 10.0, INF, Tail::default(), &mut flags),
        qtukey(0.95, 2.0, 10.0, 1.0, Tail::default(), &mut flags),
    ];
    let Some(expected) = oracle(
        r#"
        # Use the limit of the exact density integral at x = ncp -> Inf.
        cat(sprintf("%.17g", c(
          pgamma(0x1.4d288ce703afcp1023, shape=0x1p1016, lower.tail=FALSE, log.p=TRUE),
          pbeta(.5,.5,5e-324,log.p=TRUE),
          qf(.5,.25,1e308), qf(.5,.25,1e308,ncp=2),
          dt(0,1e-310,log=TRUE), log(10*dchisq(5,5))-log(1e12),
          df(1,Inf,5,ncp=2), dt(0,5,ncp=Inf,log=TRUE), pt(0,5,ncp=-Inf),
          ptukey(Inf,Inf,10), qtukey(c(0,1),2,10,nranges=Inf),
          sqrt(2)*qt(.975,10))))
    "#,
    ) else {
        return;
    };
    assert_eq!(values.len(), expected.len());
    for (actual, expected) in values.into_iter().zip(expected) {
        assert!(
            relative_error(actual, expected) < 2e-12,
            "{actual} != {expected}"
        );
    }
    let tiny = pgamma(0.5, f64::from_bits(1), 1.0, UPPER, &mut flags);
    // Limit Q(a, z)/a = E1(z), with E1(0.5) = 0.5597735947761608.
    assert!((tiny - (f64::from_bits(1).ln() + 0.559_773_594_776_160_8_f64.ln())).abs() < 2e-13);
    assert_eq!(qlogis(1e-320, 2.0, 0.0, Tail::default(), &mut flags), 2.0);
    assert_eq!(dlnorm(0.0, -INF, 0.0, false, &mut flags), INF);
}

#[test]
fn invalid_continuous_boundaries_set_warning_flags() {
    let cases: &[fn(&mut Flags) -> f64] = &[
        |f| roxide_math::dist::normal::qnorm(0.5, 0.0, INF, Tail::default(), f),
        |f| pgamma(INF, 2.0, INF, Tail::default(), f),
        |f| pgamma(0.5, INF, 1.0, Tail::default(), f),
        |f| pexp(INF, 0.0, Tail::default(), f),
        |f| dexp(INF, 0.0, false, f),
        |f| dexp(0.0, -0.0, false, f),
        |f| pexp(0.0, -0.0, Tail::default(), f),
        |f| qexp(0.0, -0.0, Tail::default(), f),
        |f| dlogis(INF, INF, 1.0, false, f),
        |f| plogis(INF, INF, 1.0, Tail::default(), f),
        |f| dcauchy(INF, INF, 1.0, false, f),
        |f| pcauchy(INF, INF, 1.0, Tail::default(), f),
    ];
    for case in cases {
        let mut flags = Flags::default();
        assert!(case(&mut flags).is_nan());
        assert!(flags.has(Flags::NAN_PRODUCED));
    }
}
