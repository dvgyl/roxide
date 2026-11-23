//! Bridge the grid viewport tree to the active graphics device.

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_value::{SEXP_NIL, ValueApi};

use crate::engine::{GraphicsExt, Rect, Rgba, open_default_device};

fn ensure_device(ctx: &mut Ctx) -> EvalResult<()> {
    if ctx.ext::<GraphicsExt>().current == 1 {
        open_default_device(ctx)?;
    }
    Ok(())
}

pub(super) fn state(ctx: &mut Ctx, call: &Call, _: &mut Args) -> EvalResult {
    ensure_device(ctx)?;
    let Some(slot) = ctx.ext::<GraphicsExt>().active() else {
        return Err(ctx.stop(call.err_call(), "no active graphics device"));
    };
    if slot.grid_state != SEXP_NIL {
        return Ok(slot.grid_state);
    }
    let (w, h) = slot.device.size_in();
    let ps = super::gpar::pointsize(slot.device.as_ref());
    let state = initial_state(ctx, w, h, ps)?;
    let Some(slot) = ctx.ext::<GraphicsExt>().active_mut() else {
        return Err(ctx.stop(call.err_call(), "no active graphics device"));
    };
    slot.grid_state = state;
    Ok(state)
}

fn initial_state(ctx: &mut Ctx, w: f64, h: f64, ps: f64) -> EvalResult {
    use roxide_value::RLogical;
    let mut fields = [SEXP_NIL; 15];
    let midpoint = super::measure::simple(ctx, &[0.5], 0)?;
    let whole = super::measure::simple(ctx, &[1.0], 0)?;
    fields[..4].copy_from_slice(&[midpoint, midpoint, whole, whole]);
    fields[4] = ctx.session_mut().alloc_real_from(&[0.5, 0.5])?;
    let gp = ctx.session_mut().alloc_list(0)?;
    let class = strings(ctx, &["gpar"])?;
    attrib(ctx, gp, "class", class)?;
    fields[5] = gp;
    fields[6] = ctx
        .session_mut()
        .scalar_lgl(RLogical::from_option(Some(true)))?;
    fields[7] = ctx.session_mut().alloc_real_from(&[0.0, w * 72.0])?;
    fields[8] = ctx.session_mut().alloc_real_from(&[0.0, h * 72.0])?;
    fields[9] = ctx.session_mut().scalar_real(0.0)?;
    fields[13] = ctx.session_mut().scalar_str("ROOT")?;
    fields[14] = ctx
        .session_mut()
        .scalar_lgl(RLogical::from_option(Some(false)))?;
    let vp = build_viewport(ctx, &fields)?;
    let node = ctx.session_mut().alloc_env(EMPTY_ENV)?;
    let size = ctx.session_mut().alloc_real_from(&[w, h])?;
    let gp = super::gpar::defaults(ctx, ps)?;
    let transform = Transform([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]).matrix(ctx)?;
    let path = ctx.session_mut().alloc_str_from(&[])?;
    let clip = ctx.session_mut().alloc_real_from(&[0.0, 0.0, w, h])?;
    let children = ctx.session_mut().alloc_list(0)?;
    let display = ctx.session_mut().alloc_list(0)?;
    for (name, value) in [
        ("vp", vp),
        ("size", size),
        ("gp", gp),
        ("transform", transform),
        ("rotation", fields[9]),
        ("parent", SEXP_NIL),
        ("children", children),
        ("path", path),
        ("clip", clip),
        ("tracks", SEXP_NIL),
    ] {
        put(ctx, node, name, value)?;
    }
    let state = ctx.session_mut().alloc_env(EMPTY_ENV)?;
    put(ctx, state, "root", node)?;
    put(ctx, state, "current", node)?;
    put(ctx, state, "display", display)?;
    Ok(state)
}

pub(super) fn select(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let state = args.get(0);
    let node = args.get(1);
    let bounds = get(ctx, node, "clip")?;
    clip(ctx, call, &mut Args::from_pairs(vec![(None, bounds)]))?;
    put(ctx, state, "current", node)?;
    Ok(SEXP_NIL)
}

