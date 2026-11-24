use crate::graphics::coords::rect;

use super::{
    Call, Ctx, EvalResult, Margins, Panel, Param, Region, SEXP_NIL, SexpId, SexpType, ValueApi,
    active, alloc_strings, flag, invalid, names, nums, set, state, stored, vector,
};

pub(in crate::graphics) fn snapshot(ctx: &mut Ctx, call: &Call) -> EvalResult {
    let slot = active(ctx, call, true)?;
    let layout = slot.par_state;
    let frame = slot.frame_open;
    let par = state(slot)?;
    let mut params: Vec<_> = par
        .params
        .iter()
        .map(|(name, value)| (*name, value.clone()))
        .collect();
    let mut geometry = vec![
        (if matches!(par.mar, Margins::Lines(_)) {
            "mar"
        } else {
            "mai"
        }),
        (if par.omd.is_some() {
            "omd"
        } else if matches!(par.oma, Margins::Lines(_)) {
            "oma"
        } else {
            "omi"
        }),
    ];
    match &par.panel {
        Panel::Grid { by_row, .. } => geometry.insert(0, if *by_row { "mfrow" } else { "mfcol" }),
        Panel::Custom(Region::Size(_)) => geometry.insert(0, "fin"),
        Panel::Custom(Region::Normalized(_)) => geometry.insert(0, "fig"),
        Panel::Layout(_) => {}
    }
    match par.plot {
        Some(Region::Normalized(_)) => geometry.push("plt"),
        Some(Region::Size(_)) => geometry.push("pin"),
        None => {}
    }
    geometry.extend(["mfg", "xlog", "ylog", "xaxp", "yaxp", "new"]);
    let layout = if matches!(par.panel, Panel::Layout(_)) {
        layout
    } else {
        SEXP_NIL
    };
    let layout_and_margins = geometry
        .into_iter()
        .filter_map(|name| par.get(name).map(|v| (name, v)));
    params.splice(..0, layout_and_margins);
    params.extend([
        (".usr", Param::Reals(par.usr.to_vec())),
        (".logusr", Param::Reals(par.logusr.to_vec())),
        (".frame", Param::Flag(Some(frame))),
        (".csi", Param::Real(par.csi)),
    ]);
    let clip = par.clip;
    ctx.scoped(|ctx| {
        ctx.protect(layout);
        let result = ctx.session_mut().alloc_list(params.len() + 2)?;
        ctx.protect(result);
        for (i, (_, param)) in params.iter().enumerate() {
            let value = param.alloc(ctx)?;
            ctx.session_mut().list_set(result, i, value)?;
        }
        ctx.session_mut().list_set(result, params.len(), layout)?;
        if let Some(clip) = clip {
            let value = ctx.session_mut().alloc_real_from(&clip)?;
            ctx.session_mut()
                .list_set(result, params.len() + 1, value)?;
        }
        let labels = alloc_strings(
            ctx,
            params
                .iter()
                .map(|(name, _)| *name)
                .chain([".layout", ".clip"]),
        )?;
        ctx.protect(labels);
        let key = ctx.session_mut().intern("names")?;
        ctx.session_mut().set_attrib(result, key, labels)?;
        Ok(result)
    })
}

pub(in crate::graphics) fn restore(ctx: &mut Ctx, call: &Call, input: SexpId) -> EvalResult<()> {
    let labels = names(ctx, input)?;
    if ctx.session().type_of(input)? != SexpType::Vec
        || ctx.session().xlength(input)? != labels.len()
    {
        return Err(ctx.stop(call.err_call(), "invalid recorded graphics state"));
    }
    let slot = active(ctx, call, false)?;
    slot.par_data = None;
    slot.par_state = SEXP_NIL;
    let layout = labels.iter().position(|x| x.as_deref() == Some(".layout"));
    if let Some(i) = layout {
        let expr = ctx.session().list_get(input, i)?;
        if expr != SEXP_NIL {
            ctx.eval(expr, call.env)?;
        }
    }
    for (i, name) in labels.iter().enumerate() {
        let Some(name) = name.as_deref() else {
            continue;
        };
        let value = ctx.session().list_get(input, i)?;
        let stored = state(active(ctx, call, false)?)?
            .params
            .get_key_value(name)
            .map(|(&key, value)| (key, value.kind()));
        if let Some((key, kind)) = stored {
            let param = stored::parse(ctx, call, name, kind, value)?;
            state(active(ctx, call, false)?)?.params.insert(key, param);
        } else {
            match name {
                ".usr" | ".logusr" => {
                    let range = <[f64; 4]>::try_from(nums(ctx, value)?)
                        .map_err(|_| invalid(ctx, call, name))?;
                    let par = state(active(ctx, call, false)?)?;
                    if name == ".usr" {
                        par.usr = range;
                    } else {
                        par.logusr = range;
                    }
                }
                ".clip" => {
                    let clip = if value == SEXP_NIL {
                        None
                    } else {
                        Some(vector::<4>(ctx, call, name, value)?)
                    };
                    state(active(ctx, call, false)?)?.clip = clip;
                }
                ".csi" => {
                    let [csi] = vector::<1>(ctx, call, name, value)?;
                    state(active(ctx, call, false)?)?.csi = csi;
                }
                ".frame" => active(ctx, call, false)?.frame_open = flag(ctx, value)? == Some(true),
                ".layout" => {}
                "new" => {
                    let new = flag(ctx, value)? == Some(true);
                    state(active(ctx, call, false)?)?.new = new;
                }
                _ => set(ctx, call, name, value)?,
            }
        }
    }
    let slot = active(ctx, call, false)?;
    let par = state(slot)?;
    let clip = par.clip.unwrap_or_else(|| par.geometry().plot);
    slot.device.clip(rect(clip));
    Ok(())
}
