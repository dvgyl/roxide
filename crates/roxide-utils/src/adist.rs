mod pattern;

use roxide_eval::{Args, Call, Ctx, EvalResult, Module};
use roxide_gc::SexpId;
use roxide_value::{CharId, Encoding, NA_INTEGER, NA_REAL, SEXP_NIL, SexpType, ValueApi};

use super::tools_rd::{chars, internal, set_attr};
use pattern::Pattern;

pub(super) const MODULE: Module = Module {
    prims: &[internal("rox_utils_adist", 10, adist)],
    prelude: r#"
adist <- function(x, y = NULL, costs = NULL, counts = FALSE, fixed = TRUE,
                  partial = !fixed, ignore.case = FALSE, useBytes = FALSE) {
    if (is.null(y)) y <- x
    .Internal(rox_utils_adist(as.character(x), as.character(y), names(x), names(y),
                              costs, counts, fixed, partial, ignore.case, useBytes))
}
"#,
};

type Costs = [f64; 3];
const OPERATIONS: [&str; 3] = ["ins", "del", "sub"];
const COST_NAMES: [&str; 3] = ["insertions", "deletions", "substitutions"];

fn adist(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let trace = flag(ctx, call, args.get(5), false)?;
    let fixed = flag(ctx, call, args.get(6), false)?;
    let partial = flag(ctx, call, args.get(7), false)?;
    let fold = flag(ctx, call, args.get(8), false)?;
    let bytes = flag(ctx, call, args.get(9), true)?;
    if !fixed && !partial {
        ctx.warn_now(
            call.err_call(),
            "argument 'partial = FALSE' will be ignored",
        )?;
        return Err(ctx.stop(
            call.err_call(),
            "VECTOR_ELT() can only be applied to a 'list', not a 'character'",
        ));
    }
    let limit = ctx.host().limits().max_vector_bytes;
    let costs = edit_costs(ctx, call, args.get(4), partial)?;
    let x_ids = ctx.session().str_slice(args.get(0))?.to_vec();
    let y_ids = ctx.session().str_slice(args.get(1))?.to_vec();
    let mut units_left = limit / size_of::<u32>();
    for &id in x_ids.iter().chain(&y_ids) {
        if id != CharId::NA {
            let bytes = ctx.session().char_bytes(id)?.len();
            units_left = units_left.checked_sub(bytes).ok_or_else(|| {
                ctx.stop(
                    call.err_call(),
                    "edit distance inputs exceed the size limit",
                )
            })?;
        }
    }
    let use_bytes = bytes
        || x_ids.iter().chain(&y_ids).any(|&id| {
            ctx.session()
                .char_encoding(id)
                .is_ok_and(|encoding| encoding == Encoding::Bytes)
        });
    let x = strings(ctx, &x_ids, use_bytes, fold && !partial)?;
    let y = strings(ctx, &y_ids, use_bytes, fold && !partial)?;
    let size = x
        .len()
        .checked_mul(y.len())
        .filter(|&size| {
            size <= limit / if trace { 32 } else { 8 }
                && x.len() <= i32::MAX as usize
                && y.len() <= i32::MAX as usize
        })
        .ok_or_else(|| {
            ctx.stop(
                call.err_call(),
                "edit distance matrix exceeds the size limit",
            )
        })?;

    let patterns = if partial {
        x.iter()
            .map(|item| {
                item.as_ref()
                    .map(|units| Pattern::new(units, fixed, fold, use_bytes))
                    .transpose()
                    .map_err(|error| ctx.stop(call.err_call(), error))
            })
            .collect::<EvalResult<Vec<_>>>()?
    } else {
        Vec::new()
    };

    let mut distances = Vec::with_capacity(size);
    let mut counts = trace.then(|| vec![NA_INTEGER; size * 3]);
    let mut offsets = (trace && partial).then(|| vec![-1; size * 2]);
    let mut trafos = if trace && !partial {
        Vec::with_capacity(size)
    } else {
        Vec::new()
    };
    let mut budget = limit.min(32 * 1024 * 1024);
    for rhs in &y {
        for (row, lhs) in x.iter().enumerate() {
            ctx.check_user_interrupt()?;
            let index = distances.len();
            let (distance, edits, alignment) = match (lhs, rhs) {
                (Some(lhs), Some(rhs)) => {
                    let width = patterns
                        .get(row)
                        .and_then(Option::as_ref)
                        .map_or(lhs.len() + 1, Pattern::len);
                    let work = width.saturating_mul(rhs.len() + 1);
                    if work > budget {
                        return Err(
                            ctx.stop(call.err_call(), "edit distance exceeds the work limit")
                        );
                    }
                    budget -= work;
                    if partial {
                        let found = patterns[row]
                            .as_ref()
                            .and_then(|pattern| pattern.find(rhs, costs));
                        if let Some(found) = found {
                            if let Some(offsets) = &mut offsets {
                                offsets[index] = found.start as i32 + 1;
                                offsets[index + size] = found.end as i32;
                            }
                            (found.cost, found.edits, String::new())
                        } else {
                            (f64::INFINITY, [NA_INTEGER; 3], String::new())
                        }
                    } else {
                        full_distance(lhs, rhs, costs, trace)
                    }
                }
                _ => (NA_REAL, [NA_INTEGER; 3], String::new()),
            };
            distances.push(distance);
            if let Some(counts) = &mut counts {
                for (operation, count) in edits.into_iter().enumerate() {
                    counts[index + operation * size] = count;
                }
            }
            if trace && !partial {
                trafos.push(alignment);
            }
        }
    }

    let dims = [x.len() as i32, y.len() as i32];
    let labels = [args.get(2), args.get(3)];
    let answer = ctx.session_mut().alloc_real_from(&distances)?;
    ctx.protect(answer);
    shape(ctx, answer, &dims, &labels)?;
    if let Some(counts) = counts {
        let keys = chars(ctx, OPERATIONS)?;
        ctx.protect(keys);
        let counts = ctx.session_mut().alloc_int_from(&counts)?;
        shape(
            ctx,
            counts,
            &[dims[0], dims[1], 3],
            &[labels[0], labels[1], keys],
        )?;
        set_attr(ctx, answer, "counts", counts)?;
        if let Some(offsets) = offsets {
            let keys = chars(ctx, ["first", "last"])?;
            ctx.protect(keys);
            let offsets = ctx.session_mut().alloc_int_from(&offsets)?;
            shape(
                ctx,
                offsets,
                &[dims[0], dims[1], 2],
                &[labels[0], labels[1], keys],
            )?;
            set_attr(ctx, answer, "offsets", offsets)?;
        } else {
            let trafos = chars(ctx, trafos.iter().map(String::as_str))?;
            shape(ctx, trafos, &dims, &labels)?;
            set_attr(ctx, answer, "trafos", trafos)?;
        }
    }
    Ok(answer)
}