pub(super) fn page(ctx: &mut Ctx, call: &Call, _: &mut Args) -> EvalResult {
    ensure_device(ctx)?;
    let id = ctx.ext::<GraphicsExt>().current;
    let Some(mut slot) = ctx
        .ext::<GraphicsExt>()
        .slots
        .get_mut(id)
        .and_then(Option::take)
    else {
        return Err(ctx.stop(call.err_call(), "no active graphics device"));
    };
    let result = slot.device.new_page(ctx.host(), Rgba::TRANSPARENT);
    if result.is_ok() {
        slot.display_list.clear();
        slot.page_count += 1;
        slot.frame_open = false;
        slot.grid_state = SEXP_NIL;
    }
    ctx.ext::<GraphicsExt>().slots[id] = Some(slot);
    result.map_err(|err| ctx.stop(call.err_call(), err.to_string()))?;
    Ok(SEXP_NIL)
}

pub(super) fn clip(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let limits = super::numbers(ctx, call, args.get(0))?;
    let [x0, y0, x1, y1] = limits.as_slice() else {
        return Err(ctx.stop(call.err_call(), "invalid clip rectangle"));
    };
    let Some(slot) = ctx.ext::<GraphicsExt>().active_mut() else {
        return Err(ctx.stop(call.err_call(), "no active graphics device"));
    };
    slot.device
        .clip(Rect::normalized(x0 * 72.0, y0 * 72.0, x1 * 72.0, y1 * 72.0));
    Ok(SEXP_NIL)
}

use roxide_gc::SexpId;
use roxide_value::EMPTY_ENV;

use super::{
    fields::*,
    measure::{Space, Units},
};

#[derive(Clone, Copy)]
pub(super) struct Transform(pub(super) [f64; 9]);

impl Transform {
    pub(super) fn read(ctx: &mut Ctx, node: SexpId) -> EvalResult<Self> {
        let x = get(ctx, node, "transform")?;
        let mut values = [0.0; 9];
        for (i, value) in values.iter_mut().enumerate() {
            *value = real(ctx, x, i, 0.0)?;
        }
        Ok(Self(values))
    }

    pub(super) fn point(self, (x, y): (f64, f64)) -> (f64, f64) {
        let m = self.0;
        (x * m[0] + y * m[1] + m[2], x * m[3] + y * m[4] + m[5])
    }

    fn then(self, other: Self) -> Self {
        let mut out = [0.0; 9];
        for (i, v) in out.iter_mut().enumerate() {
            let row = i % 3;
            let col = i / 3 * 3;
            *v = (0..3).map(|j| self.0[row + j * 3] * other.0[col + j]).sum();
        }
        Self(out)
    }

    pub(super) fn bounds(self, x: f64, y: f64, w: f64, h: f64) -> [f64; 4] {
        let points =
            [(x, y), (x + w, y), (x + w, y + h), (x, y + h)].map(|point| self.point(point));
        let (x, y) = points[0];
        points
            .into_iter()
            .fold([x, y, x, y], |[l, b, r, t], (x, y)| {
                [l.min(x), b.min(y), r.max(x), t.max(y)]
            })
    }

    fn matrix(self, ctx: &mut Ctx) -> EvalResult {
        let matrix = ctx.session_mut().alloc_real_from(&self.0)?;
        let dim = ctx.session_mut().alloc_int_from(&[3, 3])?;
        attrib(ctx, matrix, "dim", dim)?;
        Ok(matrix)
    }
}

fn cell_span(ctx: &mut Ctx, tracks: SexpId, pos: SexpId, axis: usize) -> EvalResult<[f64; 3]> {
    let sizes: Vec<f64> = ctx.session().real(tracks)?.iter().collect();
    let first = real(ctx, pos, 0, 1.0)? as usize;
    let last = real(ctx, pos, 1, sizes.len() as f64)? as usize;
    if first == 0 || first > last || last > sizes.len() {
        let axis = if axis == 0 { "col" } else { "row" };
        return Err(ctx.stop(SEXP_NIL, format!("invalid 'layout.pos.{axis}'")));
    }
    let before = if axis == 0 {
        &sizes[..first - 1]
    } else {
        &sizes[last..]
    };
    Ok([
        before.iter().sum(),
        sizes[first - 1..last].iter().sum(),
        sizes.iter().sum(),
    ])
}

