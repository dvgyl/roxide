use roxide_gc::SexpId;
use roxide_value::na::R_NAN;
use roxide_value::{
    NA_INTEGER, NA_REAL, RLogical, RSession, SexpType, ValueApi, int_add, int_mul, int_sub,
    nan_propagate,
};

use super::ir::Op;
use crate::EvalResult;

#[derive(Clone, Copy)]
pub(super) enum Number {
    Integer(i32),
    Real(f64),
}

impl Number {
    pub fn real(self) -> f64 {
        match self {
            Self::Integer(NA_INTEGER) => NA_REAL,
            Self::Integer(n) => f64::from(n),
            Self::Real(n) => n,
        }
    }

    pub fn logical(self) -> RLogical {
        let n = self.real();
        RLogical::from_option((!n.is_nan()).then_some(n != 0.0))
    }

    pub fn alloc(self, session: &mut RSession) -> EvalResult {
        Ok(match self {
            Self::Integer(n) => session.scalar_int(n)?,
            Self::Real(n) => session.scalar_real(n)?,
        })
    }
}

pub(super) fn plain(session: &RSession, value: SexpId) -> bool {
    session.is_object(value) == Ok(false)
        && session.is_s4(value) == Ok(false)
        && session.attrs(value).is_ok_and(|attrs| attrs.is_empty())
}

pub(super) fn number(session: &RSession, value: SexpId) -> Option<Number> {
    if !plain(session, value) || session.xlength(value) != Ok(1) {
        return None;
    }
    Some(match session.type_of(value).ok()? {
        SexpType::Lgl => Number::Integer(
            session
                .lgl(value)
                .ok()?
                .get(0)
                .get()
                .map_or(NA_INTEGER, i32::from),
        ),
        SexpType::Int => Number::Integer(session.int(value).ok()?.get(0)),
        SexpType::Real => Number::Real(session.real(value).ok()?.get(0)),
        _ => return None,
    })
}

pub(super) fn binary(
    session: &mut RSession,
    op: &Op,
    lhs: Number,
    rhs: Number,
) -> EvalResult<(SexpId, bool)> {
    if let (Number::Integer(a), Number::Integer(b)) = (lhs, rhs) {
        let integer = match op {
            Op::Add(_) => Some(int_add(a, b)),
            Op::Sub(_) => Some(int_sub(a, b)),
            Op::Mul(_) => Some(int_mul(a, b)),
            _ => None,
        };
        if let Some((n, overflow)) = integer {
            return Ok((session.scalar_int(n)?, overflow));
        }
    }

    let (a, b) = (lhs.real(), rhs.real());
    let comparison = match op {
        Op::Lt(_) => Some(a < b),
        Op::Le(_) => Some(a <= b),
        Op::Gt(_) => Some(a > b),
        Op::Ge(_) => Some(a >= b),
        Op::Eq(_) => Some(a == b),
        Op::Ne(_) => Some(a != b),
        _ => None,
    };
    if let Some(test) = comparison {
        let result = RLogical::from_option((!a.is_nan() && !b.is_nan()).then_some(test));
        return Ok((session.scalar_lgl(result)?, false));
    }

    let n = match op {
        Op::Pow(_) if b == 0.0 || a == 1.0 => 1.0,
        _ if a.is_nan() || b.is_nan() => nan_propagate(a, b),
        Op::Add(_) => a + b,
        Op::Sub(_) => a - b,
        Op::Mul(_) => a * b,
        Op::Div(_) => a / b,
        Op::Pow(_) if a == 0.0 && b < 0.0 => f64::INFINITY,
        Op::Pow(_) if a.is_finite() && a < 0.0 && b.is_infinite() => R_NAN,
        Op::Pow(_) => a.powf(b),
        _ => return Err(super::exec::invalid()),
    };
    Ok((session.scalar_real(n)?, false))
}

pub(super) fn unary(session: &mut RSession, op: &Op, src: Number) -> EvalResult {
    match (op, src) {
        (Op::Not(_), src) => Ok(session.scalar_lgl(src.logical().not())?),
        (Op::Neg(_), Number::Integer(n)) => Number::Integer(n.wrapping_neg()).alloc(session),
        (Op::Neg(_), Number::Real(n)) => Number::Real(-n).alloc(session),
        _ => src.alloc(session),
    }
}

pub(super) fn short_operand(session: &RSession, value: SexpId) -> Option<RLogical> {
    if let Some(number) = number(session, value) {
        return Some(number.logical());
    }
    (matches!(
        session.type_of(value),
        Ok(SexpType::Lgl | SexpType::Int | SexpType::Real | SexpType::Cplx)
    ) && session.xlength(value) == Ok(0))
    .then_some(RLogical::NA)
}
