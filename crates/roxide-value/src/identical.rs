//! Compare R values with the options from `identical()`.

use std::collections::HashSet;

use roxide_gc::SexpId;

use crate::api::{ValueApi, ValueError};
use crate::attributes::compact_row_count;
use crate::na::is_na_real;
use crate::object::{Body, CharId, Encoding, SexpType};
use crate::session::RSession;

/// Controls how [`identical`] compares R values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdenticalOpts {
    /// Compare numbers by value, with equal positive and negative zeros.
    pub num_eq: bool,
    /// Compare missing numbers by category. Keep NA and NaN separate.
    pub single_na: bool,
    /// Compare attributes by name, regardless of their order.
    pub attrib_as_set: bool,
    /// Ignore compiled bytecode when comparing closures.
    pub ignore_bytecode: bool,
    /// Ignore enclosing environments when comparing closures.
    pub ignore_environment: bool,
    /// Ignore source references in closures and their bodies.
    pub ignore_srcref: bool,
}

impl Default for IdenticalOpts {
    fn default() -> Self {
        Self {
            num_eq: true,
            single_na: true,
            attrib_as_set: true,
            ignore_bytecode: true,
            ignore_environment: false,
            ignore_srcref: true,
        }
    }
}

/// Compare two R values without evaluating them or materializing ALTREP vectors.
pub fn identical(
    session: &RSession,
    x: SexpId,
    y: SexpId,
    opts: IdenticalOpts,
) -> Result<bool, ValueError> {
    let mut pending = vec![(x, y, false)];
    let mut visited = HashSet::new();
    while let Some((x, y, in_closure_body)) = pending.pop() {
        let a = session.heap.get(x)?;
        let b = session.heap.get(y)?;
        if x == y || !visited.insert((x, y, in_closure_body)) {
            continue;
        }
        let ty = a.sexp_type();
        if ty != b.sexp_type() || a.flags != b.flags {
            return Ok(false);
        }

        let keep = |&(sym, _): &(crate::Sym, SexpId)| {
            if !opts.ignore_srcref {
                return true;
            }
            match session.sym_name(sym) {
                Ok("srcref") => ty != SexpType::Clo && !in_closure_body,
                Ok("wholeSrcref" | "srcfile") => !in_closure_body,
                _ => true,
            }
        };
        let mut ax: Vec<_> = a.attrs.iter().filter(keep).collect();
        let mut ay: Vec<_> = b.attrs.iter().filter(keep).collect();
        if ax.len() != ay.len() {
            return Ok(false);
        }
        if opts.attrib_as_set {
            ax.sort_unstable_by_key(|(sym, _)| sym.as_sexp().to_bits());
            ay.sort_unstable_by_key(|(sym, _)| sym.as_sexp().to_bits());
        }
        for ((kx, vx), (ky, vy)) in ax.into_iter().zip(ay) {
            if kx != ky {
                return Ok(false);
            }
            if session.sym_name(kx)? == "row.names"
                && let Some(equal) = equal_row_names(session, vx, vy)?
            {
                if !equal {
                    return Ok(false);
                }
                continue;
            }
            pending.push((vx, vy, false));
        }

        macro_rules! equal_vector {
            ($view:ident, $equal:expr) => {{
                let a = session.$view(x)?;
                let b = session.$view(y)?;
                a.len() == b.len() && a.iter().zip(b.iter()).all($equal)
            }};
        }

        let equal = match ty {
            SexpType::Lgl => equal_vector!(lgl, |(a, b)| a.get() == b.get()),
            SexpType::Int => equal_vector!(int, |(a, b)| a == b),
            SexpType::Real => equal_vector!(real, |(a, b)| equal_number(a, b, opts)),
            SexpType::Cplx => equal_vector!(cplx, |(a, b)| {
                equal_number(a.re, b.re, opts) && equal_number(a.im, b.im, opts)
            }),
            SexpType::Raw => equal_vector!(raw, |(a, b)| a == b),
            SexpType::Char => equal_char(session, CharId(x), CharId(y))?,
            SexpType::Str => {
                let a = session.str_slice(x)?;
                let b = session.str_slice(y)?;
                if a.len() != b.len() {
                    return Ok(false);
                }
                for (&a, &b) in a.iter().zip(b) {
                    if !equal_char(session, a, b)? {
                        return Ok(false);
                    }
                }
                true
            }
            SexpType::Vec | SexpType::Expr => {
                let a = session.list_slice(x)?;
                let b = session.list_slice(y)?;
                if a.len() != b.len() {
                    return Ok(false);
                }
                pending.extend(a.iter().zip(b).map(|(&x, &y)| (x, y, in_closure_body)));
                true
            }
            _ => match (&a.body, &b.body) {
                (Body::List(a), Body::List(b))
                | (Body::Lang(a), Body::Lang(b))
                | (Body::Dots(a), Body::Dots(b)) => {
                    pending.extend([
                        (a.car, b.car, in_closure_body),
                        (a.cdr, b.cdr, in_closure_body),
                        (a.tag, b.tag, false),
                    ]);
                    true
                }
                (
                    Body::Closure {
                        formals: af,
                        body: ab,
                        env: ae,
                    },
                    Body::Closure {
                        formals: bf,
                        body: bb,
                        env: be,
                    },
                ) => {
                    pending.extend([(*af, *bf, false), (*ab, *bb, true)]);
                    opts.ignore_environment || ae == be
                }
                (Body::Builtin(a), Body::Builtin(b)) | (Body::Special(a), Body::Special(b)) => {
                    a == b
                }
                (Body::ExtPtr(a), Body::ExtPtr(b)) => a.addr == b.addr,
                (Body::Nil, Body::Nil) | (Body::Obj, Body::Obj) => true,
                _ => false,
            },
        };
        if !equal {
            return Ok(false);
        }
    }
    Ok(true)
}

