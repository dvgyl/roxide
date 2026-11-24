#![forbid(unsafe_code)]

use std::panic::catch_unwind;
use std::process::ExitCode;

use roxide_fuzz::{
    check_description, check_makeconf, check_r_syntax, check_rd_help, check_xdr_deserialize,
};

const MUTATIONS_PER_TARGET: usize = 2_000;
const RNG_SEED: u64 = 0x6d25_b412_e3cd_719a;

struct Target {
    name: &'static str,
    check: fn(&[u8]) -> Result<(), String>,
    seeds: &'static [(&'static [u8], bool)],
}

const TARGETS: &[Target] = &[
    Target {
        name: "parse",
        check: check_r_syntax,
        seeds: &[
            (include_bytes!("../../corpus/parse/pipe.R"), true),
            (include_bytes!("../../corpus/parse/incomplete.R"), false),
        ],
    },
    Target {
        name: "xdr",
        check: check_xdr_deserialize,
        seeds: &[
            (include_bytes!("../../corpus/xdr/v2-integer.xdr"), true),
            (include_bytes!("../../corpus/xdr/v3-null.xdr"), true),
            (include_bytes!("../../corpus/xdr/v3-truncated.xdr"), false),
        ],
    },
    Target {
        name: "makeconf",
        check: check_makeconf,
        seeds: &[
            (include_bytes!("../../corpus/makeconf/vars.mk"), true),
            (include_bytes!("../../corpus/makeconf/incomplete.mk"), false),
        ],
    },
    Target {
        name: "description",
        check: check_description,
        seeds: &[
            (include_bytes!("../../corpus/description/DESCRIPTION"), true),
            (
                include_bytes!("../../corpus/description/bad-continuation"),
                false,
            ),
        ],
    },
    Target {
        name: "rd",
        check: check_rd_help,
        seeds: &[
            (include_bytes!("../../corpus/rd/topic.Rd"), true),
            (include_bytes!("../../corpus/rd/deep.Rd"), false),
        ],
    },
];

fn main() -> ExitCode {
    let mut total = 0;
    for target in TARGETS {
        for (index, &(bytes, accepted)) in target.seeds.iter().enumerate() {
            let label = format!("seed {}", index + 1);
            if !run(target, bytes, &label, Some(accepted)) {
                return ExitCode::FAILURE;
            }
            total += 1;
        }

        let mut rng = Rng(RNG_SEED);
        for index in 0..MUTATIONS_PER_TARGET {
            let (seed, _) = target.seeds[index % target.seeds.len()];
            let bytes = mutate(seed, &mut rng);
            let label = format!("mutation {}", index + 1);
            if !run(target, &bytes, &label, None) {
                return ExitCode::FAILURE;
            }
            total += 1;
        }

        println!(
            "{} passed {} seeds and {MUTATIONS_PER_TARGET} mutations.",
            target.name,
            target.seeds.len()
        );
    }
    println!("Checked {total} inputs.");
    ExitCode::SUCCESS
}

fn run(target: &Target, bytes: &[u8], label: &str, accepted: Option<bool>) -> bool {
    let name = target.name;
    let Ok(result) = catch_unwind(|| (target.check)(bytes)) else {
        eprintln!("{name} panicked on {label} with RNG seed {RNG_SEED:#018x}. Input {bytes:02x?}");
        return false;
    };
    if let Some(accepted) = accepted
        && result.is_ok() != accepted
    {
        let expected = if accepted { "Ok" } else { "Err" };
        eprintln!("{name} returned {result:?} on {label}. Expected {expected}.");
        return false;
    }
    true
}

fn mutate(seed: &[u8], rng: &mut Rng) -> Vec<u8> {
    let mut bytes = seed.to_vec();
    let edits = 1 + rng.index(4);
    for _ in 0..edits {
        let operation = rng.index(5);
        if bytes.is_empty() || operation == 0 {
            let position = rng.index(bytes.len() + 1);
            bytes.insert(position, rng.byte());
            continue;
        }
        let position = rng.index(bytes.len());
        match operation {
            1 => bytes[position] = rng.byte(),
            2 => bytes[position] ^= 1 << rng.index(8),
            3 => {
                bytes.remove(position);
            }
            _ => bytes.truncate(position),
        }
    }
    bytes
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        self.0
    }

    fn index(&mut self, len: usize) -> usize {
        ((self.next() >> 32) % len as u64) as usize
    }

    fn byte(&mut self) -> u8 {
        (self.next() >> 56) as u8
    }
}
