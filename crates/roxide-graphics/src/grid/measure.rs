//! Evaluate grid units in viewport coordinates.

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{EMPTY_ENV, SEXP_NIL, SexpType, ValueApi};

use crate::engine::{GContext, GraphicsExt};

use super::{fields::*, unit::string_metric};

const UNIT_CODES: &[(i32, &str, Option<f64>)] = &[
    (0, "npc", None),
    (1, "cm", Some(1.0 / 2.54)),
    (2, "inches", Some(1.0)),
    (3, "lines", None),
    (4, "native", None),
    (5, "null", None),
    (6, "snpc", None),
    (7, "mm", Some(1.0 / 25.4)),
    (8, "points", Some(1.0 / 72.27)),
    (9, "picas", Some(12.0 / 72.27)),
    (10, "bigpts", Some(1.0 / 72.0)),
    (11, "dida", Some((1238.0 / 1157.0) / 72.27)),
    (12, "cicero", Some((12.0 * 1238.0 / 1157.0) / 72.27)),
    (13, "scaledpts", Some(1.0 / (65536.0 * 72.27))),
    (14, "strwidth", None),
    (15, "strheight", None),
    (16, "strascent", None),
    (17, "strdescent", None),
    (18, "char", None),
    (19, "grobx", None),
    (20, "groby", None),
    (21, "grobwidth", None),
    (22, "grobheight", None),
    (23, "grobascent", None),
    (24, "grobdescent", None),
    (103, "mylines", None),
    (104, "mychar", None),
    (105, "mystrwidth", None),
    (106, "mystrheight", None),
    (SummaryOp::Sum as i32, "sum", None),
    (SummaryOp::Min as i32, "min", None),
    (SummaryOp::Max as i32, "max", None),
];

fn codes(ctx: &mut Ctx, call: &Call, value: SexpId) -> EvalResult<Vec<i32>> {
    if ctx.session().type_of(value)? != SexpType::Str {
        return Err(ctx.stop(call.err_call(), "'units' must be character"));
    }
    (0..ctx.session().xlength(value)?)
        .map(|i| {
            let name = text(ctx, value, i)?.unwrap_or_default();
            let code = match name.as_str() {
                "inch" | "in" => Some(2),
                "pt" | "point" => Some(8),
                "centimeter" | "centimeters" | "centimetre" | "centimetres" => Some(1),
                "millimeter" | "millimeters" | "millimetre" | "millimetres" => Some(7),
                _ => UNIT_CODES
                    .iter()
                    .find_map(|&(code, unit, _)| (unit == name).then_some(code)),
            };
            code.ok_or_else(|| ctx.stop(call.err_call(), "Invalid unit"))
        })
        .collect()
}

pub(super) fn simple(ctx: &mut Ctx, values: &[f64], code: i32) -> EvalResult {
    let x = ctx.session_mut().alloc_real_from(values)?;
    let code = ctx.session_mut().scalar_int(code)?;
    attrib(ctx, x, "unit", code)?;
    let class = strings(ctx, &["simpleUnit", "unit", "unit_v2"])?;
    attrib(ctx, x, "class", class)?;
    Ok(x)
}

