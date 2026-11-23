use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{NA_REAL, SEXP_NIL, SexpType, ValueApi};

use super::strings;
use crate::distn::support::{attr, limit, numbers, set_attr, set_dim, set_dimnames};

fn character(ctx: &Ctx, x: SexpId) -> EvalResult<Vec<String>> {
    if x == SEXP_NIL {
        return Ok(Vec::new());
    }
    ctx.session()
        .str_slice(x)?
        .iter()
        .map(|&id| Ok(String::from_utf8_lossy(ctx.session().char_bytes(id)?).into_owned()))
        .collect()
}

fn columns(ctx: &mut Ctx, x: SexpId) -> EvalResult<Option<usize>> {
    let dims = attr(ctx, x, "dim")?;
    if dims == SEXP_NIL {
        return Ok(None);
    }
    Ok(ctx.session().int(dims)?.iter().nth(1).map(|n| n as usize))
}

fn column_names(ctx: &mut Ctx, x: SexpId) -> EvalResult<Vec<String>> {
    let dim_names = attr(ctx, x, "dimnames")?;
    if dim_names == SEXP_NIL {
        return Ok(Vec::new());
    }
    character(ctx, ctx.session().list_get(dim_names, 1)?)
}

struct Numeric {
    values: Vec<f64>,
    labels: Vec<String>,
    width: usize,
}

struct Categorical {
    codes: Vec<i32>,
    levels: Vec<String>,
    coding: Numeric,
}

enum Variable {
    Numeric(Numeric),
    Categorical(Categorical),
    Unsupported(&'static str),
}

impl Numeric {
    fn label(&self, index: usize, variable: &str, categorical: bool) -> String {
        if !categorical && self.width == 1 {
            variable.to_owned()
        } else if let Some(label) = self.labels.get(index) {
            format!("{variable}{label}")
        } else {
            format!("{variable}{}", index + 1)
        }
    }
}

impl Variable {
    fn width(&self, code: i32) -> usize {
        match self {
            Self::Numeric(n) => n.width,
            Self::Categorical(f) if code == 1 => f.coding.width,
            Self::Categorical(f) => f.levels.len(),
            Self::Unsupported(_) => 0,
        }
    }

    fn label(&self, code: i32, col: usize, variable: &str) -> String {
        match self {
            Self::Numeric(n) => n.label(col, variable, false),
            Self::Categorical(f) if code == 1 => f.coding.label(col, variable, true),
            Self::Categorical(f) => format!("{variable}{}", f.levels[col]),
            Self::Unsupported(_) => variable.to_owned(),
        }
    }

