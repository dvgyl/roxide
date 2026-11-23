//! Compare clean-room random streams with the configured GNU R binary.

use std::process::Command;

use roxide_math::core::{Flags, Tail};
use roxide_math::dist::normal::{dnorm, pnorm, qnorm, rnorm};
use roxide_math::rng::{Kinds, NormalKind, Rng, UniformKind};

const UNIFORMS: [(UniformKind, &str); 7] = [
    (UniformKind::WichmannHill, "Wichmann-Hill"),
    (UniformKind::MarsagliaMulticarry, "Marsaglia-Multicarry"),
    (UniformKind::SuperDuper, "Super-Duper"),
    (UniformKind::MersenneTwister, "Mersenne-Twister"),
    (UniformKind::KnuthTaocp, "Knuth-TAOCP"),
    (UniformKind::KnuthTaocp2002, "Knuth-TAOCP-2002"),
    (UniformKind::LecuyerCmrg, "L'Ecuyer-CMRG"),
];

fn oracle(script: &str) -> Option<String> {
    let Some(path) = std::env::var_os("ROXIDE_ORACLE_RSCRIPT") else {
        eprintln!("Set ROXIDE_ORACLE_RSCRIPT to run the live comparisons.");
        return None;
    };
    let output = Command::new(path)
        .args(["--vanilla", "-e", script])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Some(String::from_utf8(output.stdout).unwrap())
}

