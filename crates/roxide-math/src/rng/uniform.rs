use super::{SeedError, UniformKind};

const MT_SIZE: usize = 624;
const KNUTH_SIZE: usize = 100;
const KNUTH_MASK: u32 = (1 << 30) - 1;
const LE_M1: i64 = 4_294_967_087;
const LE_M2: i64 = 4_294_944_443;
const WH_MODULI: [u32; 3] = [30269, 30307, 30323];
const U32_SCALE: f64 = 1.0 / 4_294_967_296.0;
const WORD_SCALE: f64 = f64::from_bits(0x3df0_0000_000f_ffff);
const KNUTH_SCALE: f64 = f64::from_bits(0x3e10_0000_0000_0002);
const ZERO_FIXUP: f64 = WORD_SCALE * 0.5;

/// Opaque state for a uniform generator.
#[derive(Clone, Debug)]
pub struct UniformState(State);

#[derive(Clone, Debug)]
enum State {
    WichmannHill([u32; 3]),
    Marsaglia([u32; 2]),
    SuperDuper([u32; 2]),
    MersenneTwister(Box<[u32; MT_SIZE]>, usize),
    Knuth(Box<[u32; KNUTH_SIZE]>, usize),
    Lecuyer([u32; 6]),
    External,
}

fn scramble(word: u32) -> u32 {
    word.wrapping_mul(69069).wrapping_add(1)
}

impl UniformState {
    pub(super) fn seeded(seed: u32, kind: UniformKind) -> Self {
        let mut word = seed;
        for _ in 0..50 {
            word = scramble(word);
        }
        let mut next = || {
            word = scramble(word);
            word
        };
        let state = match kind {
            UniformKind::WichmannHill => {
                State::WichmannHill(WH_MODULI.map(|modulus| (next() % modulus).max(1)))
            }
            UniformKind::MarsagliaMulticarry => State::Marsaglia([next().max(1), next().max(1)]),
            UniformKind::SuperDuper => State::SuperDuper([next().max(1), next() | 1]),
            UniformKind::MersenneTwister => {
                next();
                State::MersenneTwister(Box::new(std::array::from_fn(|_| next())), MT_SIZE)
            }
            UniformKind::KnuthTaocp | UniformKind::KnuthTaocp2002 => State::Knuth(
                Box::new(knuth_init(
                    word % 1_073_741_821,
                    kind == UniformKind::KnuthTaocp2002,
                )),
                KNUTH_SIZE,
            ),
            UniformKind::LecuyerCmrg => State::Lecuyer(std::array::from_fn(|_| {
                let mut value = next();
                while i64::from(value) >= LE_M2 {
                    value = next();
                }
                value
            })),
            UniformKind::UserSupplied => State::External,
        };
        Self(state)
    }

    pub(super) fn decode(
        kind: UniformKind,
        words: &[i32],
        fallback: u32,
    ) -> Result<Self, SeedError> {
        fn take<const N: usize>(words: &[i32]) -> Result<[u32; N], SeedError> {
            let values: &[i32; N] = words.first_chunk().ok_or(SeedError::WrongLength)?;
            Ok(values.map(|word| word as u32))
        }
        let state = match kind {
            UniformKind::WichmannHill => {
                let mut values = take::<3>(words)?;
                for (word, modulus) in values.iter_mut().zip(WH_MODULI) {
                    *word = (*word % modulus).max(1);
                }
                State::WichmannHill(values)
            }
            UniformKind::MarsagliaMulticarry => {
                State::Marsaglia(take::<2>(words)?.map(|x| x.max(1)))
            }
            UniformKind::SuperDuper => {
                let [a, b] = take::<2>(words)?;
                State::SuperDuper([a.max(1), b | 1])
            }
            UniformKind::MersenneTwister => {
                let (&cursor, rest) = words.split_first().ok_or(SeedError::WrongLength)?;
                let values = take(rest)?;
                if values.iter().all(|&word| word == 0) {
                    return Ok(Self::seeded(fallback, kind));
                }
                let cursor = if cursor <= 0 {
                    MT_SIZE
                } else {
                    cursor as usize
                };
                State::MersenneTwister(Box::new(values), cursor)
            }
            UniformKind::KnuthTaocp | UniformKind::KnuthTaocp2002 => {
                let values = take::<KNUTH_SIZE>(words)?.map(|word| word & KNUTH_MASK);
                let cursor = *words.get(KNUTH_SIZE).ok_or(SeedError::WrongLength)?;
                if values.iter().all(|&word| word == 0) {
                    return Ok(Self::seeded(fallback, kind));
                }
                State::Knuth(
                    Box::new(values),
                    if cursor <= 0 {
                        KNUTH_SIZE
                    } else {
                        cursor as usize
                    },
                )
            }
            UniformKind::LecuyerCmrg => {
                let values = take::<6>(words)?;
                if values[..3].iter().all(|&word| word == 0)
                    || values
                        .iter()
                        .zip([LE_M1; 3].into_iter().chain([LE_M2; 3]))
                        .any(|(&word, limit)| i64::from(word) >= limit)
                {
                    return Ok(Self::seeded(fallback, kind));
                }
                State::Lecuyer(values)
            }
            UniformKind::UserSupplied => return Err(SeedError::UserSupplied),
        };
        Ok(Self(state))
    }

    pub(super) fn encode(&self, out: &mut Vec<i32>) {
        let words: &[u32] = match &self.0 {
            State::WichmannHill(words) => words,
            State::Marsaglia(words) | State::SuperDuper(words) => words,
            State::MersenneTwister(words, cursor) => {
                out.push(*cursor as i32);
                words.as_slice()
            }
            State::Knuth(words, cursor) => {
                out.extend(words.iter().map(|&word| word as i32));
                out.push(*cursor as i32);
                return;
            }
            State::Lecuyer(words) => words,
            State::External => &[],
        };
        out.extend(words.iter().map(|&word| word as i32));
    }