pub(super) fn create(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let codes = codes(ctx, call, args.get(1))?;
    let x = super::numbers(ctx, call, args.get(0))?;
    if x.is_empty() || codes.is_empty() {
        return Err(ctx.stop(call.err_call(), "'x' and 'units' must have length > 0"));
    }
    if let [code] = codes.as_slice()
        && !matches!(code, 14..=17 | 19..=24)
    {
        return simple(ctx, &x, *code);
    }
    let n = x.len().max(codes.len());
    if n > ctx.host().limits().max_vector_bytes / size_of::<SexpId>() {
        return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
    }
    let data = args.get(2);
    let cls = attr(ctx, data, "class")?;
    let singleton = ctx.session().type_of(data)? == SexpType::Str
        || (0..ctx.session().xlength(cls)?).any(|i| {
            text(ctx, cls, i).is_ok_and(|s| matches!(s.as_deref(), Some("grob" | "gPath")))
        });
    let out = ctx.session_mut().alloc_list(n)?;
    ctx.protect(out);
    for i in 0..n {
        let code = codes[i % codes.len()];
        let mut datum = SEXP_NIL;
        if matches!(code, 14..=17 | 19..=24) {
            let len = ctx.session().xlength(data)?;
            datum = if singleton {
                data
            } else if len == 0 {
                SEXP_NIL
            } else {
                if ctx.session().type_of(data)? != SexpType::Vec {
                    return Err(ctx.stop(call.err_call(), "'data' must be a list"));
                }
                ctx.session().list_get(data, i % len)?
            };
            if matches!(code, 14..=17) && ctx.session().type_of(datum)? != SexpType::Str {
                return Err(ctx.stop(
                    call.err_call(),
                    "no string supplied for 'strwidth/height' unit",
                ));
            }
            if matches!(code, 19..=24) {
                let ns = ctx.ns_lookup("grid").unwrap_or(call.env);
                datum = ctx.call_function(".grid_grob_data", &[(None, datum)], ns)?;
                ctx.protect(datum);
            }
        }
        let value = ctx.session_mut().scalar_real(x[i % x.len()])?;
        let code = ctx.session_mut().scalar_int(code)?;
        let item = list(ctx, &[value, datum, code])?;
        ctx.session_mut().list_set(out, i, item)?;
    }
    let class = strings(ctx, &["unit", "unit_v2"])?;
    attrib(ctx, out, "class", class)?;
    Ok(out)
}

#[derive(Clone, Copy)]
pub(super) struct Item {
    value: f64,
    pub(super) code: i32,
    data: SexpId,
}

pub(super) struct Units {
    value: SexpId,
    simple: Option<i32>,
    pub(super) len: usize,
}

impl Units {
    pub(super) fn read(ctx: &mut Ctx, value: SexpId) -> EvalResult<Self> {
        let code = attr(ctx, value, "unit")?;
        let simple = (code != SEXP_NIL)
            .then(|| real(ctx, code, 0, -1.0))
            .transpose()?;
        Ok(Self {
            value,
            simple: simple.map(|x| x as i32),
            len: ctx.session().xlength(value)?,
        })
    }

    pub(super) fn at(&self, ctx: &mut Ctx, i: usize) -> EvalResult<Item> {
        if self.len == 0 {
            return Err(ctx.stop(SEXP_NIL, "Cannot create zero-length unit vector"));
        }
        if let Some(code) = self.simple {
            return Ok(Item {
                value: real(ctx, self.value, i, f64::NAN)?,
                code,
                data: SEXP_NIL,
            });
        }
        let value = ctx.session().list_get(self.value, i % self.len)?;
        let item = ctx.session().list_slice(value)?;
        let &[x, data, code] = item else {
            return Err(ctx.stop(SEXP_NIL, "invalid unit"));
        };
        Ok(Item {
            value: real(ctx, x, 0, f64::NAN)?,
            code: real(ctx, code, 0, -1.0)? as i32,
            data,
        })
    }
}

#[derive(Clone, Copy)]
enum SummaryOp {
    Sum = 201,
    Min = 202,
    Max = 203,
}

impl SummaryOp {
    fn from_code(code: i32) -> Option<Self> {
        match code {
            x if x == Self::Sum as i32 => Some(Self::Sum),
            x if x == Self::Min as i32 => Some(Self::Min),
            x if x == Self::Max as i32 => Some(Self::Max),
            _ => None,
        }
    }

    fn identity(self) -> f64 {
        match self {
            Self::Sum => 0.0,
            Self::Min => f64::INFINITY,
            Self::Max => f64::NEG_INFINITY,
        }
    }

