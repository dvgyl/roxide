use roxide_gc::SexpId;
use roxide_value::{CharId, NA_INTEGER, SEXP_NIL, SexpType, Sortedness, ValueApi, VecView};

use super::stack::ForCursor;
use crate::{Ctx, EvalResult};

impl ForCursor {
    pub fn new(ctx: &mut Ctx, value: SexpId, call: SexpId) -> EvalResult<Self> {
        let mut value = value;
        if ctx
            .classes(value, false)?
            .iter()
            .any(|name| name == "factor")
        {
            ctx.check_vector_len(ctx.session.xlength(value)?, size_of::<CharId>(), call)?;
            let levels = ctx.session.intern("levels")?;
            let labels = ctx.session.str_slice(ctx.session.attr(value, levels))?;
            let values: Vec<_> = ctx
                .session
                .int(value)?
                .iter()
                .map(|i| {
                    labels
                        .get(i.saturating_sub(1) as usize)
                        .copied()
                        .unwrap_or(CharId::NA)
                })
                .collect();
            value = ctx.session.alloc_str_from(&values)?;
        }
        match ctx.session.type_of(value)? {
            SexpType::Int => {
                if let VecView::Alt { class, heap, len } = ctx.session.int(value)?
                    && class.package_name() == "base"
                    && class.class_name() == "compact_intseq"
                    && len != 0
                {
                    return Ok(Self::Int {
                        next: i128::from(class.elt(heap, 0)),
                        step: if class.is_sorted(heap) == Sortedness::Decreasing {
                            -1
                        } else {
                            1
                        },
                        remaining: len,
                    });
                }
            }
            SexpType::Real => {
                if let VecView::Alt { class, heap, len } = ctx.session.real(value)?
                    && class.package_name() == "base"
                    && class.class_name() == "compact_realseq"
                    && len != 0
                {
                    return Ok(Self::Real {
                        start: class.elt(heap, 0),
                        step: if class.is_sorted(heap) == Sortedness::Decreasing {
                            -1.0
                        } else {
                            1.0
                        },
                        index: 0,
                        len,
                    });
                }
            }
            _ => {}
        }
        Ok(match ctx.session.type_of(value)? {
            SexpType::Nil => Self::Empty,
            SexpType::List => Self::Pairlist(value),
            SexpType::Vec
            | SexpType::Expr
            | SexpType::Int
            | SexpType::Real
            | SexpType::Lgl
            | SexpType::Cplx
            | SexpType::Raw
            | SexpType::Str => {
                let value = ctx.session.shallow_duplicate(value)?;
                Self::Vector {
                    value,
                    index: 0,
                    len: ctx.session.xlength(value)?,
                }
            }
            _ => return Err(ctx.stop(call, "invalid for() loop sequence")),
        })
    }

    pub fn advance(&mut self, ctx: &mut Ctx) -> EvalResult<Option<SexpId>> {
        Ok(Some(match self {
            Self::Int {
                next,
                step,
                remaining,
            } if *remaining != 0 => {
                let value = i32::try_from(*next).unwrap_or(NA_INTEGER);
                *next += i128::from(*step);
                *remaining -= 1;
                ctx.session.scalar_int(value)?
            }
            Self::Real {
                start,
                step,
                index,
                len,
            } if *index < *len => {
                let value = if *index == 0 {
                    *start
                } else {
                    *start + *index as f64 * *step
                };
                *index += 1;
                ctx.session.scalar_real(value)?
            }
            Self::Pairlist(tail) if *tail != SEXP_NIL => {
                let (value, next, _) = ctx.session.cons_parts(*tail)?;
                *tail = next;
                value
            }
            Self::Vector { value, index, len } if *index < *len => {
                let item = ctx.element(*value, *index)?;
                *index += 1;
                item
            }
            _ => return Ok(None),
        }))
    }
}