    fn get(&self, code: i32, col: usize, row: usize, n: usize) -> f64 {
        match self {
            Self::Numeric(x) => x.values[col * n + row],
            Self::Categorical(f) => {
                let Ok(level) = usize::try_from(f.codes[row]).map(|level| level.saturating_sub(1))
                else {
                    return NA_REAL;
                };
                if f.codes[row] == 0 || level >= f.levels.len() {
                    return NA_REAL;
                }
                if code == 1 {
                    f.coding.values[col * f.levels.len() + level]
                } else {
                    f64::from(u8::from(level == col))
                }
            }
            Self::Unsupported(_) => NA_REAL,
        }
    }
}

fn variable(
    ctx: &mut Ctx,
    site: &Call,
    x: SexpId,
    coding: SexpId,
    n: usize,
    response: bool,
) -> EvalResult<Variable> {
    let levels = attr(ctx, x, "levels")?;
    if levels != SEXP_NIL {
        let levels = character(ctx, levels)?;
        let codes = ctx.session().int(x)?.iter().collect::<Vec<_>>();
        if codes.len() != n {
            return Err(ctx.stop(site.err_call(), "invalid factor length"));
        }
        if !response && levels.len() < 2 {
            return Err(ctx.stop(
                site.err_call(),
                "contrasts can be applied only to factors with 2 or more levels",
            ));
        }
        let width = columns(ctx, coding)?.unwrap_or(0);
        let values = if width == 0 {
            Vec::new()
        } else {
            numbers(ctx, site, coding)?
        };
        if width.saturating_mul(levels.len()) != values.len() {
            return Err(ctx.stop(site.err_call(), "wrong number of contrast matrix rows"));
        }
        return Ok(Variable::Categorical(Categorical {
            codes,
            levels,
            coding: Numeric {
                values,
                width,
                labels: column_names(ctx, coding)?,
            },
        }));
    }
    let kind = ctx.session().type_of(x)?;
    if !matches!(kind, SexpType::Int | SexpType::Real | SexpType::Lgl) {
        return Ok(Variable::Unsupported(kind.type_name(false)));
    }
    let width = columns(ctx, x)?.unwrap_or(1);
    let values = numbers(ctx, site, x)?;
    if width.saturating_mul(n) != values.len() {
        return Err(ctx.stop(site.err_call(), "variable lengths differ"));
    }
    Ok(Variable::Numeric(Numeric {
        values,
        width,
        labels: column_names(ctx, x)?,
    }))
}

pub(super) fn model_matrix(ctx: &mut Ctx, site: &Call, args: &mut Args) -> EvalResult {
    let terms = args.get(0);
    let frame = ctx.session().list_slice(args.get(1))?.to_vec();
    let coding = ctx.session().list_slice(args.get(2))?.to_vec();
    let contrast_spec = args.get(3);
    let row_names = args.get(4);
    let n = ctx.session().xlength(row_names)?;
    let nvars = frame.len();
    let factors = attr(ctx, terms, "factors")?;
    let response = attr(ctx, terms, "response")?;
    let response = ctx.session().int(response)?.iter().next() == Some(1);
    let intercept = attr(ctx, terms, "intercept")?;
    let intercept = ctx.session().int(intercept)?.iter().next() == Some(1);
    let mut codes = ctx.session().int(factors)?.iter().collect::<Vec<_>>();
    let name_list = attr(ctx, factors, "dimnames")?;
    let names = if name_list == SEXP_NIL {
        Vec::new()
    } else {
        character(ctx, ctx.session().list_get(name_list, 0)?)?
    };
    if !codes.is_empty() && (nvars == 0 || names.len() != nvars || codes.len() % nvars != 0)
        || coding.len() != nvars
    {
        return Err(ctx.stop(site.err_call(), "invalid model frame"));
    }
    let variables = frame
        .into_iter()
        .zip(coding)
        .enumerate()
        .map(|(i, (x, coding))| variable(ctx, site, x, coding, n, response && i == 0))
        .collect::<EvalResult<Vec<_>>>()?;

    if !intercept
        && nvars != 0
        && let Some((_, code)) = codes.iter_mut().enumerate().find(|(i, code)| {
            **code > 0
                && !(response && i % nvars == 0)
                && matches!(variables[i % nvars], Variable::Categorical(_))
        })
    {
        *code = 2;
    }

    let mut vals = Vec::new();
    let mut labels = Vec::new();
    let mut assign = Vec::new();
    if intercept {
        limit::<f64>(ctx, site, n)?;
        vals.resize(n, 1.0);
        labels.push("(Intercept)".to_owned());
        assign.push(0);
    }
    for (term_index, codes) in codes.chunks(nvars.max(1)).enumerate() {
        let selected = codes
            .iter()
            .enumerate()
            .filter(|&(_, &code)| code > 0)
            .collect::<Vec<_>>();
        if response && matches!(selected.as_slice(), [(0, _)]) {
            ctx.warn_now(
                site.err_call(),
                "the response appeared on the right-hand side and was dropped",
            )?;
            ctx.warn_now(
                site.err_call(),
                format!(
                    "problem with term {} in model.matrix: no columns are assigned",
                    term_index + 1
                ),
            )?;
            continue;
        }
        let mut width = 1usize;
        for &(i, &code) in &selected {
            if let Variable::Unsupported(kind) = variables[i] {
                return Err(ctx.stop(
                    site.err_call(),
                    format!("variable {} has no levels", i + 1) + &format!(" (type {kind})"),
                ));
            }
            width = width.saturating_mul(variables[i].width(code));
        }
        let total = labels.len().saturating_add(width);
        limit::<f64>(ctx, site, total.saturating_mul(n))?;
        limit::<String>(ctx, site, total)?;
        for mut index in 0..width {
            let mut label = String::new();
            let start = vals.len();
            vals.resize(start + n, 1.0);
            for &(i, &code) in &selected {
                let x = &variables[i];
                let cols = x.width(code);
                let col = index % cols;
                index /= cols;
                if !label.is_empty() {
                    label.push(':');
                }
                label.push_str(&x.label(code, col, &names[i]));
                for (row, value) in vals[start..].iter_mut().enumerate() {
                    *value *= x.get(code, col, row, n);
                }
            }
            labels.push(label);
            assign.push((term_index + 1) as i32);
        }
    }

    ctx.scoped(|ctx| {
        let result = ctx.session_mut().alloc_real_from(&vals)?;
        set_dim(ctx, site, result, n, labels.len())?;
        let labels = if labels.is_empty() {
            SEXP_NIL
        } else {
            strings(ctx.session_mut(), labels.iter().map(String::as_str))?
        };
        set_dimnames(ctx, result, row_names, labels)?;
        let assign = ctx.session_mut().alloc_int_from(&assign)?;
        set_attr(ctx, result, "assign", assign)?;
        set_attr(ctx, result, "contrasts", contrast_spec)?;
        Ok(result)
    })
}