    fn combine(self, a: f64, b: f64) -> f64 {
        if a.is_nan() || b.is_nan() {
            return a + b;
        }
        match self {
            Self::Sum => a + b,
            Self::Min => a.min(b),
            Self::Max => a.max(b),
        }
    }
}

impl Item {
    pub(super) fn weight(self, ctx: &mut Ctx, depth: usize) -> EvalResult<f64> {
        if self.code == 5 {
            Ok(self.value)
        } else if let Some(op) = SummaryOp::from_code(self.code) {
            self.reduce(ctx, op, depth, |ctx, item| item.weight(ctx, depth + 1))
        } else {
            Ok(f64::NAN)
        }
    }

    fn reduce(
        self,
        ctx: &mut Ctx,
        op: SummaryOp,
        depth: usize,
        mut term: impl FnMut(&mut Ctx, Item) -> EvalResult<f64>,
    ) -> EvalResult<f64> {
        if depth >= 128 {
            return Err(ctx.stop(SEXP_NIL, "unit expression is too deeply nested"));
        }
        let units = Units::read(ctx, self.data)?;
        let mut result = op.identity();
        for i in 0..units.len {
            let item = units.at(ctx, i)?;
            result = op.combine(result, term(ctx, item)?);
        }
        Ok(self.value * result)
    }
}

#[derive(Clone, Copy)]
pub(super) struct Space {
    pub(super) size: [f64; 2],
    scale: [[f64; 2]; 2],
    pub(super) gp: SexpId,
    pub(super) node: SexpId,
}

impl Space {
    pub(super) fn read(ctx: &mut Ctx, node: SexpId) -> EvalResult<Self> {
        let size = get(ctx, node, "size")?;
        let vp = get(ctx, node, "vp")?;
        let size = [real(ctx, size, 0, 0.0)?, real(ctx, size, 1, 0.0)?];
        Self::new(ctx, node, size, vp)
    }

    pub(super) fn new(ctx: &mut Ctx, node: SexpId, size: [f64; 2], vp: SexpId) -> EvalResult<Self> {
        let mut scale = [[0.0, 1.0]; 2];
        for (i, name) in ["xscale", "yscale"].into_iter().enumerate() {
            let value = get(ctx, vp, name)?;
            scale[i] = [real(ctx, value, 0, 0.0)?, real(ctx, value, 1, 1.0)?];
        }
        Ok(Self {
            size,
            scale,
            gp: get(ctx, node, "gp")?,
            node,
        })
    }

    pub(super) fn measure(
        self,
        ctx: &mut Ctx,
        item: Item,
        axis: usize,
        position: bool,
        i: usize,
    ) -> EvalResult<f64> {
        self.inches(ctx, item, axis, position, i, (SummaryOp::Sum, 0))
    }