fn region(ctx: &mut Ctx, vp: SexpId, parent: Space) -> EvalResult<Option<([f64; 2], [f64; 2])>> {
    let tracks = get(ctx, parent.node, "tracks")?;
    let rows = get(ctx, vp, "valid.pos.row")?;
    let cols = get(ctx, vp, "valid.pos.col")?;
    if tracks == SEXP_NIL || (rows == SEXP_NIL && cols == SEXP_NIL) {
        return Ok(None);
    }
    let mut origin = [0.0; 2];
    let mut size = [0.0; 2];
    let base_vp = get(ctx, parent.node, "vp")?;
    let layout = get(ctx, base_vp, "layout")?;
    let just = get(ctx, layout, "valid.just")?;
    for (axis, (name, pos)) in [("widths", cols), ("heights", rows)]
        .into_iter()
        .enumerate()
    {
        let tracks = get(ctx, tracks, name)?;
        let [offset, span, total] = cell_span(ctx, tracks, pos, axis)?;
        origin[axis] = offset + real(ctx, just, axis, 0.5)? * (parent.size[axis] - total);
        size[axis] = span;
    }
    Ok(Some((origin, size)))
}

pub(super) fn push(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let vp = args.get(0);
    let parent = Space::read(ctx, args.get(1))?;
    let root = args.get(2);
    let angle = num(ctx, vp, "angle", 0, 0.0)?;
    let (sin, cos) = angle.to_radians().sin_cos();
    let (origin, size) = match region(ctx, vp, parent)? {
        Some(region) => region,
        None => {
            let mut size = [0.0; 2];
            let mut anchor = [0.0; 2];
            let mut offset = [0.0; 2];
            let just = get(ctx, vp, "valid.just")?;
            for (axis, (dimension, position)) in
                [("width", "x"), ("height", "y")].into_iter().enumerate()
            {
                let value = get(ctx, vp, dimension)?;
                let item = Units::read(ctx, value)?.at(ctx, 0)?;
                size[axis] = parent.measure(ctx, item, axis, false, 0)?;
                let value = get(ctx, vp, position)?;
                let item = Units::read(ctx, value)?.at(ctx, 0)?;
                anchor[axis] = parent.measure(ctx, item, axis, true, 0)?;
                offset[axis] = real(ctx, just, axis, 0.5)? * size[axis];
            }
            let [x, y] = offset;
            (
                [
                    anchor[0] - (x * cos - y * sin),
                    anchor[1] - (x * sin + y * cos),
                ],
                size,
            )
        }
    };
    if !size.into_iter().chain(origin).all(f64::is_finite) {
        return Err(ctx.stop(
            call.err_call(),
            "non-finite location and/or size for viewport",
        ));
    }
    let matrix = Transform([cos, -sin, origin[0], sin, cos, origin[1], 0.0, 0.0, 1.0])
        .then(Transform::read(ctx, parent.node)?);
    let rotation = num(ctx, parent.node, "rotation", 0, 0.0)? + angle;
    let node = ctx.session_mut().alloc_env(EMPTY_ENV)?;
    ctx.protect(node);
    let gp = get(ctx, vp, "gp")?;
    let gp = super::gpar::inherit(ctx, parent.gp, gp)?;
    put(ctx, node, "gp", gp)?;
    for name in ["col", "fill"] {
        let color = get(ctx, gp, name)?;
        crate::grdevices::parse_rgba(ctx, color)?;
    }
    let parent_path = get(ctx, parent.node, "path")?;
    let mut path = ctx.session().str_slice(parent_path)?.to_vec();
    let name = get(ctx, vp, "name")?;
    path.extend_from_slice(ctx.session().str_slice(name)?);
    let path = ctx.session_mut().alloc_str_from(&path)?;
    let r_size = ctx.session_mut().alloc_real_from(&size)?;
    let r_matrix = matrix.matrix(ctx)?;
    let r_rotation = ctx.session_mut().scalar_real(rotation)?;
    let children = ctx.session_mut().alloc_list(0)?;
    for (name, value) in [
        ("vp", vp),
        ("parent", parent.node),
        ("children", children),
        ("path", path),
        ("size", r_size),
        ("transform", r_matrix),
        ("rotation", r_rotation),
    ] {
        put(ctx, node, name, value)?;
    }
    let clip = num(ctx, vp, "clip", 0, 0.0)?;
    let mut bounds = get(ctx, if clip.is_nan() { root } else { parent.node }, "clip")?;
    if clip == 1.0 {
        if rotation % 90.0 == 0.0 {
            bounds = ctx
                .session_mut()
                .alloc_real_from(&matrix.bounds(0.0, 0.0, size[0], size[1]))?;
        } else {
            let warning = ctx.session_mut().intern("grid.Call.graphics")?;
            let native = ctx.session_mut().intern("C_setviewport")?;
            let vp = ctx.session_mut().intern("vp")?;
            let yes = ctx
                .session_mut()
                .scalar_lgl(roxide_value::RLogical::from_option(Some(true)))?;
            let mut tail = SEXP_NIL;
            for arg in [yes, vp.as_sexp(), native.as_sexp()] {
                tail = ctx.session_mut().cons(arg, tail, SEXP_NIL)?;
            }
            let expr = ctx.session_mut().lcons(warning.as_sexp(), tail, SEXP_NIL)?;
            ctx.warn_now(expr, "cannot clip to rotated viewport")?;
        }
    }
    put(ctx, node, "clip", bounds)?;
    let layout = get(ctx, vp, "layout")?;
    let space = Space::new(ctx, node, size, vp)?;
    let tracks = super::layout::tracks(ctx, layout, space)?;
    put(ctx, node, "tracks", tracks)?;
    let state = args.get(3);
    if state != SEXP_NIL {
        update_child(ctx, parent.node, name, node)?;
        select(
            ctx,
            call,
            &mut Args::from_pairs(vec![(None, state), (None, node)]),
        )?;
    }
    Ok(node)
}

