use std::io::{self, BufReader, Read};
use std::path::Path;

use roxide_eval::{Args, Call, Ctx, EvalResult, Module};
use roxide_host::{HostErrorKind, OpenMode};
use roxide_value::{CharId, Encoding, SexpType, ValueApi};

use super::tools_rd::{internal, set_attr};

pub(super) const MODULE: Module = Module {
    prims: &[internal("rox_tools_md5sum", 2, md5sum)],
    prelude: r#"
md5sum <- function(files, bytes) {
    if (!missing(bytes)) {
        if (!missing(files)) stop("'files' and 'bytes' are mutually exclusive")
        if (!is.raw(bytes)) stop("'bytes' must be a raw vector")
        return(.Internal(rox_tools_md5sum(NULL, bytes)))
    }
    if (!is.character(files)) stop("invalid 'path' argument")
    .Internal(rox_tools_md5sum(path.expand(files), NULL))
}
"#,
};

fn md5sum(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let bytes = args.get(1);
    if ctx.session().type_of(bytes)? == SexpType::Raw {
        let raw = ctx.session().raw(bytes)?;
        let hash = if let Some(slice) = raw.as_slice() {
            digest(slice)
        } else {
            digest(raw.to_vec().as_slice())
        };
        let hash = hash.map_err(|error| ctx.stop(call.err_call(), error.to_string()))?;
        return Ok(ctx.session_mut().scalar_str(&hash)?);
    }

    let files = args.get(0);
    let paths = ctx.session().str_slice(files)?.to_vec();
    let mut hashes = Vec::with_capacity(paths.len());
    for id in paths {
        ctx.check_user_interrupt()?;
        if id == CharId::NA {
            hashes.push(CharId::NA);
            continue;
        }
        let path = String::from_utf8_lossy(ctx.session().char_bytes(id)?).into_owned();
        if path.is_empty() {
            hashes.push(CharId::NA);
            continue;
        }
        let file = Path::new(&path);
        let result = match ctx.host().metadata(file, true) {
            Ok(meta) if meta.is_file => ctx
                .host()
                .open(file, OpenMode::read_only())
                .ok()
                .and_then(|file| digest(BufReader::new(file)).ok()),
            Err(error) if error.kind == HostErrorKind::NotFound => {
                hashes.push(CharId::NA);
                continue;
            }
            _ => None,
        };
        let id = match result {
            Some(hash) => {
                let id = ctx
                    .session_mut()
                    .mk_char(hash.as_bytes(), Encoding::Native)?;
                ctx.protect(id.as_sexp());
                id
            }
            None => {
                ctx.warn_now(call.err_call(), format!("md5 failed on file '{path}'"))?;
                CharId::NA
            }
        };
        hashes.push(id);
    }
    let answer = ctx.session_mut().alloc_str_from(&hashes)?;
    ctx.protect(answer);
    set_attr(ctx, answer, "names", files)?;
    Ok(answer)
}

fn digest(mut input: impl Read) -> io::Result<String> {
    let mut state = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476];
    let mut block = [0; 64];
    let mut length = 0u64;
    let end = loop {
        let mut used = 0;
        while used < block.len() {
            match input.read(&mut block[used..]) {
                Ok(0) => break,
                Ok(n) => used += n,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
        }
        length = length.wrapping_add(used as u64);
        if used < block.len() {
            break used;
        }
        compress(&mut state, &block);
    };

    block[end..].fill(0);
    block[end] = 0x80;
    if end >= 56 {
        compress(&mut state, &block);
        block.fill(0);
    }
    block[56..].copy_from_slice(&length.wrapping_mul(8).to_le_bytes());
    compress(&mut state, &block);

    Ok(state
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn compress(state: &mut [u32; 4], block: &[u8; 64]) {
    const SHIFTS: [[u32; 4]; 4] = [
        [7, 12, 17, 22],
        [5, 9, 14, 20],
        [4, 11, 16, 23],
        [6, 10, 15, 21],
    ];
    let mut words = [0; 16];
    for (word, bytes) in words.iter_mut().zip(block.as_chunks::<4>().0) {
        *word = u32::from_le_bytes(*bytes);
    }
    let [mut a, mut b, mut c, mut d] = *state;
    for (step, constant) in SINE_TABLE.into_iter().enumerate() {
        let round = step / 16;
        let (mix, word) = match round {
            0 => ((b & c) | (!b & d), step),
            1 => ((b & d) | (c & !d), (5 * step + 1) % 16),
            2 => (b ^ c ^ d, (3 * step + 5) % 16),
            _ => (c ^ (b | !d), (7 * step) % 16),
        };
        let sum = a
            .wrapping_add(mix)
            .wrapping_add(constant)
            .wrapping_add(words[word]);
        [a, b, c, d] = [
            d,
            b.wrapping_add(sum.rotate_left(SHIFTS[round][step % 4])),
            b,
            c,
        ];
    }
    for (word, added) in state.iter_mut().zip([a, b, c, d]) {
        *word = word.wrapping_add(added);
    }
}

// RFC 1321 defines this table as floor(2^32 * abs(sin(i))) for i = 1 to 64.
const SINE_TABLE: [u32; 64] = [
    0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
    0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
    0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
    0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
    0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
    0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
    0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
    0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
];
