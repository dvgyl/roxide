//! Compare discrete kernels with the configured GNU R 4.6.2 binary.

use std::io::Write;
use std::process::{Command, Stdio};

use roxide_math::core::{Flags, MathError, Tail};
use roxide_math::dist::discrete::*;
use roxide_math::rng::{Kinds, Rng, SampleKind};

const INF: f64 = f64::INFINITY;

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
            stdin
                .write_all(b"options(warn=-1)\nstopifnot(getRversion() == '4.6.2')\n")
                .unwrap();
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
            .map(|x| {
                if x == "NA" {
                    f64::NAN
                } else {
                    x.parse().unwrap()
                }
            })
            .collect(),
    )
}

fn literal(x: f64) -> String {
    if x == INF {
        "Inf".to_owned()
    } else if x == -INF {
        "-Inf".to_owned()
    } else {
        format!("{x:.17e}")
    }
}

fn r_bool(value: bool) -> &'static str {
    if value { "TRUE" } else { "FALSE" }
}

#[derive(Default)]
struct Comparisons {
    expressions: Vec<String>,
    actual: Vec<(f64, usize)>,
}

impl Comparisons {
    fn add(&mut self, kind: usize, expression: String, result: f64) {
        self.expressions.push(expression);
        self.actual.push((result, kind));
    }

