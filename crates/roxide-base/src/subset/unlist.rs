use std::collections::HashMap;

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{CharId, NA_INTEGER, SEXP_NIL, SexpType, ValueApi};

use crate::ops::attributes::has_class;
use crate::ops::common::{
    attr, ch, concatenate, element, error, flag, names, rank, set, set_names, text, vector_limit,
};

#[derive(Default)]
struct Group {
    prefix: String,
    start: usize,
    bare: usize,
    missing: bool,
}

struct Flat {
    groups: Vec<Group>,
    leaves: Vec<(SexpId, usize, Option<SexpId>)>,
    kind: SexpType,
    recursive: bool,
    named: bool,
    levels: Vec<SexpId>,
    all_factors: bool,
}

pub(crate) fn unlist(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    if !container(ctx.session().type_of(x)?) {
        return Ok(x);
    }
    ctx.resolve_altrep_deep(x)?;
    let mut flat = Flat {
        groups: vec![Group::default()],
        leaves: Vec::new(),
        kind: SexpType::Nil,
        named: flag(ctx, args.get(2), true)?,
        recursive: flag(ctx, args.get(1), true)?,
        levels: Vec::new(),
        all_factors: true,
    };
    flat.walk(ctx, call, x, 0, 0)?;
    if flat.kind == SexpType::Nil {
        return Ok(SEXP_NIL);
    }
    let mut values = Vec::with_capacity(flat.leaves.len());
    let mut labels = Vec::with_capacity(flat.leaves.len());
    for (i, &(value, group, _)) in flat.leaves.iter().enumerate() {
        values.push(value);
        if flat.named {
            let group = &flat.groups[group];
            let label = if group.prefix.is_empty() || group.bare == 1 {
                group.prefix.clone()
            } else {
                format!("{}{}", group.prefix, i - group.start + 1)
            };
            labels.push(if group.missing && group.bare == 1 {
                CharId::NA
            } else {
                ch(ctx.session_mut(), &label)?
            });
        }
    }
    let s = ctx.session_mut();
    let factor = flat.all_factors && !values.is_empty() && !flat.levels.is_empty();
    let out = if factor {
        let mut level_ids = Vec::new();
        let mut codes = HashMap::new();
        for levels in flat.levels {
            for &level in s.str_slice(levels)? {
                codes.entry(level).or_insert_with(|| {
                    level_ids.push(level);
                    level_ids.len() as i32
                });
            }
        }
        let mut data = Vec::with_capacity(values.len());
        for (value, _, levels) in flat.leaves {
            let code = s.int(value)?.get(0);
            let code = if let Some(levels) = levels
                && code > 0
            {
                s.str_slice(levels)?
                    .get(code as usize - 1)
                    .and_then(|id| codes.get(id))
                    .copied()
                    .unwrap_or(NA_INTEGER)
            } else {
                NA_INTEGER
            };
            data.push(code);
        }
        let out = s.alloc_int_from(&data)?;
        let levels = s.alloc_str_from(&level_ids)?;
        set(s, out, "levels", levels)?;
        out
    } else {
        concatenate(s, &values, flat.kind)?
    };
    if labels.iter().any(|id| *id != CharId::BLANK) {
        set_names(s, out, &labels)?;
    }
    if factor {
        let class = s.scalar_str("factor")?;
        set(s, out, "class", class)?;
    }
    Ok(out)
}

fn container(ty: SexpType) -> bool {
    matches!(ty, SexpType::List | SexpType::Vec)
}

impl Flat {
    fn walk(
        &mut self,
        ctx: &mut Ctx,
        call: &Call,
        x: SexpId,
        group: usize,
        depth: usize,
    ) -> EvalResult<()> {
        let stack_cap = (ctx.stack_budget() / 4096).clamp(128, 2048);
        if depth >= ctx.opts().expressions.min(stack_cap) {
            return Err(error(ctx, call, "evaluation nested too deeply"));
        }
        let ty = ctx.session().type_of(x)?;
        let list = container(ty);
        let kind = if self.recursive && ty == SexpType::Expr {
            SexpType::Nil
        } else {
            ty
        };
        if ty == SexpType::Nil {
            return Ok(());
        }
        let vector = crate::ops::common::atomic(ty) || ty == SexpType::Expr;
        if !vector && (!list || !self.recursive && depth > 1) {
            self.all_factors = false;
            return self.push(ctx, call, x, group, SexpType::Vec, None);
        }
        let levels = if vector && has_class(ctx.session(), x, "factor") {
            let levels = attr(ctx.session(), x, "levels");
            self.levels.push(levels);
            Some(levels)
        } else {
            if vector {
                self.all_factors = false;
            }
            None
        };
        if !list && rank(kind) > rank(self.kind) {
            self.kind = kind;
        }

        let mut labels = names(ctx.session(), x).to_vec();
        if self.named && ty == SexpType::List {
            let s = ctx.session();
            let tags: Vec<_> = s
                .pairlist_iter(x)
                .map(|(tag, _)| {
                    tag.and_then(|sym| s.sym_name(sym).ok())
                        .unwrap_or("")
                        .to_owned()
                })
                .collect();
            labels = tags
                .iter()
                .map(|name| ch(ctx.session_mut(), name))
                .collect::<Result<_, _>>()?;
        }
        let n = ctx.session().xlength(x)?;
        for i in 0..n {
            let child = element(ctx.session_mut(), x, i)?;
            ctx.protect(child);
            let name = labels.get(i).copied().filter(|id| *id != CharId::BLANK);
            let child_group = if self.named
                && let Some(name) = name
            {
                let prefix = &self.groups[group].prefix;
                let missing = name == CharId::NA && prefix.is_empty();
                let name = if name == CharId::NA {
                    "NA".to_owned()
                } else {
                    text(ctx.session(), name)
                };
                let prefix = if prefix.is_empty() {
                    name
                } else {
                    format!("{prefix}.{name}")
                };
                self.groups.push(Group {
                    prefix,
                    start: self.leaves.len(),
                    bare: 0,
                    missing,
                });
                self.groups.len() - 1
            } else {
                group
            };
            if list && (self.recursive || depth == 0) || ty == SexpType::Expr && self.recursive {
                self.walk(ctx, call, child, child_group, depth + 1)?;
            } else {
                if list {
                    self.all_factors = false;
                }
                let kind = if list {
                    if self.kind == SexpType::Expr {
                        SexpType::Expr
                    } else {
                        SexpType::Vec
                    }
                } else {
                    kind
                };
                self.push(ctx, call, child, child_group, kind, levels)?;
            }
        }
        Ok(())
    }

    fn push(
        &mut self,
        ctx: &mut Ctx,
        call: &Call,
        value: SexpId,
        group: usize,
        kind: SexpType,
        levels: Option<SexpId>,
    ) -> EvalResult<()> {
        vector_limit(
            ctx,
            call,
            self.leaves.len() + 1,
            size_of::<(SexpId, usize, Option<SexpId>)>(),
        )?;
        if rank(kind) > rank(self.kind) || !self.recursive && kind == SexpType::Expr {
            self.kind = kind;
        }
        self.leaves.push((value, group, levels));
        self.groups[group].bare += 1;
        Ok(())
    }
}