fn position(ctx: &mut Ctx, call: &Call, value: SexpId) -> EvalResult {
    if value == SEXP_NIL {
        return Ok(value);
    }
    let values = super::numbers(ctx, call, value)?;
    if values.is_empty() || values.iter().any(|x| !x.is_finite()) {
        return Err(ctx.stop(call.err_call(), "invalid layout position"));
    }
    let first = values.iter().copied().fold(f64::INFINITY, f64::min);
    let last = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    Ok(ctx
        .session_mut()
        .alloc_int_from(&[first as i32, last as i32])?)
}

fn scale(ctx: &mut Ctx, call: &Call, value: SexpId, name: &str) -> EvalResult {
    let values = super::numbers(ctx, call, value)?;
    match values.as_slice() {
        &[a, b] if a.is_finite() && b.is_finite() && a != b => {
            Ok(ctx.session_mut().alloc_real_from(&[a, b])?)
        }
        _ => Err(ctx.stop(call.err_call(), format!("invalid '{name}' in viewport"))),
    }
}

pub(super) fn choice(
    ctx: &mut Ctx,
    call: &Call,
    input: SexpId,
    options: &[&str],
) -> EvalResult<usize> {
    let value = text(ctx, input, 0)?;
    if ctx.session().xlength(input)? == 1
        && let Some(name) = value
    {
        if let Some(hit) = options.iter().position(|&option| option == name) {
            return Ok(hit);
        }
        let mut hits = options
            .iter()
            .enumerate()
            .filter(|(_, option)| option.starts_with(&name));
        if let Some((i, _)) = hits.next()
            && hits.next().is_none()
        {
            return Ok(i);
        }
    }
    Err(ctx.stop(
        call.err_call(),
        format!("'arg' should be one of {}", options.join(", ")),
    ))
}

fn build_viewport(ctx: &mut Ctx, values: &[SexpId; 15]) -> EvalResult {
    let [
        x,
        y,
        w,
        h,
        just,
        gp,
        clip,
        xscale,
        yscale,
        angle,
        layout,
        row,
        col,
        name,
        mask,
    ] = *values;
    let nil = SEXP_NIL;
    let entries = [
        ("x", x),
        ("y", y),
        ("width", w),
        ("height", h),
        ("justification", just),
        ("gp", gp),
        ("clip", clip),
        ("xscale", xscale),
        ("yscale", yscale),
        ("angle", angle),
        ("layout", layout),
        ("layout.pos.row", row),
        ("layout.pos.col", col),
        ("valid.just", just),
        ("valid.pos.row", row),
        ("valid.pos.col", col),
        ("name", name),
        ("parentgpar", nil),
        ("gpar", nil),
        ("trans", nil),
        ("widths", nil),
        ("heights", nil),
        ("width.cm", nil),
        ("height.cm", nil),
        ("rotation", nil),
        ("cliprect", nil),
        ("parent", nil),
        ("children", nil),
        ("devwidth", nil),
        ("devheight", nil),
        ("clippath", nil),
        ("mask", mask),
        ("resolvedmask", nil),
    ];
    let vp = named(ctx, &entries)?;
    let class = strings(ctx, &["viewport"])?;
    attrib(ctx, vp, "class", class)?;
    Ok(vp)
}