    fn inches(
        self,
        ctx: &mut Ctx,
        item: Item,
        axis: usize,
        position: bool,
        i: usize,
        (mode, depth): (SummaryOp, usize),
    ) -> EvalResult<f64> {
        let size = self.size[axis];
        let [a, b] = self.scale[axis];
        let value = item.value;
        let scale = match item.code {
            0 => size,
            4 => return Ok((value - if position { a } else { 0.0 }) / (b - a) * size),
            5 => {
                return Ok(match mode {
                    SummaryOp::Min => size * 2.54,
                    SummaryOp::Max => f64::MIN_POSITIVE,
                    SummaryOp::Sum => 0.0,
                });
            }
            6 => self.size[0].min(self.size[1]),
            3 | 18 | 103 | 104 => {
                let gc = font(ctx, self.gp, i)?;
                gc.ps * gc.cex / 72.0
                    * if matches!(item.code, 3 | 103) {
                        gc.lineheight
                    } else {
                        1.0
                    }
            }
            14..=17 | 105..=106 => {
                let gc = font(ctx, self.gp, i)?;
                let label = text(ctx, item.data, 0)?.unwrap_or_else(|| "NA".to_owned());
                let Some(slot) = ctx.ext::<GraphicsExt>().active() else {
                    return Err(ctx.stop(SEXP_NIL, "no active graphics device"));
                };
                string_metric(slot.device.as_ref(), &gc, &label, Some(item.code)) / 72.0
            }
            19..=24 => {
                return ctx.scoped(|ctx| {
                    let code = ctx.session_mut().scalar_int(item.code)?;
                    let value = ctx.session_mut().scalar_real(value)?;
                    let ns = ctx.ns_lookup("grid").unwrap_or(EMPTY_ENV);
                    let result = ctx.call_function(
                        ".grid_grob_measure",
                        &[
                            (None, item.data),
                            (None, code),
                            (None, value),
                            (None, self.node),
                        ],
                        ns,
                    )?;
                    real(ctx, result, 0, f64::NAN)
                });
            }
            code => {
                if let Some(op) = SummaryOp::from_code(code) {
                    return item.reduce(ctx, op, depth, |ctx, child| {
                        self.inches(ctx, child, axis, position, i, (op, depth + 1))
                    });
                }
                let scale = UNIT_CODES
                    .iter()
                    .find_map(|&(key, _, scale)| if key == code { scale } else { None });
                scale
                    .ok_or_else(|| ctx.stop(SEXP_NIL, "invalid unit or unit not yet implemented"))?
            }
        };
        Ok(value * scale)
    }

    fn fraction(ctx: &mut Ctx, inches: f64, size: f64) -> EvalResult<f64> {
        if size.abs() >= 1e-6 {
            return Ok(inches / size);
        }
        if inches == 0.0 {
            return Ok(0.0);
        }
        Err(ctx.stop(SEXP_NIL, "Viewport has zero dimension(s)"))
    }

    fn inverse(
        self,
        ctx: &mut Ctx,
        inches: f64,
        code: i32,
        axis: usize,
        position: bool,
        i: usize,
    ) -> EvalResult<f64> {
        let size = self.size[axis];
        let [a, b] = self.scale[axis];
        match code {
            0 => Self::fraction(ctx, inches, size),
            4 if size.abs() < 1e-6 && inches == 0.0 => Ok(0.0),
            4 => Ok(Self::fraction(ctx, inches, size)? * (b - a) + if position { a } else { 0.0 }),
            6 => Self::fraction(ctx, inches, self.size[0].min(self.size[1])),
            1..=3 | 7..=13 | 18 => Ok(inches
                / self.measure(
                    ctx,
                    Item {
                        value: 1.0,
                        code,
                        data: SEXP_NIL,
                    },
                    axis,
                    position,
                    i,
                )?),
            _ => Err(ctx.stop(SEXP_NIL, "invalid unit or unit not yet implemented")),
        }
    }
}

pub(super) fn font(ctx: &mut Ctx, gp: SexpId, i: usize) -> EvalResult<GContext> {
    let family = get(ctx, gp, "fontfamily")?;
    let ps = num(ctx, gp, "fontsize", i, 12.0)?;
    let cex = num(ctx, gp, "cex", i, 1.0)?;
    let (ps, cex) = if ps == 0.0 { (1.0, 0.0) } else { (ps, cex) };
    Ok(GContext {
        ps,
        cex,
        fontface: num(ctx, gp, "font", i, 1.0)? as u8,
        fontfamily: text(ctx, family, i)?.unwrap_or_default(),
        lineheight: num(ctx, gp, "lineheight", i, 1.2)?,
        ..GContext::default()
    })
}