fn hex_float(text: &str) -> f64 {
    let negative = text.starts_with('-');
    let text = text.trim_start_matches('-').strip_prefix("0x").unwrap();
    let (mantissa, exponent) = text.split_once('p').unwrap();
    let (integer, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let whole = u64::from_str_radix(integer, 16).unwrap() as f64;
    let part = if fraction.is_empty() {
        0.0
    } else {
        u64::from_str_radix(fraction, 16).unwrap() as f64 * 2f64.powi(-4 * fraction.len() as i32)
    };
    let value = (whole + part) * 2f64.powi(exponent.parse().unwrap());
    if negative { -value } else { value }
}

fn check_seed(rng: &Rng, line: &str) {
    let expected: Vec<_> = line
        .split_whitespace()
        .map(|word| {
            if word == "NA" {
                i32::MIN
            } else {
                word.parse().unwrap()
            }
        })
        .collect();
    let mut actual = Vec::new();
    rng.encode(&mut actual);
    assert_eq!(actual.len(), expected.len());
    for (i, (actual, expected)) in actual.into_iter().zip(expected).enumerate() {
        assert_eq!(actual, expected, "Seed word {i} for {:?}", rng.kinds);
    }
}

fn check_stream(line: &str, mut draw: impl FnMut() -> f64) {
    let differences: Vec<_> = line
        .split_whitespace()
        .enumerate()
        .filter_map(|(i, value)| {
            let expected = hex_float(value);
            let actual = draw();
            (actual.to_bits() != expected.to_bits()).then_some((i, actual, expected))
        })
        .collect();
    assert!(
        differences.is_empty(),
        "{} mismatches. First mismatches: {:?}",
        differences.len(),
        &differences[..differences.len().min(8)]
    );
}

#[test]
fn uniform_streams_and_switches() {
    let names = UNIFORMS.map(|(_, name)| format!("\"{name}\"")).join(",");
    let Some(out) = oracle(&format!(
        r#"
        kinds <- c({names})
        for (source in kinds) {{
            suppressWarnings(set.seed(42L,source))
            cat(.Random.seed,"\n")
            cat(sprintf("%a",runif(5L)),"\n")
            cat(.Random.seed,"\n")
            for (target in kinds) {{
                suppressWarnings(set.seed(42L,source))
                suppressWarnings(RNGkind(target))
                cat(.Random.seed,"\n")
                cat(sprintf("%a",runif(5L)),"\n")
                cat(.Random.seed,"\n")
            }}
        }}
    "#
    )) else {
        return;
    };
    let mut lines = out.lines();
    for (source, _) in UNIFORMS {
        let kinds = Kinds {
            uniform: source,
            ..Kinds::default()
        };
        let mut rng = Rng::seeded(42, kinds);
        check_seed(&rng, lines.next().unwrap());
        check_stream(lines.next().unwrap(), || rng.unif_rand());
        check_seed(&rng, lines.next().unwrap());
        for (target, _) in UNIFORMS {
            let mut rng = Rng::seeded(42, kinds);
            rng.switch_uniform(target);
            check_seed(&rng, lines.next().unwrap());
            check_stream(lines.next().unwrap(), || rng.unif_rand());
            check_seed(&rng, lines.next().unwrap());
        }
    }
    assert!(lines.next().is_none());
}

#[test]
fn inversion_matches_one_thousand_draws() {
    let Some(out) = oracle(r#"set.seed(42L); cat(sprintf("%a", rnorm(1000L)))"#) else {
        return;
    };
    assert_eq!(out.split_whitespace().count(), 1000);
    let mut rng = Rng::seeded(42, Kinds::default());
    check_stream(&out, || rnorm(&mut rng, 0.0, 1.0, &mut Flags::default()));
}

#[test]
fn legacy_normal_and_exponential_streams() {
    let Some(out) = oracle(
        r#"
        for (kind in c("Ahrens-Dieter","Box-Muller","Kinderman-Ramage","Inversion")) {
            set.seed(42L,normal.kind=kind)
            cat(sprintf("%a",rnorm(10000L)),"\n")
            cat(.Random.seed,"\n")
        }
        set.seed(42L); .Random.seed[1] <- 10003L
        cat(sprintf("%a",rnorm(10000L)),"\n"); cat(.Random.seed,"\n")
        set.seed(42L,normal.kind="Inversion"); cat(sprintf("%a",rexp(10000L)),"\n"); cat(.Random.seed,"\n")
    "#,
    ) else {
        return;
    };
    let mut lines = out.lines();
    for normal in [
        NormalKind::AhrensDieter,
        NormalKind::BoxMuller,
        NormalKind::KindermanRamage,
        NormalKind::Inversion,
        NormalKind::BuggyKindermanRamage,
    ] {
        let kinds = Kinds {
            normal,
            ..Kinds::default()
        };
        let mut rng = Rng::seeded(42, kinds);
        let stream = lines.next().unwrap();
        if normal == NormalKind::BoxMuller {
            for (i, expected) in stream.split_whitespace().map(hex_float).enumerate() {
                let actual = rng.norm_rand();
                let limit = if i < 5 { 0 } else { 2 };
                assert!(
                    actual.to_bits().abs_diff(expected.to_bits()) <= limit,
                    "Box-Muller draw {i}"
                );
            }
        } else {
            check_stream(stream, || rng.norm_rand());
        }
        check_seed(&rng, lines.next().unwrap());
    }
    let mut rng = Rng::seeded(42, Kinds::default());
    check_stream(lines.next().unwrap(), || rng.exp_rand());
    check_seed(&rng, lines.next().unwrap());
}

#[test]
fn normal_density_and_tails() {
    let mut flags = Flags::default();
    let upper_log = Tail {
        lower: false,
        log: true,
    };
    let upper = Tail {
        lower: false,
        log: false,
    };
    assert_eq!(pnorm(0.0, 0.0, 1.0, Tail::default(), &mut flags), 0.5);
    assert_eq!(dnorm(0.0, 1.0, 0.0, false, &mut flags), 0.0);
    let Some(out) = oracle(
        r#"
        x <- c(0,0.5,2,8,14,29,37)
        cat(sprintf("%.17g",dnorm(x)), "\n")
        cat(sprintf("%.17g",pnorm(x,lower.tail=FALSE)), "\n")
        cat(sprintf("%.17g",pnorm(x,lower.tail=FALSE,log.p=TRUE)), "\n")
        cat(sprintf("%.17g",qnorm(c(-1e-8,-.02,-20,-713,-731,-1000,-1e10),lower.tail=FALSE,log.p=TRUE)), "\n")
    "#,
    ) else {
        return;
    };
    let mut actual = Vec::new();
    let x = [0.0, 0.5, 2.0, 8.0, 14.0, 29.0, 37.0];
    actual.extend(x.map(|x| dnorm(x, 0.0, 1.0, false, &mut flags)));
    actual.extend(x.map(|x| pnorm(x, 0.0, 1.0, upper, &mut flags)));
    actual.extend(x.map(|x| pnorm(x, 0.0, 1.0, upper_log, &mut flags)));
    actual.extend(
        [-1e-8, -0.02, -20.0, -713.0, -731.0, -1000.0, -1e10]
            .map(|lp| qnorm(lp, 0.0, 1.0, upper_log, &mut flags)),
    );
    for (actual, expected) in actual.into_iter().zip(out.split_whitespace()) {
        let expected: f64 = expected.parse().unwrap();
        assert!(
            (actual / expected - 1.0).abs() < 8e-15,
            "{actual} vs {expected}"
        );
    }
}

#[test]
fn ahrens_dieter_zero_uniform_fixup() {
    let Some(out) = oracle(
        r#"
        set.seed(42L, normal.kind = "Ahrens-Dieter")
        .Random.seed[2:4] <- c(1L, 0L, 0L)
        cat(sprintf("%a", rnorm(1L)), "\n")
        cat(.Random.seed, "\n")
    "#,
    ) else {
        return;
    };
    let mut words = Vec::new();
    Rng::seeded(
        42,
        Kinds {
            normal: NormalKind::AhrensDieter,
            ..Kinds::default()
        },
    )
    .encode(&mut words);
    words[1..4].copy_from_slice(&[1, 0, 0]);
    let mut rng = Rng::decode(&words).unwrap();
    let mut lines = out.lines();
    check_stream(lines.next().unwrap(), || rng.norm_rand());
    check_seed(&rng, lines.next().unwrap());
}

#[test]
fn restored_cursors_match_oracle() {
    let Some(out) = oracle(
        r#"
        for (kind in c("Mersenne-Twister", "Knuth-TAOCP", "Knuth-TAOCP-2002")) {
            set.seed(42L, kind)
            saved <- .Random.seed
            mt <- kind == "Mersenne-Twister"
            slot <- if (mt) 2L else 102L
            cursors <- if (mt) c(0L, 1L, 624L, 625L, 1000L) else c(-1L, 0L, 1L, 99L, 100L, 101L, 1000L)
            for (cursor in cursors) {
                .Random.seed <- saved
                .Random.seed[slot] <- cursor
                cat(sprintf("%a", runif(5L)), "\n")
                cat(.Random.seed, "\n")
            }
        }
    "#,
    ) else {
        return;
    };
    let mut lines = out.lines();
    for uniform in [
        UniformKind::MersenneTwister,
        UniformKind::KnuthTaocp,
        UniformKind::KnuthTaocp2002,
    ] {
        let mut words = Vec::new();
        Rng::seeded(
            42,
            Kinds {
                uniform,
                ..Kinds::default()
            },
        )
        .encode(&mut words);
        let (slot, cursors): (usize, &[i32]) = if uniform == UniformKind::MersenneTwister {
            (1, &[0, 1, 624, 625, 1000])
        } else {
            (101, &[-1, 0, 1, 99, 100, 101, 1000])
        };
        for &cursor in cursors {
            words[slot] = cursor;
            let mut rng = Rng::decode(&words).unwrap();
            check_stream(lines.next().unwrap(), || rng.unif_rand());
            check_seed(&rng, lines.next().unwrap());
        }
    }
    assert!(lines.next().is_none());
}