pub(super) fn create(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    use roxide_value::{RLogical, SexpType};
    let mut values = [SEXP_NIL; 15];
    for (i, output) in values[..4].iter_mut().enumerate() {
        let input = args.get(i);
        let unit = if inherits(ctx, input, "unit")? {
            input
        } else {
            super::measure::create(
                ctx,
                call,
                &mut Args::from_pairs(vec![(None, input), (None, args.get(4)), (None, SEXP_NIL)]),
            )?
        };
        if ctx.session().xlength(unit)? != 1 {
            return Err(ctx.stop(
                call.err_call(),
                "'x', 'y', 'width', and 'height' must all be units of length 1",
            ));
        }
        *output = unit;
        ctx.protect(unit);
    }
    values[5] = args.get(6);
    if !inherits(ctx, values[5], "gpar")? {
        return Err(ctx.stop(call.err_call(), "invalid 'gp' value"));
    }
    values[10] = args.get(11);
    if values[10] != SEXP_NIL && !inherits(ctx, values[10], "layout")? {
        return Err(ctx.stop(call.err_call(), "invalid 'layout' value"));
    }
    values[7] = scale(ctx, call, args.get(8), "xscale")?;
    values[8] = scale(ctx, call, args.get(9), "yscale")?;
    ctx.protect(values[7]);
    ctx.protect(values[8]);
    let angle = real(ctx, args.get(10), 0, f64::NAN)?;
    if ctx.session().xlength(args.get(10))? != 1 || !angle.is_finite() {
        return Err(ctx.stop(call.err_call(), "invalid 'angle' in viewport"));
    }
    values[9] = ctx.session_mut().scalar_real(angle)?;
    ctx.protect(values[9]);
    let just = args.get(5);
    let just = if ctx.session().type_of(just)? == SexpType::Str
        && ctx.session().xlength(just)? == 1
        && matches!(text(ctx, just, 0)?.as_deref(), Some("centre" | "center"))
    {
        [0.5, 0.5]
    } else {
        let ns = ctx.ns_lookup("grid").unwrap_or(call.env);
        let just = ctx.call_function(".grid_just", &[(None, just)], ns)?;
        [real(ctx, just, 0, 0.5)?, real(ctx, just, 1, 0.5)?]
    };
    let h = real(ctx, args.get(16), 0, just[0])?;
    let v = real(ctx, args.get(17), 0, just[1])?;
    values[4] = ctx.session_mut().alloc_real_from(&[h, v])?;
    values[11] = position(ctx, call, args.get(12))?;
    values[12] = position(ctx, call, args.get(13))?;
    let clip = args.get(7);
    values[6] = if ctx.session().type_of(clip)? == SexpType::Str {
        let value =
            [Some(false), Some(true), None][choice(ctx, call, clip, &["inherit", "on", "off"])?];
        ctx.session_mut().scalar_lgl(RLogical::from_option(value))?
    } else {
        if ctx.session().type_of(clip)? != SexpType::Lgl || ctx.session().xlength(clip)? != 1 {
            return Err(ctx.stop(call.err_call(), "invalid 'clip' value"));
        }
        clip
    };
    let mask = args.get(15);
    values[14] = if ctx.session().type_of(mask)? == SexpType::Str {
        let inherit = choice(ctx, call, mask, &["inherit", "none"])? == 0;
        ctx.session_mut()
            .scalar_lgl(RLogical::from_option(Some(inherit)))?
    } else {
        mask
    };
    let counter = args.get(18);
    let name = args.get(14);
    values[13] = if name == SEXP_NIL {
        let n = num(ctx, counter, "viewport", 0, 0.0)? as i32;
        let n = n.saturating_add(1);
        let next = ctx.session_mut().scalar_int(n)?;
        put(ctx, counter, "viewport", next)?;
        ctx.session_mut().scalar_str(&format!("GRID.VP.{n}"))?
    } else {
        if ctx.session().type_of(name)? != SexpType::Str
            || ctx.session().xlength(name)? != 1
            || text(ctx, name, 0)?.is_none_or(|name| name.is_empty())
        {
            return Err(ctx.stop(call.err_call(), "invalid viewport name"));
        }
        name
    };
    build_viewport(ctx, &values)
}

