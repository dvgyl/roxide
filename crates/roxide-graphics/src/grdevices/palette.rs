use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_value::{CharId, SEXP_NIL, SexpType, ValueApi};

use crate::engine::{DEFAULT_PALETTE, GraphicsExt, Rgba};

use super::colors::{alpha_byte, catalog, hcl_rgb, hex, literal};
use super::{flag, limit, number, optional_numbers, recycled, set_attr, strings};

pub(super) fn series(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let start = number(ctx, args.get(0))?;
    let end = number(ctx, args.get(1))?;
    let count = number(ctx, args.get(2))?.ceil();
    if !count.is_finite() || count < 0.0 {
        return Err(ctx.stop(
            call.err_call(),
            "'length.out' must be a non-negative number",
        ));
    }
    let count = count as usize;
    limit::<f64>(ctx, call, count)?;
    if count <= 1 {
        return Ok(ctx.session_mut().alloc_real_from(&[start][..count])?);
    }
    // A fused multiply-add reproduces the sequence rounding on the arm64 oracle.
    let step = (end - start) / (count - 1) as f64;
    let mut output: Vec<_> = (0..count - 1)
        .map(|i| (i as f64).mul_add(step, start))
        .collect();
    output.push(end);
    Ok(ctx.session_mut().alloc_real_from(&output)?)
}

