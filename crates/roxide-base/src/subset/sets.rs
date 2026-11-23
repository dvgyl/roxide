use std::collections::{HashMap, HashSet};

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{CharId, RLogical, RSession, SEXP_NIL, SexpType, ValueApi, is_na_real};

use crate::ops::attributes::has_class;
use crate::ops::common::{atomic, element, error, flag, number, rank, vector_limit};

#[derive(Eq, Hash, PartialEq)]
enum Key {
    Missing,
    Code(i32),
    Bits(u64),
    Complex(u64, u64),
    Text(Vec<u8>),
}

fn bits(x: f64) -> u64 {
    if x == 0.0 {
        0
    } else if x.is_nan() {
        f64::NAN.to_bits()
    } else {
        x.to_bits()
    }
}

fn keys(s: &mut RSession, x: SexpId, ty: SexpType) -> EvalResult<Vec<Key>> {
    let (x, _) = s.coerce(x, ty)?;
    Ok(match ty {
        SexpType::Lgl => s
            .lgl(x)?
            .iter()
            .map(|v| v.get().map_or(Key::Missing, |v| Key::Code(i32::from(v))))
            .collect(),
        SexpType::Int => s.int(x)?.iter().map(Key::Code).collect(),
        SexpType::Real => s
            .real(x)?
            .iter()
            .map(|v| {
                if is_na_real(v) {
                    Key::Missing
                } else {
                    Key::Bits(bits(v))
                }
            })
            .collect(),
        SexpType::Cplx => s
            .cplx(x)?
            .iter()
            .map(|v| {
                if is_na_real(v.re) || is_na_real(v.im) {
                    Key::Missing
                } else {
                    Key::Complex(bits(v.re), bits(v.im))
                }
            })
            .collect(),
        SexpType::Raw => s.raw(x)?.iter().map(|v| Key::Code(i32::from(v))).collect(),
        _ => s
            .str_slice(x)?
            .iter()
            .map(|&id| {
                if id == CharId::NA {
                    Ok(Key::Missing)
                } else {
                    let bytes = s.char_bytes(id)?;
                    let bytes = if s.char_encoding(id)? == roxide_value::Encoding::Latin1 {
                        bytes
                            .iter()
                            .map(|&byte| char::from(byte))
                            .collect::<String>()
                            .into_bytes()
                    } else {
                        bytes.to_vec()
                    };
                    Ok(Key::Text(bytes))
                }
            })
            .collect::<Result<_, roxide_value::ValueError>>()?,
    })
}

fn factor_labels(ctx: &mut Ctx, x: SexpId, env: SexpId) -> EvalResult {
    if has_class(ctx.session(), x, "factor") {
        ctx.call_function("as.character", &[(None, x)], env)
    } else {
        Ok(x)
    }
}

pub(crate) fn match_values(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = factor_labels(ctx, args.get(0), call.env)?;
    ctx.protect(x);
    let table = factor_labels(ctx, args.get(1), call.env)?;
    ctx.protect(table);
    let no_match = number(ctx.session_mut(), args.get(2))?;
    let no_match =
        if no_match.is_finite() && no_match > i32::MIN as f64 && no_match <= i32::MAX as f64 {
            no_match as i32
        } else {
            roxide_value::NA_INTEGER
        };
    let n = ctx.session().xlength(x)?;
    vector_limit(ctx, call, n, size_of::<i32>())?;
    if n == 0 || ctx.session().xlength(table)? == 0 {
        return Ok(ctx.session_mut().alloc_int_from(&vec![no_match; n])?);
    }
    let s = ctx.session_mut();
    let tx = s.type_of(x)?;
    let tt = s.type_of(table)?;
    if (!atomic(tx) && !matches!(tx, SexpType::Vec | SexpType::Expr))
        || (!atomic(tt) && !matches!(tt, SexpType::Vec | SexpType::Expr))
    {
        return Err(error(ctx, call, "'match' requires vector arguments"));
    }
    let mixed_raw = tx != tt && (tx == SexpType::Raw || tt == SexpType::Raw);
    let ty =
        if !atomic(tx) || !atomic(tt) || tx == SexpType::Str || tt == SexpType::Str || mixed_raw {
            SexpType::Str
        } else if rank(tx) >= rank(tt) {
            tx
        } else {
            tt
        };
    let mut index = HashMap::new();
    for (i, key) in keys(s, table, ty)?.into_iter().enumerate() {
        index.entry(key).or_insert(i as i32 + 1);
    }
    let ignored: HashSet<_> = if args.get(3) == SEXP_NIL
        || s.lgl(args.get(3))
            .is_ok_and(|v| v.len() == 1 && v.get(0) == RLogical::FALSE)
    {
        HashSet::new()
    } else {
        keys(s, args.get(3), ty)?.into_iter().collect()
    };
    let out: Vec<_> = keys(s, x, ty)?
        .into_iter()
        .map(|key| {
            if ignored.contains(&key) {
                no_match
            } else {
                *index.get(&key).unwrap_or(&no_match)
            }
        })
        .collect();
    Ok(s.alloc_int_from(&out)?)
}

pub(crate) fn duplicated(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let from_last = flag(ctx, args.get(2), false)?;
    let first_only = flag(ctx, args.get(3), false)?;
    let n = ctx.session().xlength(x)?;
    vector_limit(ctx, call, n, size_of::<RLogical>())?;
    let order = (0..n).map(|i| if from_last { n - i - 1 } else { i });
    let mut out = vec![RLogical::FALSE; n];
    let mut first = 0;
    let s = ctx.session_mut();
    let ty = s.type_of(x)?;
    let ignored = args.get(1);
    let ignore_none = ignored == SEXP_NIL
        || s.lgl(ignored)
            .is_ok_and(|v| v.len() == 1 && v.get(0) == RLogical::FALSE);
    if atomic(ty) {
        let items = keys(s, x, ty)?;
        let ignored = if ignore_none {
            HashSet::new()
        } else {
            keys(s, ignored, ty)?.into_iter().collect()
        };
        let mut seen = HashSet::new();
        for i in order {
            if !ignored.contains(&items[i]) && !seen.insert(&items[i]) {
                out[i] = RLogical::TRUE;
                if first_only {
                    first = i + 1;
                    break;
                }
            }
        }
    } else if ty != SexpType::Nil {
        let mut seen = Vec::new();
        for i in order {
            let item = element(s, x, i)?;
            let mut duplicate = false;
            if !ignore_none {
                let mut skip = false;
                for j in 0..s.xlength(ignored)? {
                    let other = element(s, ignored, j)?;
                    if roxide_value::identical(s, item, other, Default::default())? {
                        skip = true;
                        break;
                    }
                }
                if skip {
                    continue;
                }
            }
            for &previous in &seen {
                if roxide_value::identical(s, item, previous, Default::default())? {
                    duplicate = true;
                    break;
                }
            }
            if duplicate {
                out[i] = RLogical::TRUE;
                if first_only {
                    first = i + 1;
                    break;
                }
            } else {
                seen.push(item);
            }
        }
    }
    if first_only {
        Ok(s.scalar_int(first as i32)?)
    } else {
        Ok(s.alloc_lgl_from(&out)?)
    }
}