pub(super) fn convert(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    if !inherits(ctx, args.get(0), "unit")? {
        return Err(ctx.stop(call.err_call(), "Not a unit object"));
    }
    let units = Units::read(ctx, args.get(0))?;
    let codes = codes(ctx, call, args.get(1))?;
    if codes.is_empty() || units.len > ctx.host().limits().max_vector_bytes / size_of::<f64>() {
        return Err(ctx.stop(call.err_call(), "invalid unit or unit not yet implemented"));
    }
    let from = super::viewport::choice(ctx, call, args.get(2), &["x", "y"])?;
    let to = super::viewport::choice(ctx, call, args.get(4), &["x", "y"])?;
    let kinds = ["location", "dimension"];
    let from_pos = super::viewport::choice(ctx, call, args.get(3), &kinds)? == 0;
    let to_pos = super::viewport::choice(ctx, call, args.get(5), &kinds)? == 0;
    let node = if args.get(6) == SEXP_NIL {
        let state = super::viewport::state(ctx, call, &mut Args::from_pairs(Vec::new()))?;
        get(ctx, state, "current")?
    } else {
        args.get(6)
    };
    let space = Space::read(ctx, node)?;
    let mut output = Vec::with_capacity(units.len);
    for i in 0..units.len {
        let item = units.at(ctx, i)?;
        let code = codes[i % codes.len()];
        let value = if matches!(item.code, 0 | 4) && matches!(code, 0 | 4) && from == to {
            let [a, b] = space.scale[from];
            let v = if item.code == 4 {
                (item.value - if from_pos { a } else { 0.0 }) / (b - a)
            } else {
                item.value
            };
            if code == 4 {
                v * (b - a) + if to_pos { a } else { 0.0 }
            } else {
                v
            }
        } else {
            let inches = space.measure(ctx, item, from, from_pos, i)?;
            space.inverse(ctx, inches, code, to, to_pos, i)?
        };
        output.push(value);
    }
    let value = ctx.session_mut().alloc_real_from(&output)?;
    let (only, _) = ctx.session_mut().coerce(args.get(7), SexpType::Lgl)?;
    let only = ctx.session().lgl(only)?;
    let scalar = only.iter().next().and_then(roxide_value::RLogical::get);
    match scalar {
        Some(true) => Ok(value),
        Some(false) => create(
            ctx,
            call,
            &mut Args::from_pairs(vec![(None, value), (None, args.get(1)), (None, SEXP_NIL)]),
        ),
        None => Err(ctx.stop(call.err_call(), "missing value where TRUE/FALSE needed")),
    }
}

impl Item {
    fn value_list(self, ctx: &mut Ctx) -> EvalResult {
        let value = ctx.session_mut().scalar_real(self.value)?;
        let code = ctx.session_mut().scalar_int(self.code)?;
        list(ctx, &[value, self.data, code])
    }
}

fn vector(ctx: &mut Ctx, items: impl IntoIterator<Item = Item>) -> EvalResult {
    let items = items
        .into_iter()
        .map(|item| item.value_list(ctx))
        .collect::<EvalResult<Vec<_>>>()?;
    let value = list(ctx, &items)?;
    let class = strings(ctx, &["unit", "unit_v2"])?;
    attrib(ctx, value, "class", class)?;
    Ok(value)
}

pub(super) fn items(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let value = args.get(0);
    if !inherits(ctx, value, "unit")? {
        return Err(ctx.stop(call.err_call(), "object is not coercible to a unit"));
    }
    let units = Units::read(ctx, value)?;
    if units.len > ctx.host().limits().max_vector_bytes / size_of::<SexpId>() {
        return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
    }
    if units.simple.is_none() {
        let values = ctx.session().list_slice(value)?.to_vec();
        return list(ctx, &values);
    }
    let mut values = Vec::with_capacity(units.len);
    for i in 0..units.len {
        values.push(units.at(ctx, i)?.value_list(ctx)?);
    }
    list(ctx, &values)
}