    fn check(self) {
        let script = self
            .expressions
            .iter()
            .map(|x| format!("cat(sprintf('%.17g', {x}), '\\n')\n"))
            .collect::<String>();
        let Some(expected) = oracle(&script) else {
            return;
        };
        assert_eq!(expected.len(), self.actual.len());
        let mut counts = [0; 3];
        let mut max_errors = [0.0_f64; 3];
        let mut failures = Vec::new();
        for ((expression, (actual, kind)), expected) in
            self.expressions.iter().zip(self.actual).zip(expected)
        {
            counts[kind] += 1;
            let error = if actual == expected || (actual.is_nan() && expected.is_nan()) {
                0.0
            } else if kind == 2 || !actual.is_finite() || !expected.is_finite() || expected == 0.0 {
                INF
            } else {
                ((actual - expected) / expected).abs()
            };
            max_errors[kind] = max_errors[kind].max(error);
            if error > 1e-13 {
                failures.push(format!(
                    "{expression}: {actual:.17e} vs {expected:.17e} ({error:.3e})"
                ));
            }
        }
        eprintln!(
            "Mass, cumulative, and quantile counts: {counts:?}. Maximum relative errors: {max_errors:?}."
        );
        assert!(
            failures.is_empty(),
            "{} comparisons failed.\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
}

#[test]
fn masses_tails_and_quantiles() {
    let mut cases = Comparisons::default();
    let mut flags = Flags::default();
    macro_rules! family {
        ($r:literal, $args:literal, [$($a:expr),+]; $d:ident, $p:ident, $q:ident; $xs:expr) => {{
            for x in $xs {
                for log in [false, true] {
                    let rx = literal(x);
                    let mode = r_bool(log);
                    cases.add(0, format!("d{}({rx},{},log={mode})", $r, $args),
                        $d(x, $($a),+, log, &mut flags));
                    for lower in [false, true] {
                        let lt = r_bool(lower);
                        cases.add(1, format!("p{}({rx},{},lower.tail={lt},log.p={mode})", $r, $args),
                            $p(x, $($a),+, Tail {lower, log}, &mut flags));
                    }
                }
            }
            for lower in [false, true] {
                for log in [false, true] {
                    let tail = Tail {lower, log};
                    let probabilities = if log {
                        vec![-INF, -1000.0, 1e-300_f64.ln(), -20.0, -1.0, -1e-12, 0.0]
                    } else {
                        vec![0.0, 1e-300, 0.1, 0.5, 0.75, 1.0 - 1e-12, 1.0]
                    };
                    for p in probabilities {
                        cases.add(2, format!("q{}({},{},lower.tail={},log.p={})", $r, literal(p),
                            $args, r_bool(lower), r_bool(log)), $q(p, $($a),+, tail, &mut flags));
                    }
                }
            }
        }};
    }
    family!("binom", "8,.25", [8.0, 0.25]; dbinom, pbinom, qbinom; [-1.0, 0.0, 2.0, 2.25, 5.0, 8.0, INF]);
    family!("binom", "2000,.5", [2000.0, 0.5]; dbinom, pbinom, qbinom; [0.0, 140.0, 220.0, 900.0, 1000.0, 1100.0, 1779.0, 1999.0]);
    family!("pois", "3", [3.0]; dpois, ppois, qpois; [-1.0, 0.0, 2.0, 2.25, 100.0, 210.0, INF]);
    family!("nbinom", "2,.4", [2.0, 0.4]; dnbinom, pnbinom, qnbinom; [-1.0, 0.0, 2.0, 2.25, 100.0, 1350.0, INF]);
    family!("nbinom", "size=2.25,mu=3.75", [2.25, 3.75]; dnbinom_mu, pnbinom_mu, qnbinom_mu; [0.0, 1.0, 2.0, 100.0, 1400.0]);
    family!("nbinom", "size=Inf,mu=1", [INF, 1.0]; dnbinom_mu, pnbinom_mu, qnbinom_mu; [0.0, 1.0, 2.0]);
    family!("nbinom", "size=1e100,mu=1", [1e100, 1.0]; dnbinom_mu, pnbinom_mu, qnbinom_mu; [0.0, 1.0, 2.0, 10.0, 19.0]);
    family!("geom", ".3", [0.3]; dgeom, pgeom, qgeom; [-1.0, 0.0, 2.0, 2.25, 100.0, 1930.0, INF]);
    family!("hyper", "6,9,5", [6.0, 9.0, 5.0]; dhyper, phyper, qhyper; [-1.0, 0.0, 2.0, 2.25, 3.0, 4.0, 5.0, INF]);
    family!("hyper", "1000,1500,700", [1000.0, 1500.0, 700.0]; dhyper, phyper, qhyper; [0.0, 42.0, 280.0, 549.0, 606.0, 664.0, 699.0]);
    family!("hyper", "1.5e9,1e9,1", [1.5e9, 1e9, 1.0]; dhyper, phyper, qhyper; [-1.0, 0.0, 1.0]);
    family!("hyper", "1.5e9,1e9,5", [1.5e9, 1e9, 5.0]; dhyper, phyper, qhyper; [-1.0, 0.0, 1.0, 3.0, 4.0, 5.0, 6.0]);
    family!("hyper", "1.5e9,1e9,30", [1.5e9, 1e9, 30.0]; dhyper, phyper, qhyper; [0.0, 2.0, 18.0, 25.0, 30.0]);
    family!("wilcox", "3,4", [3.0, 4.0]; dwilcox, pwilcox, qwilcox; [-1.0, 0.0, 2.0, 2.25, 6.0, 8.0, 12.0, INF]);
    family!("signrank", "4", [4.0]; dsignrank, psignrank, qsignrank; [-1.0, 0.0, 2.0, 2.25, 5.0, 7.0, 10.0, INF]);
    for log in [false, true] {
        for (x, weights) in [
            ([4.0, 4.0, 2.0], [2.0, 3.0, 5.0]),
            ([0.0, 1.0, 0.0], [0.0, 8.0, 0.0]),
            ([15.0, 15.0, 70.0], [1.0, 1.0, 4.0]),
            ([1.0, 1.0, 1.0], [1e-200, 3e-100, 1.0]),
        ] {
            cases.add(
                0,
                format!(
                    "dmultinom(c({},{},{}),prob=c({},{},{}),log={})",
                    x[0],
                    x[1],
                    x[2],
                    weights[0],
                    weights[1],
                    weights[2],
                    r_bool(log)
                ),
                dmultinom(&x, &weights, log, &mut flags),
            );
        }
    }
    for q in [-1e-8, 2.0 - 1e-7, 2.0 - 1e-6] {
        cases.add(
            1,
            format!("ppois({},3)", literal(q)),
            ppois(q, 3.0, Tail::default(), &mut flags),
        );
        cases.add(
            1,
            format!("psignrank({},4)", literal(q)),
            psignrank(q, 4.0, Tail::default(), &mut flags),
        );
    }
    cases.add(
        2,
        "qbinom(.5,-.2,.5)".to_owned(),
        qbinom(0.5, -0.2, 0.5, Tail::default(), &mut flags),
    );
    cases.add(
        2,
        "qbinom(1,4.7,.5)".to_owned(),
        qbinom(1.0, 4.7, 0.5, Tail::default(), &mut flags),
    );
    cases.add(
        0,
        "dnbinom(1e17,size=1e15,mu=1e17)".to_owned(),
        dnbinom_mu(1e17, 1e15, 1e17, false, &mut flags),
    );
    cases.add(
        1,
        "psignrank(0,1000)".to_owned(),
        psignrank(0.0, 1000.0, Tail::default(), &mut flags),
    );
    cases.check();
}

#[test]
fn quantile_jumps() {
    let mut cases = Comparisons::default();
    let mut flags = Flags::default();
    macro_rules! jumps {
        ($r:literal, $args:literal, [$($a:expr),+]; $q:ident; $xs:expr) => {{
            let mut inputs = String::new();
            let mut labels = Vec::new();
            for k in $xs {
                for lower in [false, true] {
                    for log in [false, true] {
                        for delta in [-1e-12, 0.0, 1e-12] {
                            let p = format!("p{}({k},{},lower.tail={})*(1+{})",
                                $r, $args, r_bool(lower), literal(delta));
                            let p = if log { format!("log({p})") } else { p };
                            inputs.push_str(&format!("cat(sprintf('%.17g',{p}),'\\n')\n"));
                            labels.push((p, Tail { lower, log }));
                        }
                    }
                }
            }
            let Some(inputs) = oracle(&inputs) else { return };
            assert_eq!(inputs.len(), labels.len());
            for (p, (expression, tail)) in inputs.into_iter().zip(labels) {
                cases.add(2, format!("q{}({expression},{},lower.tail={},log.p={})",
                    $r, $args, r_bool(tail.lower), r_bool(tail.log)), $q(p, $($a),+, tail, &mut flags));
            }
        }};
    }
    jumps!("binom", "8,.25", [8.0,0.25]; qbinom; [0,2,4,6,7]);
    jumps!("pois", "3", [3.0]; qpois; [0,2,3,7,12]);
    jumps!("nbinom", "2,.4", [2.0,0.4]; qnbinom; [0,2,3,7,12]);
    jumps!("nbinom", "size=2,mu=3", [2.0,3.0]; qnbinom_mu; [0,2,3,7,12]);
    jumps!("geom", ".3", [0.3]; qgeom; [0,2,3,7,12]);
    jumps!("hyper", "6,9,5", [6.0,9.0,5.0]; qhyper; [0,1,2,3,4]);
    jumps!("hyper", "5,7,9", [5.0,7.0,9.0]; qhyper; [2,3,4]);
    jumps!("hyper", "1.5e9,1e9,30", [1.5e9,1e9,30.0]; qhyper; [0,2,4,10,12]);
    jumps!("wilcox", "3,4", [3.0,4.0]; qwilcox; [0,2,5,7,11]);
    jumps!("signrank", "4", [4.0]; qsignrank; [0,2,5,7,9]);
    for offset in [8.0, 9.0, 10.0, 11.0, 1000.0, 1001.0, 1002.0] {
        let p = 0.5 + offset * f64::EPSILON;
        cases.add(
            2,
            format!("qsignrank({},1)", literal(p)),
            qsignrank(p, 1.0, Tail::default(), &mut flags),
        );
        let p = 0.5 * (1.0 + offset * f64::EPSILON);
        cases.add(
            2,
            format!("qbinom({},1,.5)", literal(p)),
            qbinom(p, 1.0, 0.5, Tail::default(), &mut flags),
        );
        cases.add(
            2,
            format!("qhyper({},1,1,1)", literal(p)),
            qhyper(p, 1.0, 1.0, 1.0, Tail::default(), &mut flags),
        );
    }
    cases.check();
}

#[test]
fn random_streams_and_final_states() {
    let mut flags = Flags::default();
    let mut script = String::new();
    let mut cases = Vec::new();
    macro_rules! stream {
        ($r:literal, $seed:expr, $sample:ident, $rng:ident, $draw:expr) => {{
            let kinds = Kinds { sample: SampleKind::$sample, ..Kinds::default() };
            let mut $rng = Rng::seeded($seed, kinds);
            let values: Vec<f64> = $draw;
            let mut state = Vec::new();
            $rng.encode(&mut state);
            script.push_str(&format!("suppressWarnings(RNGkind('Mersenne-Twister','Inversion','{}')); set.seed({}); cat(sprintf('%.17g',{}),'\\n'); cat(sprintf('%d',.Random.seed),'\\n')\n", stringify!($sample), $seed, $r));
            cases.push(($r, values, state));
        }};
    }
    stream!(
        "rbinom(3,59,.5)",
        42,
        Rejection,
        rng,
        (0..3)
            .map(|_| rbinom(&mut rng, 59.0, 0.5, &mut flags))
            .collect()
    );
    stream!(
        "rbinom(3,60,.5)",
        42,
        Rejection,
        rng,
        (0..3)
            .map(|_| rbinom(&mut rng, 60.0, 0.5, &mut flags))
            .collect()
    );
    stream!(
        "rbinom(200,300,.2)",
        42,
        Rejection,
        rng,
        (0..200)
            .map(|_| rbinom(&mut rng, 300.0, 0.2, &mut flags))
            .collect()
    );
    stream!(
        "rbinom(200,7000,.95)",
        11,
        Rejection,
        rng,
        (0..200)
            .map(|_| rbinom(&mut rng, 7000.0, 0.95, &mut flags))
            .collect()
    );
    stream!(
        "rbinom(100,2147483647,.2)",
        42,
        Rejection,
        rng,
        (0..100)
            .map(|_| rbinom(&mut rng, 2147483647.0, 0.2, &mut flags))
            .collect()
    );
    stream!(
        "rpois(3,9.99)",
        42,
        Rejection,
        rng,
        (0..3).map(|_| rpois(&mut rng, 9.99, &mut flags)).collect()
    );
    stream!(
        "rpois(3,10)",
        42,
        Rejection,
        rng,
        (0..3).map(|_| rpois(&mut rng, 10.0, &mut flags)).collect()
    );
    stream!(
        "rpois(500,10)",
        42,
        Rejection,
        rng,
        (0..500)
            .map(|_| rpois(&mut rng, 10.0, &mut flags))
            .collect()
    );
    stream!(
        "rpois(300,70)",
        11,
        Rejection,
        rng,
        (0..300)
            .map(|_| rpois(&mut rng, 70.0, &mut flags))
            .collect()
    );
    stream!(
        "rnbinom(200,2,.4)",
        42,
        Rejection,
        rng,
        (0..200)
            .map(|_| rnbinom(&mut rng, 2.0, 0.4, &mut flags))
            .collect()
    );
    stream!(
        "rnbinom(200,size=2.25,mu=18)",
        42,
        Rejection,
        rng,
        (0..200)
            .map(|_| rnbinom_mu(&mut rng, 2.25, 18.0, &mut flags))
            .collect()
    );
    stream!(
        "rgeom(100,.3)",
        42,
        Rejection,
        rng,
        (0..100).map(|_| rgeom(&mut rng, 0.3, &mut flags)).collect()
    );
    stream!(
        "rgeom(3,1)",
        42,
        Rejection,
        rng,
        (0..3).map(|_| rgeom(&mut rng, 1.0, &mut flags)).collect()
    );
    stream!(
        "rwilcox(200,3,4)",
        42,
        Rejection,
        rng,
        (0..200)
            .map(|_| rwilcox(&mut rng, 3.0, 4.0, &mut flags))
            .collect()
    );
    stream!(
        "rwilcox(200,3,4)",
        42,
        Rounding,
        rng,
        (0..200)
            .map(|_| rwilcox(&mut rng, 3.0, 4.0, &mut flags))
            .collect()
    );
    stream!(
        "rsignrank(200,4)",
        42,
        Rejection,
        rng,
        (0..200)
            .map(|_| rsignrank(&mut rng, 4.0, &mut flags))
            .collect()
    );
    stream!(
        "rmultinom(100,10,c(2,3,5))",
        42,
        Rejection,
        rng,
        (0..100)
            .flat_map(|_| {
                let mut out = [0; 3];
                rmultinom(&mut rng, 10, &[2.0, 3.0, 5.0], &mut out).unwrap();
                out.map(f64::from)
            })
            .collect()
    );
    stream!(
        "rhyper(100,1288490188,858993460,1)",
        19,
        Rejection,
        rng,
        (0..100)
            .map(|_| rhyper(&mut rng, 1288490188.0, 858993460.0, 1.0, &mut flags))
            .collect()
    );
    stream!(
        "rhyper(100,1288490188,858993460,5)",
        19,
        Rejection,
        rng,
        (0..100)
            .map(|_| rhyper(&mut rng, 1288490188.0, 858993460.0, 5.0, &mut flags))
            .collect()
    );
    stream!(
        "rhyper(100,1288490188,858993460,30)",
        19,
        Rejection,
        rng,
        (0..100)
            .map(|_| rhyper(&mut rng, 1288490188.0, 858993460.0, 30.0, &mut flags))
            .collect()
    );
    stream!(
        "rhyper(100,1288490188,858993459,30)",
        19,
        Rejection,
        rng,
        (0..100)
            .map(|_| rhyper(&mut rng, 1288490188.0, 858993459.0, 30.0, &mut flags))
            .collect()
    );
    stream!(
        "rhyper(100,1500000000,1000000000,30)",
        42,
        Rejection,
        rng,
        (0..100)
            .map(|_| rhyper(&mut rng, 1.5e9, 1e9, 30.0, &mut flags))
            .collect()
    );
    stream!(
        "rhyper(100,60,50,80)",
        42,
        Rejection,
        rng,
        (0..100)
            .map(|_| rhyper(&mut rng, 60.0, 50.0, 80.0, &mut flags))
            .collect()
    );
    stream!(
        "rhyper(100,3,12,5)",
        42,
        Rejection,
        rng,
        (0..100)
            .map(|_| rhyper(&mut rng, 3.0, 12.0, 5.0, &mut flags))
            .collect()
    );

    let Some(expected) = oracle(&script) else {
        return;
    };
    let mut length = 0;
    let mut sample_count = 0;
    let stream_count = cases.len();
    for (label, values, state) in cases {
        let oracle_values = &expected[length..length + values.len()];
        let first_mismatch = values
            .iter()
            .zip(oracle_values)
            .position(|(ours, theirs)| ours != theirs);
        assert_eq!(
            first_mismatch,
            None,
            "{label}. First 10 Rust values {:?}. First 10 R values {:?}.",
            &values[..values.len().min(10)],
            &oracle_values[..values.len().min(10)]
        );
        length += values.len();
        sample_count += values.len();
        assert!(
            state
                .iter()
                .map(|&x| f64::from(x))
                .eq(expected[length..length + state.len()].iter().copied()),
            "{label}. The final states differ."
        );
        length += state.len();
    }
    assert_eq!(length, expected.len());
    eprintln!("Verified {sample_count} values and {stream_count} full final generator states.");
}

#[test]
fn invalid_inputs_and_integer_tolerance() {
    let mut flags = Flags::default();
    assert!(dpois(2.0 + 1.9e-9, 3.0, false, &mut flags) > 0.0);
    assert_eq!(dpois(2.0 + 2.1e-9, 3.0, false, &mut flags), 0.0);
    assert!(flags.has(Flags::NONINT_X));
    let mut flags = Flags::default();
    assert_eq!(dsignrank(2.25, 4.0, false, &mut flags), 0.0);
    assert!(!flags.has(Flags::NONINT_X));
    assert!(dwilcox(2.0, 10_000.0, 10_000.0, false, &mut flags).is_nan());
    assert!(dsignrank(2.0, 1e6, true, &mut flags).is_nan());
    assert!(flags.has(Flags::NAN_PRODUCED));
    let mut rng = Rng::seeded(42, Kinds::default());
    let mut out = [3, 5];
    assert_eq!(
        rmultinom(&mut rng, 3, &[0.0, 0.0], &mut out),
        Err(MathError::Dim)
    );
    assert_eq!(out, [3, 5]);
}

#[test]
fn probabilities_with_exact_combinatorial_references() {
    let mut flags = Flags::default();
    let upper = Tail {
        lower: false,
        log: false,
    };
    // These references use integer binomial coefficients. The corresponding
    // oracle probabilities differ from the exact values by about 1e-13.
    let upper_hyper = phyper(550.0, 1000.0, 1500.0, 700.0, upper, &mut flags);
    assert!((upper_hyper / 3.347_561_977_886_486e-136 - 1.0).abs() < 1e-14);
    let tiny_hyper = phyper(666.0, 1000.0, 1500.0, 700.0, upper, &mut flags);
    assert!((tiny_hyper / 2.781_926_381_787_558e-300 - 1.0).abs() < 1e-14);
    let multi = dmultinom(&[150.0, 153.0, 697.0], &[1.0, 1.0, 4.0], false, &mut flags);
    assert!((multi / 0.000_151_063_071_559_746_83 - 1.0).abs() < 1e-14);
}

#[test]
fn extreme_discrete_tails_and_endpoints() {
    let mut flags = Flags::default();
    let log_upper = Tail {
        lower: false,
        log: true,
    };
    let value = pnbinom_mu(1.0, 1e-12, 1e-30, log_upper, &mut flags);
    assert!((value + 111.217_231_644_273_14).abs() < 2e-12, "{value}");
    assert_eq!(qnbinom_mu(-110.0, 1e-12, 1e-30, log_upper, &mut flags), 1.0);
    let large = 2.0_f64.powi(54);
    let value = phyper(
        large / 2.0,
        large,
        large,
        large,
        Tail::default(),
        &mut flags,
    );
    assert!((value - 0.5).abs() < 1e-8, "{value}");
    let value = phyper(1.0, 2.0, large, large / 2.0, Tail::default(), &mut flags);
    assert!((value - 0.75).abs() < 1e-15, "{value}");
    assert_eq!(qwilcox(0.0, 400.0, 400.0, Tail::default(), &mut flags), 0.0);
    assert_eq!(dwilcox(-1.0, 400.0, 400.0, false, &mut flags), 0.0);
    assert_eq!(
        dsignrank(0.0, 1100.0, true, &mut flags),
        -1100.0 * std::f64::consts::LN_2
    );
    assert_eq!(dbinom(1.0, INF, 0.4, false, &mut flags), 0.0);
    assert_eq!(dhyper(-1.5, 10.0, 10.0, 10.0, false, &mut flags), 0.0);
    assert_eq!(flags.bits(), 0);
    assert!(pbinom(1.0, 2.4, 0.5, Tail::default(), &mut flags).is_nan());
    assert!(flags.has(Flags::NONINT_X));
}
