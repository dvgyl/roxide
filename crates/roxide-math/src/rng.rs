//! Uniform, normal, and exponential random streams and sampling.

#![deny(missing_docs)]

mod normal;
mod sample;
mod uniform;

pub use uniform::UniformState;

macro_rules! kinds {
    ($(#[$meta:meta])* pub enum $kind:ident { $($(#[$doc:meta])* $name:ident = $id:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $kind {
            $($(#[$doc])* $name = $id),+
        }

        impl $kind {
            fn decode(value: i32) -> Option<Self> {
                match value {
                    $($id => Some(Self::$name),)+
                    _ => None,
                }
            }
        }
    };
}

kinds! {
    /// Uniform generator codes in `.Random.seed`.
    pub enum UniformKind {
        /// Wichmann-Hill combined generator.
        WichmannHill = 0,
        /// Marsaglia multiply-with-carry generator.
        MarsagliaMulticarry = 1,
        /// Super-Duper generator.
        SuperDuper = 2,
        /// MT19937.
        MersenneTwister = 3,
        /// Knuth's 1997 subtractive generator.
        KnuthTaocp = 4,
        /// External generator code.
        UserSupplied = 5,
        /// Knuth's revised subtractive generator.
        KnuthTaocp2002 = 6,
        /// L'Ecuyer's combined multiple recursive generator.
        LecuyerCmrg = 7,
    }
}

kinds! {
    /// Normal generator codes in `.Random.seed`.
    pub enum NormalKind {
        /// Original Kinderman-Ramage variant.
        BuggyKindermanRamage = 0,
        /// Ahrens-Dieter algorithm FL.
        AhrensDieter = 1,
        /// Box-Muller transform.
        BoxMuller = 2,
        /// External normal generator code.
        UserNorm = 3,
        /// Inverse normal distribution.
        Inversion = 4,
        /// Corrected Kinderman-Ramage algorithm.
        KindermanRamage = 5,
    }
}

kinds! {
    /// Index sampling codes in `.Random.seed`.
    pub enum SampleKind {
        /// Scale and truncate a uniform value.
        Rounding = 0,
        /// Draw bits and reject out-of-range indices.
        Rejection = 1,
    }
}

/// The three algorithms selected for a random stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kinds {
    /// Uniform generator.
    pub uniform: UniformKind,
    /// Normal generator.
    pub normal: NormalKind,
    /// Index sampler.
    pub sample: SampleKind,
}

impl Default for Kinds {
    fn default() -> Self {
        Self {
            uniform: UniformKind::MersenneTwister,
            normal: NormalKind::Inversion,
            sample: SampleKind::Rejection,
        }
    }
}

impl Kinds {
    /// Read the algorithm codes from a seed header.
    pub fn decode(header: i32) -> Result<Self, SeedError> {
        if !(0..=11000).contains(&header) {
            return Err(SeedError::NaKind);
        }
        let uniform = UniformKind::decode(header % 100).ok_or(SeedError::InvalidKind)?;
        let normal = NormalKind::decode(header / 100 % 100).ok_or(SeedError::InvalidKind)?;
        let sample = SampleKind::decode(header / 10000).ok_or(SeedError::InvalidKind)?;
        if uniform == UniformKind::UserSupplied {
            return Err(SeedError::UserSupplied);
        }
        Ok(Self {
            uniform,
            normal,
            sample,
        })
    }

    fn header(self) -> i32 {
        self.uniform as i32 + 100 * self.normal as i32 + 10000 * self.sample as i32
    }
}

/// Reason why a saved seed could not be restored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeedError {
    /// The seed has the specified non-integer R type.
    NotInteger(&'static str),
    /// The header is absent, missing, or outside the permitted range.
    NaKind,
    /// The header contains an unknown algorithm code.
    InvalidKind,
    /// The vector has too few words for its algorithm.
    WrongLength,
    /// No external uniform generator is installed.
    UserSupplied,
}

/// Random stream and its non-serialized normal spare.
#[derive(Clone, Debug)]
pub struct Rng {
    /// Uniform generator state.
    pub state: UniformState,
    /// Selected algorithms.
    pub kinds: Kinds,
    /// Spare from the Box-Muller transform.
    pub bm_saved: Option<f64>,
}

impl Rng {
    /// Restore a random stream from integer seed words.
    pub fn decode(seed: &[i32]) -> Result<Self, SeedError> {
        Self::decode_with_seed(seed, 1)
    }

    /// Restore a stream and use `fallback` to replace an unusable generator state.
    pub fn decode_with_seed(seed: &[i32], fallback: u32) -> Result<Self, SeedError> {
        let (&header, words) = seed.split_first().ok_or(SeedError::NaKind)?;
        let kinds = Kinds::decode(header)?;
        let state = UniformState::decode(kinds.uniform, words, fallback)?;
        Ok(Self {
            state,
            kinds,
            bm_saved: None,
        })
    }

    /// Replace `out` with the integer representation of this stream.
    pub fn encode(&self, out: &mut Vec<i32>) {
        out.clear();
        out.push(self.kinds.header());
        self.state.encode(out);
    }

    /// Initialize a stream from the raw bits of a seed.
    pub fn seeded(seed: u32, kinds: Kinds) -> Self {
        Self {
            state: UniformState::seeded(seed, kinds.uniform),
            kinds,
            bm_saved: None,
        }
    }

    /// Draw a seed from the current uniform stream and initialize the selected stream.
    pub fn switch_uniform(&mut self, to: UniformKind) {
        let seed = (self.unif_rand() * f64::from(u32::MAX)) as u32;
        self.state = UniformState::seeded(seed, to);
        self.kinds.uniform = to;
    }

    /// Draw one uniform value between zero and one.
    pub fn unif_rand(&mut self) -> f64 {
        self.state.next()
    }
}