    pub(super) fn next(&mut self) -> f64 {
        let value = match &mut self.0 {
            State::WichmannHill(words) => {
                for ((word, multiplier), modulus) in
                    words.iter_mut().zip([171, 172, 170]).zip(WH_MODULI)
                {
                    *word = (*word * multiplier) % modulus;
                }
                let [a, b, c] = *words;
                (f64::from(a) / 30269.0 + f64::from(b) / 30307.0 + f64::from(c) / 30323.0).fract()
            }
            State::Marsaglia([a, b]) => {
                *a = 36969 * (*a & 65535) + (*a >> 16);
                *b = 18000 * (*b & 65535) + (*b >> 16);
                f64::from((*a << 16) | (*b & 65535)) * WORD_SCALE
            }
            State::SuperDuper([a, b]) => {
                *a ^= *a >> 15;
                *a ^= *a << 17;
                *b = b.wrapping_mul(69069);
                f64::from(*a ^ *b) * WORD_SCALE
            }
            State::MersenneTwister(words, cursor) => {
                if *cursor == MT_SIZE + 1 {
                    // This cursor is the default-initialization marker for MT19937.
                    let mut seed = 4357;
                    for word in words.iter_mut() {
                        let high = seed & 0xffff_0000;
                        seed = scramble(seed);
                        *word = high | (seed >> 16);
                        seed = scramble(seed);
                    }
                }
                if *cursor >= MT_SIZE {
                    for i in 0..MT_SIZE {
                        let joined =
                            (words[i] & 0x8000_0000) | (words[(i + 1) % MT_SIZE] & 0x7fff_ffff);
                        let odd_mask = 0u32.wrapping_sub(joined & 1);
                        words[i] =
                            words[(i + 397) % MT_SIZE] ^ (joined >> 1) ^ (odd_mask & 0x9908_b0df);
                    }
                    *cursor = 0;
                }
                let mut value = words[*cursor];
                *cursor += 1;
                value ^= value >> 11;
                value ^= (value << 7) & 0x9d2c_5680;
                value ^= (value << 15) & 0xefc6_0000;
                value ^= value >> 18;
                f64::from(value) * U32_SCALE
            }
            State::Knuth(words, cursor) => {
                if *cursor >= KNUTH_SIZE {
                    knuth_advance::<1109>(words);
                    *cursor = 0;
                }
                let value = words[*cursor];
                *cursor += 1;
                f64::from(value) * KNUTH_SCALE
            }
            State::Lecuyer(words) => {
                let [x0, x1, x2, y0, y1, y2] = words.map(i64::from);
                let h = (1_403_580 * x1 - 810_728 * x0).rem_euclid(LE_M1);
                let l = (527_612 * y2 - 1_370_589 * y0).rem_euclid(LE_M2);
                *words = [
                    x1 as u32, x2 as u32, h as u32, y1 as u32, y2 as u32, l as u32,
                ];
                let distance = if h > l { h - l } else { h - l + LE_M1 };
                distance as f64 * (1.0 / 4_294_967_088.0)
            }
            State::External => return f64::NAN,
        };
        if value <= 0.0 {
            ZERO_FIXUP
        } else if value >= 1.0 {
            1.0 - ZERO_FIXUP
        } else {
            value
        }
    }
}

fn knuth_advance<const N: usize>(words: &mut [u32; KNUTH_SIZE]) {
    let mut sequence = [0u32; N];
    sequence[..KNUTH_SIZE].copy_from_slice(words);
    for i in KNUTH_SIZE..N {
        sequence[i] = sequence[i - 100].wrapping_sub(sequence[i - 37]) & KNUTH_MASK;
    }
    words.copy_from_slice(&sequence[N - KNUTH_SIZE..]);
}

fn knuth_init(seed: u32, revised: bool) -> [u32; KNUTH_SIZE] {
    let mut polynomial = [0u32; 199];
    let mut word = (seed + 2) & (KNUTH_MASK - 1);
    for coeff in &mut polynomial[..100] {
        *coeff = word;
        word <<= 1;
        if word > (1 << 30) || (revised && word == 1 << 30) {
            word -= KNUTH_MASK - 1;
        }
    }
    polynomial[1] += 1;
    let mut bits = seed;
    let mut separation = 69;
    while separation > 0 {
        for i in (1..100).rev() {
            polynomial[2 * i] = polynomial[i];
            if revised {
                polynomial[2 * i - 1] = 0;
            }
        }
        if !revised {
            for i in 0..68 {
                polynomial[2 * i + 1] = polynomial[198 - 2 * i] & !1;
            }
        }
        for i in (100..199).rev() {
            let coeff = polynomial[i];
            if revised || coeff & 1 != 0 {
                polynomial[i - 63] = polynomial[i - 63].wrapping_sub(coeff) & KNUTH_MASK;
                polynomial[i - 100] = polynomial[i - 100].wrapping_sub(coeff) & KNUTH_MASK;
            }
        }
        if bits & 1 != 0 {
            polynomial.copy_within(..100, 1);
            polynomial[0] = polynomial[100];
            if revised || polynomial[100] & 1 != 0 {
                polynomial[37] = polynomial[37].wrapping_sub(polynomial[100]) & KNUTH_MASK;
            }
        }
        if bits == 0 {
            separation -= 1;
        } else {
            bits >>= 1;
        }
    }
    let mut words = std::array::from_fn(|i| polynomial[(i + 37) % 100]);
    if revised {
        for _ in 0..10 {
            knuth_advance::<299>(&mut words);
        }
    }
    words
}