fn discrete_catalog() -> impl Iterator<Item = (&'static str, &'static str)> {
    include_str!("palettes.tsv")
        .lines()
        .filter_map(|line| line.split_once('\t'))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HclKind {
    Qualitative,
    Sequential,
    Diverging,
    DivergingX,
}

#[derive(Clone, Copy)]
struct HclEntry {
    kind: HclKind,
    name: &'static str,
    params: &'static str,
}

fn hcl_entries() -> impl Iterator<Item = HclEntry> {
    include_str!("hcl_palettes.tsv").lines().filter_map(|line| {
        let (kind, rest) = line.split_once('\t')?;
        let (name, params) = rest.split_once('\t')?;
        let kind = match kind {
            "Q" => HclKind::Qualitative,
            "S" => HclKind::Sequential,
            "D" => HclKind::Diverging,
            "X" => HclKind::DivergingX,
            _ => return None,
        };
        Some(HclEntry { kind, name, params })
    })
}

fn hcl_catalog() -> impl Iterator<Item = (&'static str, &'static str)> {
    hcl_entries().map(|entry| (entry.name, entry.params))
}

fn normalize(name: &str) -> String {
    name.chars()
        .filter(|c| !matches!(c, ' ' | '-' | '_'))
        .flat_map(char::to_lowercase)
        .collect()
}

fn select<'a>(
    ctx: &mut Ctx,
    call: &Call,
    requested: &str,
    entries: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> EvalResult<(&'a str, &'a str)> {
    let key = normalize(requested);
    let mut matches = Vec::new();
    for (name, payload) in entries {
        let candidate = normalize(name);
        if candidate == key {
            return Ok((name, payload));
        }
        if candidate.starts_with(&key) && !key.is_empty() {
            matches.push((name, payload));
        }
    }
    match matches.as_slice() {
        [entry] => Ok(*entry),
        [] => Err(ctx.stop(
            call.err_call(),
            "'palette' does not match any given palette",
        )),
        _ => Err(ctx.stop(call.err_call(), "'palette' is ambiguous")),
    }
}

pub(super) fn palette(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let value = args.get(0);
    let len = ctx.session().xlength(value)?;
    if ctx.session().type_of(value)? != SexpType::Str {
        return Err(ctx.stop(call.err_call(), "invalid argument type"));
    }
    let previous = ctx.ext::<GraphicsExt>().palette.clone();
    let replacement = if len == 0 {
        None
    } else if len == 1 {
        let requested = text(ctx, args, 0)?;
        if requested.eq_ignore_ascii_case("default") || requested.eq_ignore_ascii_case("r4") {
            Some(DEFAULT_PALETTE.iter().map(|&s| s.to_owned()).collect())
        } else if requested.eq_ignore_ascii_case("r3") {
            Some(
                [
                    "black", "red", "green3", "blue", "cyan", "magenta", "yellow", "gray",
                ]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            )
        } else {
            let key = normalize(&requested);
            let mut matching =
                discrete_catalog().filter(|(name, _)| normalize(name).starts_with(&key));
            let matched = discrete_catalog()
                .find(|(name, _)| normalize(name) == key)
                .or_else(|| matching.next().filter(|_| matching.next().is_none()));
            let Some((_, data)) = matched else {
                return Err(ctx.stop(call.err_call(), "unknown palette (need >= 2 colors)"));
            };
            let (colors, _) = data.split_once('\t').unwrap_or((data, ""));
            Some(
                colors
                    .split_ascii_whitespace()
                    .map(canonical)
                    .collect::<Result<_, _>>()
                    .map_err(|message| ctx.stop(call.err_call(), message))?,
            )
        }
    } else {
        if len > 1024 {
            return Err(ctx.stop(call.err_call(), "maximum number of colors is 1024"));
        }
        let replacement = ctx
            .session()
            .str_slice(value)?
            .iter()
            .map(|&id| {
                let text = if id == CharId::NA {
                    "NA".into()
                } else {
                    String::from_utf8_lossy(ctx.session().char_bytes(id)?)
                };
                Ok(canonical(&text))
            })
            .collect::<EvalResult<Vec<_>>>()?;
        Some(
            replacement
                .into_iter()
                .collect::<Result<_, _>>()
                .map_err(|message| ctx.stop(call.err_call(), message))?,
        )
    };
    let output = strings(ctx, previous.iter().map(|s| Some(s.as_str())))?;
    if let Some(replacement) = replacement {
        ctx.ext::<GraphicsExt>().palette = replacement;
    }
    Ok(output)
}

fn canonical(text: &str) -> Result<String, String> {
    let color = literal(text)?;
    if color.a == 0 {
        return Ok("transparent".to_owned());
    }
    let code = hex(color, color.a != 255);
    if color.a == 255
        && let Some((name, _)) = catalog().find(|(_, rgb)| *rgb == &code[1..])
    {
        return Ok(name.to_owned());
    }
    Ok(code)
}

fn text(ctx: &mut Ctx, args: &Args, i: usize) -> EvalResult<String> {
    let (value, _) = ctx.session_mut().coerce(args.get(i), SexpType::Str)?;
    let id = ctx
        .session()
        .str_slice(value)?
        .first()
        .copied()
        .unwrap_or(CharId::NA);
    if id == CharId::NA {
        Ok(String::new())
    } else {
        Ok(String::from_utf8_lossy(ctx.session().char_bytes(id)?).into_owned())
    }
}

pub(super) fn names(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let kind = text(ctx, args, 0)?;
    if kind == "palette" {
        return strings(ctx, discrete_catalog().map(|(name, _)| Some(name)));
    }
    let all = hcl_catalog();
    match kind.as_str() {
        "all" => strings(ctx, all.map(|(name, _)| Some(name))),
        "qualitative" => strings(ctx, all.take(9).map(|(name, _)| Some(name))),
        "sequential" => strings(ctx, all.skip(9).take(70).map(|(name, _)| Some(name))),
        "diverging" => strings(ctx, all.skip(79).take(18).map(|(name, _)| Some(name))),
        "divergingx" => strings(ctx, all.skip(97).map(|(name, _)| Some(name))),
        _ => Err(ctx.stop(call.err_call(), "invalid palette type")),
    }
}

pub(super) fn discrete(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let requested_count = if args.get(0) == SEXP_NIL {
        None
    } else {
        let n = number(ctx, args.get(0))?;
        if n.is_nan() {
            return Err(ctx.stop(call.err_call(), "missing value where TRUE/FALSE needed"));
        }
        if n < 1.0 {
            return strings(ctx, []);
        }
        Some(n as usize)
    };
    let requested = text(ctx, args, 1)?;
    let (name, data) = select(ctx, call, &requested, discrete_catalog())?;
    let (colors, labels) = data.split_once('\t').unwrap_or((data, ""));
    let colors: Vec<_> = colors.split_ascii_whitespace().collect();
    let mut n = requested_count.unwrap_or(colors.len());
    let recycle = flag(ctx, call, args.get(2), "invalid 'recycle' value")?;
    let names = flag(ctx, call, args.get(3), "invalid 'names' value")?;
    if !recycle && n > colors.len() {
        n = colors.len();
        ctx.warn_now(
            call.err_call(),
            format!("'n' set to {n}, the maximum available for {name} palette"),
        )?;
    }
    limit::<String>(ctx, call, n)?;
    ctx.scoped(|ctx| {
        let result = strings(ctx, colors.iter().copied().cycle().take(n).map(Some))?;
        if names && !labels.is_empty() {
            ctx.protect(result);
            let labels = strings(
                ctx,
                labels.split_ascii_whitespace().cycle().take(n).map(Some),
            )?;
            ctx.protect(labels);
            set_attr(ctx, result, "names", labels)?;
        }
        Ok(result)
    })
}

fn parse_param(token: &str) -> Option<f64> {
    if token == "NA" {
        None
    } else {
        token.parse::<f64>().ok()
    }
}

fn lintrj(t: f64, p1: f64, p2: f64) -> f64 {
    p2 - (p2 - p1) * t
}

fn tritrj(t: f64, j: f64, p1: f64, p2: f64, pm: f64) -> f64 {
    if t <= j {
        p2 - (p2 - pm) * t / j
    } else {
        pm - (pm - p1) * ((t - j) / (1.0 - j)).abs()
    }
}

fn hcl_point(h: f64, c: f64, l: f64, fixup: bool) -> Option<Rgba> {
    let rgb = hcl_rgb(h, c, l);
    let channels = rgb.map(|x| (255.0 * x + 0.5) as i32);
    if !fixup && channels.iter().any(|&v| !(0..=255).contains(&v)) {
        return None;
    }
    let [r, g, b] = channels.map(|v| v.clamp(0, 255) as u8);
    Some(Rgba::new(r, g, b, 255))
}

#[allow(clippy::too_many_arguments)]
fn eval_seq_hcl(
    ts: &[f64],
    h1: f64,
    h2: f64,
    c1: f64,
    c2: f64,
    l1: f64,
    l2: f64,
    p1: f64,
    p2: f64,
    cmax: Option<f64>,
    fixup: bool,
) -> Vec<Option<Rgba>> {
    let j = cmax.and_then(|cm| {
        let denom = (cm - c2).abs();
        let val = if denom == 0.0 {
            0.0
        } else {
            1.0 / (1.0 + (cm - c1).abs() / denom)
        };
        (val > 0.0 && val < 1.0).then_some(val)
    });
    ts.iter()
        .map(|&t| {
            let l = lintrj(t.powf(p2), l1, l2);
            let tp = t.powf(p1);
            let c = match (j, cmax) {
                (Some(j), Some(cm)) => tritrj(tp, j, c1, c2, cm),
                _ => lintrj(tp, c1, c2),
            };
            let h = lintrj(t, h1, h2);
            hcl_point(h, c, l, fixup)
        })
        .collect()
}

fn eval_hcl_palette(entry: HclEntry, n: usize, fixup: bool) -> Vec<Option<Rgba>> {
    let params: Vec<Option<f64>> = entry.params.split('\t').map(parse_param).collect();
    let get = |i: usize| params.get(i).copied().flatten();
    match entry.kind {
        HclKind::Qualitative => {
            let h1 = get(0).unwrap_or(0.0);
            let h2 = get(1).unwrap_or(h1 + 360.0 * (n.saturating_sub(1) as f64) / (n as f64));
            let c1 = get(2).unwrap_or(50.0);
            let l1 = get(4).unwrap_or(70.0);
            (0..n)
                .map(|i| {
                    let h = if n <= 1 {
                        h1
                    } else {
                        h1 + (h2 - h1) * (i as f64) / ((n - 1) as f64)
                    };
                    hcl_point(h, c1, l1, fixup)
                })
                .collect()
        }
        HclKind::Sequential => {
            let h1 = get(0).unwrap_or(0.0);
            let h2 = get(1).unwrap_or(h1);
            let c1 = get(2).unwrap_or(0.0);
            let c2 = get(3).unwrap_or(0.0);
            let l1 = get(4).unwrap_or(0.0);
            let l2 = get(5).unwrap_or(l1);
            let p1 = get(6).unwrap_or(1.0);
            let p2 = get(7).unwrap_or(p1);
            let cmax = get(8);
            let ts: Vec<f64> = if n <= 1 {
                vec![1.0]
            } else {
                (0..n)
                    .map(|i| 1.0 - (i as f64) / ((n - 1) as f64))
                    .collect()
            };
            eval_seq_hcl(&ts, h1, h2, c1, c2, l1, l2, p1, p2, cmax, fixup)
        }
        HclKind::Diverging => {
            let h1 = get(0).unwrap_or(0.0);
            let h2 = get(1).unwrap_or(h1);
            let c1 = get(2).unwrap_or(0.0);
            let l1 = get(4).unwrap_or(0.0);
            let l2 = get(5).unwrap_or(l1);
            let p1 = get(6).unwrap_or(1.0);
            let p2 = get(7).unwrap_or(p1);
            let cmax = get(8);
            let n2 = n.div_ceil(2);
            let ts: Vec<f64> = if n <= 1 {
                vec![0.0]
            } else {
                (0..n2)
                    .map(|k| (1.0 - 2.0 * (k as f64) / ((n - 1) as f64)).max(0.0))
                    .collect()
            };
            let mut left = eval_seq_hcl(&ts, h1, h1, c1, 0.0, l1, l2, p1, p2, cmax, fixup);
            let mut right = eval_seq_hcl(&ts, h2, h2, c1, 0.0, l1, l2, p1, p2, cmax, fixup);
            right.reverse();
            if n % 2 == 1 {
                left.pop();
            }
            left.extend(right);
            left
        }
        HclKind::DivergingX => {
            let h1 = get(0).unwrap_or(0.0);
            let h2 = get(1);
            let h3 = get(2).unwrap_or(h1);
            let c1 = get(3).unwrap_or(0.0);
            let c2 = get(4).unwrap_or(0.0);
            let c3 = get(5).unwrap_or(c1);
            let l1 = get(6).unwrap_or(0.0);
            let l2 = get(7).unwrap_or(l1);
            let l3 = get(8).unwrap_or(l1);
            let p1 = get(9).unwrap_or(1.0);
            let p2 = get(10).unwrap_or(p1);
            let p3 = get(11).unwrap_or(p1);
            let p4 = get(12).unwrap_or(p2);
            let cmax1 = get(13);
            let cmax2 = get(14);
            let n2 = n.div_ceil(2);
            let ts: Vec<f64> = if n <= 1 {
                vec![0.0]
            } else {
                (0..n2)
                    .map(|k| (1.0 - 2.0 * (k as f64) / ((n - 1) as f64)).max(0.0))
                    .collect()
            };
            let mut left = eval_seq_hcl(
                &ts,
                h1,
                h2.unwrap_or(h1),
                c1,
                c2,
                l1,
                l2,
                p1,
                p2,
                cmax1,
                fixup,
            );
            let mut right = eval_seq_hcl(
                &ts,
                h3,
                h2.unwrap_or(h3),
                c3,
                c2,
                l3,
                l2,
                p3,
                p4,
                cmax2,
                fixup,
            );
            right.reverse();
            if n % 2 == 1 {
                left.pop();
            }
            left.extend(right);
            left
        }
    }
}

pub(super) fn continuous(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let n = number(ctx, args.get(0))?;
    if n.is_nan() {
        return Err(ctx.stop(call.err_call(), "invalid 'n' value"));
    }
    let n = n as usize;
    if n == 0 {
        return strings(ctx, []);
    }
    limit::<String>(ctx, call, n)?;
    let requested = text(ctx, args, 1)?;
    let (name, _) = select(ctx, call, &requested, hcl_catalog())?;
    let Some(entry) = hcl_entries().find(|e| e.name == name) else {
        return Err(ctx.stop(
            call.err_call(),
            "'palette' does not match any given palette",
        ));
    };
    let alpha = optional_numbers(ctx, call, args.get(2))?;
    let rev = flag(ctx, call, args.get(3), "invalid 'rev' value")?;
    let (fixup_value, _) = ctx.session_mut().coerce(args.get(4), SexpType::Lgl)?;
    let fixup = ctx
        .session()
        .lgl(fixup_value)?
        .iter()
        .next()
        .and_then(|x| x.get())
        != Some(false);
    let mut values: Vec<_> = eval_hcl_palette(entry, n, fixup)
        .into_iter()
        .enumerate()
        .map(|(i, color)| {
            let Some(mut color) = color else {
                return Ok(None);
            };
            let a = recycled(&alpha, i, 1.0);
            color.a = if a.is_nan() { 255 } else { alpha_byte(a)? };
            Ok(Some(hex(color, !alpha.is_empty())))
        })
        .collect::<Result<_, String>>()
        .map_err(|message| ctx.stop(call.err_call(), message))?;
    if rev {
        values.reverse();
    }
    strings(ctx, values.iter().map(Option::as_deref))
}
