use std::collections::BTreeMap;

use roxide_eval::{Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{NA_REAL, RLogical, SexpType, ValueApi};

use crate::{engine::Rgba, grdevices::parse_rgba, stroke::LINE_TYPES};

use super::{active, alloc_strings, flag, invalid, nums, state, string, vector};

#[derive(Clone)]
pub(in crate::graphics) enum Param {
    Real(f64),
    Int(i32),
    Flag(Option<bool>),
    Text(String),
    Color(String, Rgba),
    Reals(Vec<f64>),
    Ints(Vec<i32>),
}

impl Param {
    pub(super) fn alloc(&self, ctx: &mut Ctx) -> EvalResult {
        Ok(match self {
            Self::Real(x) => ctx.session_mut().alloc_real_from(&[*x])?,
            Self::Int(x) => ctx.session_mut().alloc_int_from(&[*x])?,
            Self::Flag(x) => ctx
                .session_mut()
                .alloc_lgl_from(&[RLogical::from_option(*x)])?,
            Self::Text(x) | Self::Color(x, _) => alloc_strings(ctx, [x.as_str()])?,
            Self::Reals(x) => ctx.session_mut().alloc_real_from(x)?,
            Self::Ints(x) => ctx.session_mut().alloc_int_from(x)?,
        })
    }

    pub(super) fn kind(&self) -> ParamKind {
        match self {
            Self::Real(_) => ParamKind::Real,
            Self::Int(_) => ParamKind::Int,
            Self::Flag(_) => ParamKind::Flag,
            Self::Text(_) => ParamKind::Text,
            Self::Color(..) => ParamKind::Color,
            Self::Reals(_) => ParamKind::Reals,
            Self::Ints(_) => ParamKind::Ints,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum ParamKind {
    Real,
    Int,
    Flag,
    Text,
    Color,
    Reals,
    Ints,
}

pub(super) fn defaults(ps: f64, bg: Rgba) -> BTreeMap<&'static str, Param> {
    use Param::*;
    let mut params = BTreeMap::from([
        ("adj", Real(0.5)),
        ("ann", Flag(Some(true))),
        ("ask", Flag(Some(false))),
        ("bg", Color(color_name(bg), bg)),
        ("bty", Text("o".into())),
        ("cex", Real(1.0)),
        ("cex.axis", Real(1.0)),
        ("cex.lab", Real(1.0)),
        ("cex.main", Real(1.2)),
        ("cex.sub", Real(1.0)),
        ("crt", Real(0.0)),
        ("err", Int(0)),
        ("family", Text(String::new())),
        ("font", Int(1)),
        ("font.axis", Int(1)),
        ("font.lab", Int(1)),
        ("font.main", Int(2)),
        ("font.sub", Int(1)),
        ("lab", Ints(vec![5, 5, 7])),
        ("las", Int(0)),
        ("lend", Text("round".into())),
        ("lheight", Real(1.0)),
        ("ljoin", Text("round".into())),
        ("lmitre", Real(10.0)),
        ("lty", Text("solid".into())),
        ("lwd", Real(1.0)),
        ("mex", Real(1.0)),
        ("mgp", Reals(vec![3.0, 1.0, 0.0])),
        ("mkh", Real(0.001)),
        ("pch", Int(1)),
        ("ps", Int(ps as i32)),
        ("pty", Text("m".into())),
        ("smo", Real(1.0)),
        ("srt", Real(0.0)),
        ("tck", Real(NA_REAL)),
        ("tcl", Real(-0.5)),
        ("xaxs", Text("r".into())),
        ("xaxt", Text("s".into())),
        ("xpd", Flag(Some(false))),
        ("yaxs", Text("r".into())),
        ("yaxt", Text("s".into())),
        ("ylbias", Real(0.2)),
    ]);
    for name in ["col", "col.axis", "col.lab", "col.main", "col.sub", "fg"] {
        params.insert(name, Color("black".into(), Rgba::BLACK));
    }
    params
}

pub(in crate::graphics) fn choice(
    ctx: &mut Ctx,
    x: SexpId,
    choices: &[&str],
) -> EvalResult<Option<String>> {
    let kind = ctx.session().type_of(x)?;
    let selected = if matches!(kind, SexpType::Int | SexpType::Real) {
        let x = nums(ctx, x)?.first().copied().unwrap_or(NA_REAL);
        if x.is_finite() && x >= 0.0 {
            choices.get(x as usize).copied()
        } else {
            None
        }
    } else {
        let x = string(ctx, x)?;
        choices.iter().copied().find(|&name| name == x)
    };
    Ok(selected.map(str::to_owned))
}

pub(in crate::graphics) fn line_type(ctx: &mut Ctx, value: SexpId) -> EvalResult<Option<String>> {
    if matches!(
        ctx.session().type_of(value)?,
        SexpType::Int | SexpType::Real
    ) {
        let value = nums(ctx, value)?.first().copied().unwrap_or(NA_REAL);
        return Ok(if value == 0.0 {
            Some("blank".into())
        } else if value.is_finite() && value >= 1.0 && value <= f64::from(i32::MAX) {
            Some(LINE_TYPES[1 + (value as usize - 1) % (LINE_TYPES.len() - 1)].into())
        } else {
            None
        });
    }
    if let Some(name) = choice(ctx, value, &LINE_TYPES)? {
        return Ok(Some(name));
    }
    let text = string(ctx, value)?;
    Ok((matches!(text.len(), 2 | 4 | 6 | 8)
        && text.bytes().all(|b| b.is_ascii_hexdigit() && b != b'0'))
    .then(|| text.to_ascii_uppercase()))
}

fn color_name(color: Rgba) -> String {
    let Rgba { r, g, b, a } = color;
    match a {
        0 => "transparent".into(),
        255 => {
            let hex = format!("{r:02X}{g:02X}{b:02X}");
            include_str!("../../grdevices/named_colors.tsv")
                .lines()
                .filter_map(|line| line.split_once('\t'))
                .find_map(|(name, value)| (value == hex).then(|| name.to_owned()))
                .unwrap_or_else(|| format!("#{hex}"))
        }
        _ => format!("#{r:02X}{g:02X}{b:02X}{a:02X}"),
    }
}

pub(super) fn parse(
    ctx: &mut Ctx,
    call: &Call,
    name: &str,
    kind: ParamKind,
    value: SexpId,
) -> EvalResult<Param> {
    use Param::*;
    let kind = if name == "pch" {
        if ctx.session().type_of(value)? == SexpType::Str {
            ParamKind::Text
        } else {
            ParamKind::Int
        }
    } else {
        kind
    };
    let result = match kind {
        ParamKind::Color => {
            let rgba = parse_rgba(ctx, value)?;
            let spelling = string(ctx, value)?;
            let rgba = if spelling.parse::<f64>().is_ok_and(|x| x.trunc() == 0.0) {
                state(active(ctx, call, true)?)?.color("bg")
            } else {
                rgba
            };
            let selected = color_name(rgba);
            Color(selected, rgba)
        }
        ParamKind::Flag => {
            let flag = flag(ctx, value)?;
            if flag.is_none() && name != "xpd" {
                return Err(invalid(ctx, call, name));
            }
            Flag(flag)
        }
        ParamKind::Int => {
            let x = vector::<1>(ctx, call, name, value)?[0];
            if x <= i32::MIN as f64 || x > i32::MAX as f64 {
                return Err(invalid(ctx, call, name));
            }
            let x = x as i32;
            let valid = match name {
                "las" => (0..=3).contains(&x),
                "err" => matches!(x, -1 | 0),
                "ps" => x >= 0,
                n if n.starts_with("font") => x >= 1,
                _ => true,
            };
            if !valid {
                return Err(invalid(ctx, call, name));
            }
            Int(x)
        }
        ParamKind::Text => {
            let text = match name {
                "lend" => choice(ctx, value, &["round", "butt", "square"])?,
                "ljoin" => choice(ctx, value, &["round", "mitre", "bevel"])?,
                "lty" => line_type(ctx, value)?,
                "family" => Some(string(ctx, value)?),
                _ => {
                    let first = string(ctx, value)?.chars().next();
                    let valid = match name {
                        "bty" => "ol7cun]OLCU",
                        "pty" => "ms",
                        "xaxs" | "yaxs" => "riesd",
                        "xaxt" | "yaxt" => "sln",
                        _ => "",
                    };
                    first
                        .filter(|c| name == "pch" || valid.contains(*c))
                        .map(|c| c.to_string())
                }
            };
            Text(text.ok_or_else(|| invalid(ctx, call, name))?)
        }
        ParamKind::Real => {
            let x = nums(ctx, value)?;
            let [x] = x.as_slice() else {
                return Err(invalid(ctx, call, name));
            };
            let valid = match name {
                "tck" | "tcl" => !x.is_infinite(),
                "adj" => (0.0..=1.0).contains(x),
                "lmitre" => x.is_finite() && *x >= 1.0,
                n if n.starts_with("cex") || matches!(n, "mex" | "lwd" | "lheight") => {
                    x.is_finite() && *x > 0.0
                }
                _ => x.is_finite(),
            };
            if !valid {
                return Err(invalid(ctx, call, name));
            }
            Real(*x)
        }
        ParamKind::Ints => {
            let x = vector::<3>(ctx, call, name, value)?;
            if x.iter().any(|x| *x < 0.0 || *x > i32::MAX as f64) {
                return Err(invalid(ctx, call, name));
            }
            Ints(x.map(|x| x as i32).to_vec())
        }
        ParamKind::Reals => Reals(vector::<3>(ctx, call, name, value)?.to_vec()),
    };
    Ok(result)
}