fn flag(ctx: &mut Ctx, call: &Call, value: SexpId, allow_na: bool) -> EvalResult<bool> {
    let (value, _) = ctx.session_mut().coerce(value, SexpType::Lgl)?;
    let flag = ctx
        .session()
        .lgl(value)?
        .iter()
        .next()
        .and_then(|item| item.get());
    match flag {
        Some(flag) => Ok(flag),
        None if allow_na => Ok(false),
        None => Err(ctx.stop(call.err_call(), "NA in coercion to boolean")),
    }
}

fn edit_costs(ctx: &mut Ctx, call: &Call, value: SexpId, partial: bool) -> EvalResult<Costs> {
    let mut costs = [1.0; 3];
    if value == SEXP_NIL {
        return Ok(costs);
    }
    let names = ctx.session_mut().intern("names")?;
    let names = ctx.session().attr(value, names);
    if names == SEXP_NIL {
        let kind = ctx.session().type_of(value)?;
        let count = ctx.session().xlength(value)?;
        let valid = count > 0
            && (0..count).all(|index| {
                cost_component(ctx, value, kind, index)
                    .is_ok_and(|cost| cost.is_some_and(|cost| cost >= 0.0))
            });
        return if valid {
            Ok(costs)
        } else {
            Err(ctx.stop(call.err_call(), "cost components must be non-negative"))
        };
    }
    let ids = ctx.session().str_slice(names)?.to_vec();
    let kind = ctx.session().type_of(value)?;
    let mut priority = [0; 3];
    let mut unknown = false;
    for (index, id) in ids.into_iter().enumerate() {
        let label = ctx.session().char_bytes(id)?;
        let Some(key) = COST_NAMES.iter().position(|name| {
            id != CharId::NA && !label.is_empty() && name.as_bytes().starts_with(label)
        }) else {
            unknown = true;
            continue;
        };
        let rank = if label == COST_NAMES[key].as_bytes() {
            2
        } else {
            1
        };
        if priority[key] >= rank {
            unknown = true;
            continue;
        }
        priority[key] = rank;
        costs[key] = cost_component(ctx, value, kind, index)?.unwrap_or(1.0);
    }
    if unknown {
        ctx.warn_now(call.err_call(), "unknown cost components ignored")?;
    }
    if priority == [0; 3] || costs.iter().any(|&cost| cost.is_nan() || cost < 0.0) {
        return Err(ctx.stop(call.err_call(), "cost components must be non-negative"));
    }
    if partial {
        for cost in &mut costs {
            if *cost > i32::MAX as f64 {
                ctx.warn_now(
                    call.err_call(),
                    "NAs introduced by coercion to integer range",
                )?;
                *cost = f64::INFINITY;
            } else {
                *cost = cost.trunc();
            }
        }
    }
    Ok(costs)
}

