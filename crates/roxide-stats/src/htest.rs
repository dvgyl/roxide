//! Classical hypothesis tests and multiple-testing adjustments.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod adjust;
mod ks;
mod print;
mod rank;
mod shapiro;
mod tables;

use roxide_eval::{Args, Call, Ctx, EvalResult, Module};
use roxide_gc::SexpId;
use roxide_math::core::Tail;
use roxide_value::{SEXP_NIL, ValueApi};

use crate::distn::support::{attr, internal, number};

/// Registration table for tests in the stats namespace.
pub const MODULE: Module = Module {
    prims: &[
        internal("C_htest_adjust", 3, adjust::adjust),
        internal("C_htest_shapiro", 1, shapiro::test),
        internal("C_htest_wilcox", 9, rank::wilcox),
        internal("C_htest_rank_cor", 6, rank::cor),
        internal("C_htest_ks", 6, ks::test),
        internal("C_htest_table", 2, tables::table),
        internal("C_htest_chisq", 4, tables::chisq),
        internal("C_htest_fisher", 7, tables::fisher),
        internal("C_htest_format", 3, print::format),
        internal("C_htest_wrap", 3, print::wrap),
        internal("C_htest_limit", 1, limit_input),
        internal("C_htest_frame_call", 1, frame_call),
    ],
    prelude: concat!(
        include_str!("htest/prelude.R"),
        include_str!("htest/proportions.R"),
        include_str!("htest/formula.R"),
        include_str!("htest/print.R"),
    ),
};

const LOWER: Tail = Tail {
    lower: true,
    log: false,
};
const UPPER: Tail = Tail {
    lower: false,
    log: false,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Alternative {
    TwoSided,
    Less,
    Greater,
}

impl Alternative {
    fn read(ctx: &mut Ctx, call: &Call, id: SexpId) -> EvalResult<Self> {
        match number(ctx, call, id)? {
            1.0 => Ok(Self::TwoSided),
            2.0 => Ok(Self::Less),
            3.0 => Ok(Self::Greater),
            _ => Err(ctx.stop(call.err_call(), "invalid alternative")),
        }
    }

    fn probability(self, lower: f64, upper: f64) -> f64 {
        match self {
            Self::TwoSided if lower.is_nan() || upper.is_nan() => f64::NAN,
            Self::TwoSided => (2.0 * lower.min(upper)).min(1.0),
            Self::Less => lower,
            Self::Greater => upper,
        }
    }

    fn alpha(self, level: f64) -> f64 {
        (1.0 - level) / if self == Self::TwoSided { 2.0 } else { 1.0 }
    }
}

fn limit_input(ctx: &mut Ctx, call: &Call, args: &mut roxide_eval::Args) -> EvalResult {
    let input = args.get(0);
    crate::distn::support::limit::<f64>(ctx, call, ctx.session().xlength(input)?)?;
    Ok(input)
}

// Construct a model.frame call with argument tags and unevaluated expressions.
fn frame_call(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let input = args.get(0);
    let names = attr(ctx, input, "names")?;
    let count = ctx.session().xlength(input)?;
    if count > 4 || ctx.session().xlength(names)? != count {
        return Err(ctx.stop(call.err_call(), "invalid model frame arguments"));
    }
    let labels = ctx.session().str_slice(names)?.to_vec();
    let values = ctx.session().list_slice(input)?.to_vec();
    ctx.scoped(|ctx| {
        let mut tail = SEXP_NIL;
        for (&label, &value) in labels.iter().zip(&values).rev() {
            let name = String::from_utf8_lossy(ctx.session().char_bytes(label)?).into_owned();
            let tag = ctx.session_mut().intern(&name)?.as_sexp();
            tail = ctx.session_mut().cons(value, tail, tag)?;
            ctx.protect(tail);
        }
        let head = ctx.session_mut().intern("model.frame")?.as_sexp();
        Ok(ctx.session_mut().lcons(head, tail, SEXP_NIL)?)
    })
}