fn equal_number(a: f64, b: f64, opts: IdenticalOpts) -> bool {
    a.to_bits() == b.to_bits()
        || if a.is_nan() && b.is_nan() {
            opts.single_na && is_na_real(a) == is_na_real(b)
        } else {
            opts.num_eq && a == b
        }
}

fn equal_char(session: &RSession, a: CharId, b: CharId) -> Result<bool, ValueError> {
    if a == b {
        return Ok(true);
    }
    if a == CharId::NA || b == CharId::NA {
        return Ok(false);
    }
    let ae = session.char_encoding(a)?;
    let be = session.char_encoding(b)?;
    let av = session.char_bytes(a)?;
    let bv = session.char_bytes(b)?;
    if ae == be {
        return Ok(av == bv);
    }
    if ae == Encoding::Bytes || be == Encoding::Bytes {
        return Ok(false);
    }
    let text = |encoding, bytes: &[u8]| match encoding {
        Encoding::Latin1 => bytes.iter().copied().map(char::from).collect(),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    };
    Ok(text(ae, av) == text(be, bv))
}

fn equal_row_names(session: &RSession, x: SexpId, y: SexpId) -> Result<Option<bool>, ValueError> {
    match (
        compact_row_count(session, x)?,
        compact_row_count(session, y)?,
    ) {
        (Some(a), Some(b)) => Ok(Some(a == b)),
        (Some(n), None) => expanded_rows_equal(session, n, y).map(Some),
        (None, Some(n)) => expanded_rows_equal(session, n, x).map(Some),
        (None, None) => Ok(None),
    }
}

fn expanded_rows_equal(session: &RSession, count: u32, y: SexpId) -> Result<bool, ValueError> {
    if session.type_of(y)? != SexpType::Int {
        return Ok(false);
    }
    let values = session.int(y)?;
    Ok(values.len() == count as usize
        && values
            .iter()
            .zip(1..=count)
            .all(|(value, index)| value == index as i32))
}
