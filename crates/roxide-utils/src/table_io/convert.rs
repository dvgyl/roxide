use roxide_base::scan::{base_call, decimal, number, r_strings};
use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{CharId, NA_INTEGER, NA_REAL, RComplex, RLogical, ValueApi};

use super::{choice, flag};

#[derive(Clone, Copy)]
enum Numerals {
    Allow,
    Warn,
    Reject,
}

pub(super) struct Conversion {
    missing: Vec<Option<String>>,
    decimal: char,
    try_logical: bool,
    numerals: Numerals,
}

impl Conversion {
    pub(super) fn new(
        ctx: &mut Ctx,
        call: &Call,
        missing: SexpId,
        dec: SexpId,
        numerals: SexpId,
        try_logical: SexpId,
    ) -> EvalResult<Self> {
        let numerals = match choice(ctx, call, numerals, &["allow.loss", "warn.loss", "no.loss"])? {
            "warn.loss" => Numerals::Warn,
            "no.loss" => Numerals::Reject,
            _ => Numerals::Allow,
        };
        Ok(Self {
            missing: r_strings(ctx, missing)?,
            decimal: decimal(ctx, call, dec)?,
            try_logical: flag(ctx, try_logical)?.unwrap_or(false),
            numerals,
        })
    }

    pub(super) fn missing(&self) -> &[Option<String>] {
        &self.missing
    }

    pub(super) fn decimal(&self) -> char {
        self.decimal
    }

    pub(super) fn infer(
        &self,
        ctx: &mut Ctx,
        call: &Call,
        value: SexpId,
        as_is: bool,
    ) -> EvalResult {
        let strings = r_strings(ctx, value)?;
        let missing: Vec<_> = strings
            .iter()
            .map(Option::as_deref)
            .map(|x| x.filter(|v| !self.missing.iter().any(|m| m.as_deref() == Some(*v))))
            .collect();
        let candidate = || {
            missing
                .iter()
                .map(|x| x.filter(|v| !v.trim_ascii().is_empty()))
        };
        if self.try_logical
            && let Some(result) = candidate()
                .map(|x| match x {
                    None => Some(RLogical::NA),
                    Some(v) => number::logical(v, false).map(|v| RLogical::from_option(Some(v))),
                })
                .collect::<Option<Vec<_>>>()
        {
            return Ok(ctx.session_mut().alloc_lgl_from(&result)?);
        }
        if let Some(result) = candidate()
            .map(|x| match x {
                None => Some(NA_INTEGER),
                Some(v) => number::integer(v),
            })
            .collect::<Option<Vec<_>>>()
        {
            return Ok(ctx.session_mut().alloc_int_from(&result)?);
        }
        let mut doubles = Vec::with_capacity(strings.len());
        let mut try_complex = false;
        for x in candidate() {
            match x {
                None => doubles.push(NA_REAL),
                Some(x) => {
                    let Some((real, loss)) = number::real(x, self.decimal) else {
                        try_complex = number::complex(x, self.decimal).is_some();
                        break;
                    };
                    if loss && !self.precision(ctx, call, x)? {
                        break;
                    }
                    doubles.push(real);
                }
            }
        }
        if doubles.len() == strings.len() {
            return Ok(ctx.session_mut().alloc_real_from(&doubles)?);
        }
        drop(doubles);
        let mut complex = Vec::new();
        for x in candidate().take(if try_complex { strings.len() } else { 0 }) {
            match x {
                None => complex.push(RComplex::NA),
                Some(x) => {
                    let Some((value, losses)) = number::complex(x, self.decimal) else {
                        break;
                    };
                    let mut accepted = true;
                    for loss in losses.into_iter().flatten() {
                        accepted &= self.precision(ctx, call, loss)?;
                    }
                    if !accepted {
                        break;
                    }
                    complex.push(value);
                }
            }
        }
        if complex.len() == strings.len() {
            return Ok(ctx.session_mut().alloc_cplx_from(&complex)?);
        }
        let chars: Vec<_> = ctx
            .session()
            .str_slice(value)?
            .iter()
            .zip(missing)
            .map(|(&id, text)| if text.is_some() { id } else { CharId::NA })
            .collect();
        let chars = ctx.session_mut().alloc_str_from(&chars)?;
        if as_is {
            Ok(chars)
        } else {
            base_call(ctx, call, "factor", &[chars])
        }
    }

    fn precision(&self, ctx: &mut Ctx, call: &Call, token: &str) -> EvalResult<bool> {
        if matches!(self.numerals, Numerals::Warn) {
            ctx.warn_now(
                call.err_call(),
                format!("accuracy loss in conversion from \"{token}\" to numeric"),
            )?;
        }
        Ok(!matches!(self.numerals, Numerals::Reject))
    }
}

pub(super) fn type_convert(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let conversion = Conversion::new(
        ctx,
        call,
        args.get(1),
        args.get(3),
        args.get(4),
        args.get(5),
    )?;
    let as_is = flag(ctx, args.get(2))?.unwrap_or(false);
    conversion.infer(ctx, call, args.get(0), as_is)
}