fn terms(ctx: &mut Ctx, item: Item, result: &mut Vec<Item>) -> EvalResult<()> {
    if item.code == SummaryOp::Sum as i32 && item.data != SEXP_NIL {
        let children = Units::read(ctx, item.data)?;
        for i in 0..children.len {
            let mut child = children.at(ctx, i)?;
            child.value *= item.value;
            result.push(child);
        }
    } else {
        result.push(item);
    }
    Ok(())
}

pub(super) fn add(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let a = Units::read(ctx, args.get(0))?;
    let b = Units::read(ctx, args.get(1))?;
    let n = a.len.max(b.len);
    if n > ctx.host().limits().max_vector_bytes / size_of::<Item>() {
        return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
    }
    if let (Some(c), Some(d)) = (a.simple, b.simple)
        && c == d
    {
        let mut output = Vec::with_capacity(n);
        for i in 0..n {
            output.push(a.at(ctx, i)?.value + b.at(ctx, i)?.value);
        }
        return simple(ctx, &output, c);
    }
    let mut output = Vec::with_capacity(n);
    for i in 0..n {
        let mut first = a.at(ctx, i)?;
        let second = b.at(ctx, i)?;
        if first.code < SummaryOp::Sum as i32
            && first.code == second.code
            && roxide_value::identical::identical(
                ctx.session(),
                first.data,
                second.data,
                Default::default(),
            )?
        {
            first.value += second.value;
        } else {
            let mut sum = Vec::new();
            terms(ctx, first, &mut sum)?;
            terms(ctx, second, &mut sum)?;
            first = Item {
                value: 1.0,
                code: SummaryOp::Sum as i32,
                data: vector(ctx, sum)?,
            };
        }
        output.push(first);
    }
    vector(ctx, output)
}

fn unit_name(ctx: &mut Ctx, code: i32) -> EvalResult<&'static str> {
    UNIT_CODES
        .iter()
        .find_map(|&(key, name, _)| (key == code).then_some(name))
        .ok_or_else(|| ctx.stop(SEXP_NIL, "invalid unit"))
}

fn format_unit(ctx: &mut Ctx, item: Item, depth: usize) -> EvalResult<String> {
    if depth >= 128 {
        return Err(ctx.stop(SEXP_NIL, "unit expression is too deeply nested"));
    }
    let name = unit_name(ctx, item.code)?;
    let numeric = ctx.session_mut().scalar_real(item.value)?;
    let (string, _) = ctx.session_mut().coerce(numeric, SexpType::Str)?;
    let value = text(ctx, string, 0)?.unwrap_or_else(|| "NA".into());
    if item.code < SummaryOp::Sum as i32 {
        return Ok(format!("{value}{name}"));
    }
    let items = Units::read(ctx, item.data)?;
    let mut expression = String::new();
    if item.value != 1.0 {
        expression.push_str(&value);
        expression.push('*');
    }
    expression.push_str(name);
    expression.push('(');
    for i in 0..items.len {
        if i != 0 {
            expression.push_str(", ");
        }
        let item = items.at(ctx, i)?;
        expression.push_str(&format_unit(ctx, item, depth + 1)?);
        if expression.len() > ctx.host().limits().max_vector_bytes {
            return Err(ctx.stop(SEXP_NIL, "vector size limit exceeded"));
        }
    }
    expression.push(')');
    Ok(expression)
}

pub(super) fn as_char(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let units = Units::read(ctx, args.get(0))?;
    let limit = ctx.host().limits().max_vector_bytes;
    let mut output = Vec::new();
    let mut bytes = 0_usize;
    for i in 0..units.len {
        let item = units.at(ctx, i)?;
        let value = format_unit(ctx, item, 0)?;
        bytes = bytes.saturating_add(value.len() + size_of::<roxide_value::CharId>());
        if bytes > limit {
            return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
        }
        output.push(
            ctx.session_mut()
                .mk_char(value.as_bytes(), roxide_value::Encoding::Native)?,
        );
    }
    Ok(ctx.session_mut().alloc_str_from(&output)?)
}