fn update_child(ctx: &mut Ctx, parent: SexpId, name: SexpId, child: SexpId) -> EvalResult<()> {
    let children = get(ctx, parent, "children")?;
    let mut values = ctx.session().list_slice(children)?.to_vec();
    let old_names = attr(ctx, children, "names")?;
    let mut names = if old_names == SEXP_NIL {
        Vec::new()
    } else {
        ctx.session().str_slice(old_names)?.to_vec()
    };
    let Some(&id) = ctx.session().str_slice(name)?.first() else {
        return Err(ctx.stop(SEXP_NIL, "invalid viewport name"));
    };
    if let Some(i) = names.iter().position(|&key| key == id) {
        if child == SEXP_NIL {
            names.remove(i);
            values.remove(i);
        } else {
            values[i] = child;
        }
    } else if child != SEXP_NIL {
        names.push(id);
        values.push(child);
    }
    let result = list(ctx, &values)?;
    let names = ctx.session_mut().alloc_str_from(&names)?;
    attrib(ctx, result, "names", names)?;
    put(ctx, parent, "children", result)
}

pub(super) fn record(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let state = args.get(0);
    let original = get(ctx, state, "display")?;
    let mut display = ctx.session().list_slice(original)?.to_vec();
    let node = get(ctx, state, "current")?;
    let entry = named(
        ctx,
        &[
            ("type", args.get(1)),
            ("value", args.get(2)),
            ("count", args.get(3)),
            ("node", node),
        ],
    )?;
    display.push(entry);
    let display = list(ctx, &display)?;
    put(ctx, state, "display", display)?;
    Ok(SEXP_NIL)
}

pub(super) fn up(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let state = state(ctx, call, &mut Args::from_pairs(Vec::new()))?;
    let mut node = get(ctx, state, "current")?;
    let path = get(ctx, node, "path")?;
    let depth = ctx.session().xlength(path)?;
    let n = real(ctx, args.get(0), 0, f64::NAN)?.trunc();
    if ctx.session().xlength(args.get(0))? != 1 || !n.is_finite() || n < 0.0 || n > depth as f64 {
        return Err(ctx.stop(
            call.err_call(),
            "cannot pop the top-level viewport ('grid' and 'graphics' output mixed?)",
        ));
    }
    let n = if n == 0.0 { depth } else { n as usize };
    if n == 0 {
        return Ok(SEXP_NIL);
    }
    let remove = real(ctx, args.get(1), 0, 0.0)? == 1.0;
    let recording = real(ctx, args.get(2), 0, 0.0)? == 1.0;
    let mut child = node;
    for _ in 0..n {
        child = node;
        node = get(ctx, node, "parent")?;
    }
    if remove {
        let vp = get(ctx, child, "vp")?;
        let name = get(ctx, vp, "name")?;
        update_child(ctx, node, name, SEXP_NIL)?;
    }
    select(
        ctx,
        call,
        &mut Args::from_pairs(vec![(None, state), (None, node)]),
    )?;
    let count = ctx.session_mut().scalar_int(n as i32)?;
    if recording {
        let event = ctx
            .session_mut()
            .scalar_str(if remove { "pop" } else { "up" })?;
        record(
            ctx,
            call,
            &mut Args::from_pairs(vec![
                (None, state),
                (None, event),
                (None, SEXP_NIL),
                (None, count),
            ]),
        )?;
    }
    if remove {
        return Ok(SEXP_NIL);
    }
    let parts = &ctx.session().str_slice(path)?[depth - n..];
    let parts: Vec<_> = parts
        .iter()
        .map(|&id| Ok(String::from_utf8_lossy(ctx.session().char_bytes(id)?).into_owned()))
        .collect::<EvalResult<_>>()?;
    let parent = if n == 1 {
        SEXP_NIL
    } else {
        ctx.session_mut().scalar_str(&parts[..n - 1].join("::"))?
    };
    let name = ctx.session_mut().scalar_str(&parts[n - 1])?;
    let result = named(ctx, &[("path", parent), ("name", name), ("n", count)])?;
    let class = strings(ctx, &["vpPath", "path"])?;
    attrib(ctx, result, "class", class)?;
    Ok(result)
}