fn cost_component(
    ctx: &Ctx,
    value: SexpId,
    kind: SexpType,
    index: usize,
) -> EvalResult<Option<f64>> {
    let session = ctx.session();
    let (value, kind, index) = if kind == SexpType::List {
        let item = session.list_get(value, index)?;
        if session.xlength(item)? != 1 {
            return Ok(None);
        }
        (item, session.type_of(item)?, 0)
    } else {
        (value, kind, index)
    };
    Ok(Some(match kind {
        SexpType::Int => f64::from(session.int(value)?.get(index)),
        SexpType::Real => session.real(value)?.get(index),
        _ => f64::NAN,
    }))
}

fn strings(
    ctx: &Ctx,
    ids: &[CharId],
    bytes: bool,
    fold: bool,
) -> EvalResult<Vec<Option<Vec<u32>>>> {
    ids.iter()
        .map(|&id| {
            if id == CharId::NA {
                return Ok(None);
            }
            let data = ctx.session().char_bytes(id)?;
            let codepoints = if bytes {
                data.iter()
                    .map(|&byte| {
                        u32::from(if fold {
                            byte.to_ascii_lowercase()
                        } else {
                            byte
                        })
                    })
                    .collect()
            } else if ctx.session().char_encoding(id)? == Encoding::Latin1 {
                data.iter()
                    .map(|&byte| fold_char(char::from(byte), fold))
                    .collect()
            } else {
                String::from_utf8_lossy(data)
                    .chars()
                    .map(|ch| fold_char(ch, fold))
                    .collect()
            };
            Ok(Some(codepoints))
        })
        .collect()
}

fn fold_char(ch: char, fold: bool) -> u32 {
    (if fold {
        ch.to_lowercase().next().unwrap_or(ch)
    } else {
        ch
    }) as u32
}

fn shape(ctx: &mut Ctx, value: SexpId, dims: &[i32], labels: &[SexpId]) -> EvalResult<()> {
    ctx.protect(value);
    let dim = ctx.session_mut().alloc_int_from(dims)?;
    ctx.protect(dim);
    set_attr(ctx, value, "dim", dim)?;
    if labels.iter().any(|&label| label != SEXP_NIL) {
        let names = ctx.session_mut().alloc_list(labels.len())?;
        ctx.protect(names);
        for (index, &label) in labels.iter().enumerate() {
            ctx.session_mut().list_set(names, index, label)?;
        }
        set_attr(ctx, value, "dimnames", names)?;
    }
    Ok(())
}

fn full_distance(
    x: &[u32],
    y: &[u32],
    [insert, delete, sub]: Costs,
    trace: bool,
) -> (f64, [i32; 3], String) {
    let stride = y.len() + 1;
    let mut previous = vec![0.0; stride];
    let mut current = vec![0.0; stride];
    let mut steps = if trace {
        vec![b'I'; (x.len() + 1) * stride]
    } else {
        Vec::new()
    };
    for j in 1..stride {
        previous[j] = previous[j - 1] + insert;
    }
    for (i, &left) in x.iter().enumerate() {
        current[0] = previous[0] + delete;
        if trace {
            steps[(i + 1) * stride] = b'D';
        }
        for (j, &right) in y.iter().enumerate() {
            let same = left == right;
            let diagonal = previous[j] + if same { 0.0 } else { sub };
            let insertion = current[j] + insert;
            let deletion = previous[j + 1] + delete;
            let (cost, step) = if same && diagonal <= insertion && diagonal <= deletion {
                (diagonal, b'M')
            } else if insertion <= deletion && insertion <= diagonal {
                (insertion, b'I')
            } else if deletion <= diagonal {
                (deletion, b'D')
            } else {
                (diagonal, b'S')
            };
            current[j + 1] = cost;
            if trace {
                steps[(i + 1) * stride + j + 1] = step;
            }
        }
        std::mem::swap(&mut current, &mut previous);
    }
    let mut edits = [0; 3];
    let mut path = String::new();
    if trace {
        let (mut i, mut j) = (x.len(), y.len());
        while i != 0 || j != 0 {
            let step = steps[i * stride + j];
            match step {
                b'I' => {
                    edits[0] += 1;
                    j -= 1;
                }
                b'D' => {
                    edits[1] += 1;
                    i -= 1;
                }
                _ => {
                    edits[2] += i32::from(step == b'S');
                    i -= 1;
                    j -= 1;
                }
            }
            path.push(char::from(step));
        }
        path = path.chars().rev().collect();
    }
    (previous[y.len()], edits, path)
}