fn unit_types(ctx: &mut Ctx, units: Units, recursive: bool, depth: usize) -> EvalResult {
    if depth >= 128 || units.len > ctx.host().limits().max_vector_bytes / size_of::<SexpId>() {
        return Err(ctx.stop(SEXP_NIL, "unit expression exceeds size limit"));
    }
    let mut values = Vec::with_capacity(units.len);
    let mut labels = Vec::with_capacity(units.len);
    for i in 0..units.len {
        let item = units.at(ctx, i)?;
        let name = unit_name(ctx, item.code)?;
        labels.push(name);
        if recursive {
            values.push(
                if item.code >= SummaryOp::Sum as i32 && item.data != SEXP_NIL {
                    let children = Units::read(ctx, item.data)?;
                    unit_types(ctx, children, true, depth + 1)?
                } else {
                    ctx.session_mut().scalar_str(name)?
                },
            );
        }
    }
    let labels = strings(ctx, &labels)?;
    if recursive {
        let x = list(ctx, &values)?;
        attrib(ctx, x, "names", labels)?;
        Ok(x)
    } else {
        Ok(labels)
    }
}

pub(super) fn types(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    if !inherits(ctx, args.get(0), "unit")? {
        return Err(ctx.stop(call.err_call(), "object is not coercible to a unit"));
    }
    let units = Units::read(ctx, args.get(0))?;
    let recursive = real(ctx, args.get(1), 0, 0.0)? == 1.0;
    unit_types(ctx, units, recursive, 0)
}

fn identical_data(ctx: &Ctx, a: Item, b: Item) -> EvalResult<bool> {
    Ok(a.code == b.code
        && roxide_value::identical::identical(ctx.session(), a.data, b.data, Default::default())?)
}

pub(super) fn extrema(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let values = ctx.session().list_slice(args.get(0))?.to_vec();
    let code = real(ctx, args.get(1), 0, f64::from(SummaryOp::Min as i32))? as i32;
    let op = SummaryOp::from_code(code).unwrap_or(SummaryOp::Sum);
    let units = values
        .iter()
        .map(|&value| {
            if !inherits(ctx, value, "unit")? {
                return Err(ctx.stop(call.err_call(), "object is not coercible to a unit"));
            }
            Units::read(ctx, value)
        })
        .collect::<EvalResult<Vec<_>>>()?;
    let Some(first) = units.first() else {
        return vector(ctx, []);
    };
    let n = units.iter().map(|unit| unit.len).max().unwrap_or(0);
    if n > ctx.host().limits().max_vector_bytes / size_of::<Item>() {
        return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
    }
    let same = first.simple.is_some() && units.iter().all(|unit| unit.simple == first.simple);
    let mut same_run = true;
    let mut output = Vec::with_capacity(n);
    let mut items = Vec::with_capacity(units.len());
    for i in 0..n {
        items.clear();
        let mut item = first.at(ctx, i)?;
        for unit in &units {
            items.push(unit.at(ctx, i)?);
        }
        same_run &= item.code < SummaryOp::Sum as i32;
        if same_run {
            for &candidate in &items {
                same_run &= identical_data(ctx, item, candidate)?;
            }
        }
        if same_run {
            item.value = items
                .iter()
                .skip(1)
                .fold(item.value, |x, child| op.combine(x, child.value));
        } else {
            item = Item {
                value: 1.0,
                code,
                data: vector(ctx, items.iter().copied())?,
            };
        }
        output.push(item);
    }
    if same {
        let simple = simple(
            ctx,
            &output.iter().map(|item| item.value).collect::<Vec<_>>(),
            first.simple.unwrap_or(0),
        )?;
        let code = attr(ctx, simple, "unit")?;
        attrib(ctx, simple, "unit", SEXP_NIL)?;
        attrib(ctx, simple, "unit", code)?;
        Ok(simple)
    } else {
        vector(ctx, output)
    }
}
