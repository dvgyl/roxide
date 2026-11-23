use std::collections::HashSet;

use roxide_base::scan::r_strings;
use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{CharId, SEXP_NIL, SexpType, ValueApi};

use super::set;

const NODE: usize = 56;
const VECTOR: usize = 48;

pub(super) fn size(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let limits = ctx.host().limits();
    let depth = limits.max_expressions.min(ctx.opts().expressions).min(128);
    let mut count = Counter {
        ctx,
        call,
        path: HashSet::new(),
        depth,
    };
    let bytes = count.value(args.get(0))?;
    let s = ctx.session_mut();
    let result = s.scalar_real(bytes as f64)?;
    let class = s.scalar_str("object_size")?;
    set(s, result, "class", class)?;
    Ok(result)
}

struct Counter<'a> {
    ctx: &'a mut Ctx,
    call: &'a Call,
    path: HashSet<SexpId>,
    depth: usize,
}

impl Counter<'_> {
    fn value(&mut self, value: SexpId) -> EvalResult<usize> {
        if value == SEXP_NIL || self.path.contains(&value) {
            return Ok(0);
        }
        if self.path.len() >= self.depth {
            return Err(self
                .ctx
                .stop(self.call.err_call(), "object size recursion limit exceeded"));
        }
        self.path.insert(value);
        let result = self.count(value);
        self.path.remove(&value);
        result
    }

    fn count(&mut self, x: SexpId) -> EvalResult<usize> {
        let s = self.ctx.session();
        let ty = s.type_of(x)?;
        let mut total = match ty {
            SexpType::Raw => allocation(s.xlength(x)?),
            SexpType::Lgl | SexpType::Int => allocation(s.xlength(x)?.saturating_mul(4)),
            SexpType::Real | SexpType::Str | SexpType::Vec | SexpType::Expr => {
                allocation(s.xlength(x)?.saturating_mul(8))
            }
            SexpType::Cplx => allocation(s.xlength(x)?.saturating_mul(16)),
            _ => NODE,
        };
        let attrs = s.attrs(x)?;
        for (_, value) in attrs {
            total = total
                .saturating_add(2 * NODE)
                .saturating_add(self.value(value)?);
        }
        let s = self.ctx.session();
        match ty {
            SexpType::Str => {
                let mut seen = HashSet::new();
                for &id in s.str_slice(x)? {
                    if id != CharId::NA && seen.insert(id) {
                        total = total
                            .saturating_add(allocation(s.char_bytes(id)?.len().saturating_add(1)));
                    }
                }
            }
            SexpType::Vec | SexpType::Expr => {
                for i in 0..s.xlength(x)? {
                    let child = self.ctx.session().list_get(x, i)?;
                    total = total.saturating_add(self.value(child)?);
                }
            }
            SexpType::List | SexpType::Lang | SexpType::Dot => {
                let mut cell = x;
                let mut seen = HashSet::new();
                while cell != SEXP_NIL && seen.insert(cell) {
                    let (car, cdr, tag) = self.ctx.session().cons_parts(cell)?;
                    total = total
                        .saturating_add(self.value(car)?)
                        .saturating_add(self.value(tag)?);
                    if cell != x {
                        total = total.saturating_add(NODE);
                    }
                    cell = cdr;
                }
            }
            SexpType::Clo => {
                let (formals, body, _) = s.closure_parts(x)?;
                total = total
                    .saturating_add(self.value(formals)?)
                    .saturating_add(self.value(body)?);
            }
            _ => {}
        }
        Ok(total)
    }
}

fn allocation(bytes: usize) -> usize {
    let payload = [0usize, 8, 16, 32, 48, 64, 128]
        .into_iter()
        .find(|&size| size >= bytes)
        .unwrap_or_else(|| bytes.saturating_add(7) & !7);
    VECTOR.saturating_add(payload)
}

pub(super) fn format(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = ctx.session_mut().coerce(args.get(0), SexpType::Real)?.0;
    let x = ctx.session().real(x)?.get(0);
    let units = r_strings(ctx, args.get(1))?;
    let units = units.first().and_then(Option::as_deref).unwrap_or("");
    let standard = r_strings(ctx, args.get(2))?;
    let standard = standard.first().and_then(Option::as_deref).unwrap_or("");
    let digits = ctx.session_mut().coerce(args.get(3), SexpType::Real)?.0;
    let digits = ctx.session().real(digits)?.get(0);
    let digits = digits.clamp(-22.0, 22.0) as i32;
    let legacy = ["b", "Kb", "Mb", "Gb", "Tb", "Pb"];
    let si = [
        "B", "kB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB", "RB", "QB",
    ];
    let iec = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB", "ZiB", "YiB"];
    let (base, mut labels): (f64, &[&str]) = match standard {
        "legacy" => (1024.0, &legacy),
        "SI" => (1000.0, &si),
        "IEC" => (1024.0, &iec),
        _ => return Err(ctx.stop(call.err_call(), "invalid 'standard'")),
    };
    let index = if units == "auto" {
        (1..labels.len())
            .take_while(|&i| x >= base.powi(i as i32))
            .last()
            .unwrap_or(0)
    } else if matches!(units, "b" | "B") {
        0
    } else if let Some(i) = labels.iter().position(|&label| label == units) {
        i
    } else if let Some(i) = iec.iter().position(|&label| label == units) {
        labels = &iec;
        i
    } else if let Some(i) = si.iter().position(|&label| label == units) {
        if standard == "legacy" && units != "kB" && i < legacy.len() {
            i
        } else {
            return Err(ctx.stop(call.err_call(), "For SI units, specify 'standard = \"SI\"'"));
        }
    } else if units == "KB" && standard == "legacy" {
        1
    } else {
        return Err(ctx.stop(call.err_call(), "invalid 'units'"));
    };
    let number = x / base.powi(index as i32);
    let multiple = 10.0_f64.powi(digits);
    let number = if index == 0 {
        number
    } else {
        (number * multiple).round_ties_even() / multiple
    };
    let unit = if index == 0 { "bytes" } else { labels[index] };
    Ok(ctx.session_mut().scalar_str(&format!("{number} {unit}"))?)
}
