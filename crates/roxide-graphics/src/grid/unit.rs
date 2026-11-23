//! Device metric conversions for grid string units.

use std::collections::HashMap;

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_value::{CharId, NA_INTEGER, SexpType, ValueApi};

use crate::engine::{Device, GContext};

pub(super) fn match_indices(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let input = ctx.session().xlength(args.get(0))?;
    let table = ctx.session().xlength(args.get(1))?;
    let limit = ctx.host().limits().max_vector_bytes;
    if input > limit / size_of::<i32>()
        || table > limit / size_of::<(&[u8], i32)>()
        || table > i32::MAX as usize
    {
        return Err(ctx.stop(call.err_call(), "vector size limit exceeded"));
    }
    if input == 0 || table == 0 {
        return Ok(ctx.session_mut().alloc_int_from(&vec![NA_INTEGER; input])?);
    }
    ctx.scoped(|ctx| {
        let (input, _) = ctx.session_mut().coerce(args.get(0), SexpType::Str)?;
        ctx.protect(input);
        let (table, _) = ctx.session_mut().coerce(args.get(1), SexpType::Str)?;
        let s = ctx.session();
        let mut positions = HashMap::new();
        for (i, &item) in s.str_slice(table)?.iter().enumerate() {
            if item != CharId::NA {
                positions
                    .entry(s.char_bytes(item)?)
                    .or_insert((i + 1) as i32);
            }
        }
        let mut indices = Vec::with_capacity(s.xlength(input)?);
        for &item in s.str_slice(input)? {
            let index = if item == CharId::NA {
                NA_INTEGER
            } else {
                positions
                    .get(s.char_bytes(item)?)
                    .copied()
                    .unwrap_or(NA_INTEGER)
            };
            indices.push(index);
        }
        Ok(ctx.session_mut().alloc_int_from(&indices)?)
    })
}

pub(super) fn string_metric(
    device: &dyn Device,
    gc: &GContext,
    text: &str,
    code: Option<i32>,
) -> f64 {
    let cap_height = device.metric_info('M', gc).0;
    let line_step = 1.2 * gc.lineheight * gc.ps * gc.cex;
    let mut width: f64 = 0.0;
    let mut ascent: f64 = 0.0;
    let mut descent: f64 = 0.0;
    let mut extra = 0.0;
    for (index, line) in text.split('\n').enumerate() {
        width = width.max(device.str_width(line, gc));
        let mut top: f64 = 0.0;
        let mut bottom: f64 = 0.0;
        for ch in line.chars() {
            let (a, d, _) = device.metric_info(ch, gc);
            top = top.max(a);
            bottom = bottom.max(d);
        }
        if index == 0 {
            ascent = top;
        }
        descent = bottom;
        extra = index as f64 * line_step;
    }
    match code {
        Some(14 | 105) => width,
        Some(15 | 106) => cap_height + extra,
        Some(16) => ascent + extra,
        Some(17) => descent,
        _ => 0.0,
    }
}
