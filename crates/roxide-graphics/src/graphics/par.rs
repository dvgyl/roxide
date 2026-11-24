use std::collections::BTreeMap;

use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{NA_REAL, RLogical, SEXP_NIL, SexpType, ValueApi};

use crate::{
    engine::{DeviceSlot, GContext, Rgba},
    grdevices::parse_rgba,
};

use super::{active, alloc_strings, flag, layout::Layout, names, nums, record, string, strings};

#[derive(Clone)]
pub(super) enum Param {
    Real(f64),
    Int(i32),
    Flag(Option<bool>),
    Text(String),
    Color(String, Rgba),
    Reals(Vec<f64>),
    Ints(Vec<i32>),
}

impl Param {
    fn alloc(&self, ctx: &mut Ctx) -> EvalResult {
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
}

const READ_ONLY: &[&str] = &["cin", "cra", "csi", "cxy", "din", "page"];
const UNIT: [f64; 4] = [0.0, 1.0, 0.0, 1.0];

#[derive(Clone, Copy)]
enum Margins {
    Lines([f64; 4]),
    Inches([f64; 4]),
}

impl Margins {
    fn inches(self, line: f64) -> [f64; 4] {
        match self {
            Self::Lines(x) => x.map(|x| x * line),
            Self::Inches(x) => x,
        }
    }

    fn lines(self, line: f64) -> [f64; 4] {
        match self {
            Self::Lines(x) => x,
            Self::Inches(x) => x.map(|x| x / line),
        }
    }
}

#[derive(Clone, Copy)]
enum Region {
    Normalized([f64; 4]),
    Size([f64; 2]),
}

enum Panel {
    Grid {
        rows: usize,
        cols: usize,
        by_row: bool,
    },
    Layout(Layout),
    Custom(Region),
}

impl Panel {
    fn dims(&self) -> [usize; 2] {
        match self {
            Self::Grid { rows, cols, .. } => [*rows, *cols],
            Self::Layout(layout) => [layout.rows, layout.cols],
            Self::Custom(_) => [1, 1],
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::Layout(layout) => layout.len(),
            _ => self.dims().iter().product(),
        }
    }

    fn cell(&self, index: usize) -> [usize; 2] {
        let [rows, cols] = self.dims();
        match self {
            Self::Grid { by_row: false, .. } => [index % rows + 1, index / rows + 1],
            Self::Layout(layout) => layout.cell(index),
            _ => [index / cols + 1, index % cols + 1],
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Geometry {
    pub(super) device: [f64; 4],
    pub(super) inner: [f64; 4],
    pub(super) figure: [f64; 4],
    pub(super) plot: [f64; 4],
    fig: [f64; 4],
    plt: [f64; 4],
    omd: [f64; 4],
    omi: [f64; 4],
}

pub(super) fn size(r: [f64; 4]) -> [f64; 2] {
    [r[1] - r[0], r[3] - r[2]]
}

fn centered([w, h]: [f64; 2], [pw, ph]: [f64; 2]) -> [f64; 4] {
    let x = (1.0 - w / pw) * 0.5;
    let y = (1.0 - h / ph) * 0.5;
    [x, 1.0 - x, y, 1.0 - y]
}

fn within([l, r, b, t]: [f64; 4], region: [f64; 4]) -> [f64; 4] {
    let [w, h] = size(region);
    [
        region[0] + l * w,
        region[0] + r * w,
        region[2] + b * h,
        region[2] + t * h,
    ]
}

pub(super) struct GPar {
    params: BTreeMap<&'static str, Param>,
    din: [f64; 2],
    cin: [f64; 2],
    csi: f64,
    mar: Margins,
    oma: Margins,
    omd: Option<[f64; 4]>,
    plot: Option<Region>,
    panel: Panel,
    index: usize,
    usr: [f64; 4],
    logusr: [f64; 4],
    pub(super) log: [bool; 2],
    pub(super) axp: [[f64; 3]; 2],
    pub(super) new: bool,
    pub(super) clip: Option<[f64; 4]>,
}

impl GPar {
    fn new(din: [f64; 2], ps: f64, bg: Rgba) -> Self {
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
        Self {
            params,
            din,
            cin: [ps / 80.0, ps / 60.0],
            csi: ps / 60.0,
            mar: Margins::Lines([5.1, 4.1, 4.1, 2.1]),
            oma: Margins::Lines([0.0; 4]),
            omd: None,
            panel: Panel::Grid {
                rows: 1,
                cols: 1,
                by_row: true,
            },
            index: 0,
            plot: None,
            usr: UNIT,
            logusr: [0.0; 4],
            log: [false; 2],
            axp: [[0.0, 1.0, 5.0]; 2],
            new: false,
            clip: None,
        }
    }

    pub(super) fn usr(&self) -> [f64; 4] {
        std::array::from_fn(|i| {
            if self.log[i / 2] {
                self.logusr[i]
            } else {
                self.usr[i]
            }
        })
    }

    pub(super) fn set_usr(&mut self, axis: usize, range: [f64; 2]) {
        let indices = 2 * axis..2 * axis + 2;
        if self.log[axis] {
            self.logusr[indices.clone()].copy_from_slice(&range);
            self.usr[indices].copy_from_slice(&range.map(|x| 10.0_f64.powf(x)));
        } else {
            self.usr[indices].copy_from_slice(&range);
        }
    }

    pub(super) fn value(&self, name: &str) -> Option<&Param> {
        self.params.get(name)
    }

    pub(super) fn real(&self, name: &str) -> f64 {
        match self.value(name) {
            Some(Param::Real(x)) => *x,
            Some(Param::Int(x)) => f64::from(*x),
            _ => NA_REAL,
        }
    }

    pub(super) fn text(&self, name: &str) -> &str {
        match self.value(name) {
            Some(Param::Text(x)) => x,
            _ => "",
        }
    }

    pub(super) fn color(&self, name: &str) -> Rgba {
        match self.value(name) {
            Some(Param::Color(_, x)) => *x,
            _ => Rgba::TRANSPARENT,
        }
    }

    pub(super) fn char_size(&self, axis: usize) -> f64 {
        self.cin[axis] * self.real("cex")
    }

    pub(super) fn line(&self) -> f64 {
        self.csi * self.real("mex")
    }

    pub(super) fn axis_count(&self, axis: usize) -> f64 {
        match self.value("lab") {
            Some(Param::Ints(x)) => f64::from(x[axis]),
            _ => 5.0,
        }
    }

    pub(super) fn page(&self) -> bool {
        !self.new && self.index + 1 == self.panel.len()
    }

    fn next_index(&self, frame_open: bool) -> usize {
        if self.new && frame_open {
            self.index
        } else {
            (self.index + 1) % self.panel.len()
        }
    }

    pub(super) fn next_geometry(&self, frame_open: bool) -> Geometry {
        let line = self.cin[1] * self.real("cex") * self.real("mex");
        self.geometry_at(self.next_index(frame_open), line)
    }

    pub(super) fn advance(&mut self, frame_open: bool) {
        self.index = self.next_index(frame_open);
        self.refresh();
    }

    fn refresh(&mut self) {
        self.csi = self.cin[1] * self.real("cex");
        self.clip = None;
    }

    pub(super) fn set_layout(&mut self, layout: Layout) {
        self.set_panel(Panel::Layout(layout));
    }

    fn set_panel(&mut self, panel: Panel) {
        let [r, c] = panel.dims();
        let cex = if r.max(c) >= 3 {
            0.66
        } else if r == 2 && c == 2 {
            0.83
        } else {
            1.0
        };
        self.index = panel.len() - 1;
        self.panel = panel;
        self.plot = None;
        self.params.insert("cex", Param::Real(cex));
        self.params.insert("mex", Param::Real(1.0));
        self.refresh();
    }

    pub(super) fn geometry(&self) -> Geometry {
        self.geometry_at(self.index, self.line())
    }

    fn geometry_at(&self, index: usize, line: f64) -> Geometry {
        let [w, h] = self.din;
        let device = [0.0, w, 0.0, h];
        let (omd, omi) = if let Some([l, r, b, t]) = self.omd {
            ([l, r, b, t], [h * b, w * l, h * (1.0 - t), w * (1.0 - r)])
        } else {
            let [b, l, t, r] = self.oma.inches(line);
            ([l / w, 1.0 - r / w, b / h, 1.0 - t / h], [b, l, t, r])
        };
        let inner = within(omd, device);
        let fig = match &self.panel {
            Panel::Custom(Region::Normalized(rect)) => *rect,
            Panel::Custom(Region::Size(wanted)) => centered(*wanted, size(inner)),
            Panel::Layout(layout) => layout.figure(index, size(inner)),
            Panel::Grid { rows, cols, .. } => {
                let [r, c] = self.panel.cell(index);
                [
                    (c - 1) as f64 / *cols as f64,
                    c as f64 / *cols as f64,
                    1.0 - r as f64 / *rows as f64,
                    1.0 - (r - 1) as f64 / *rows as f64,
                ]
            }
        };
        let figure = within(fig, inner);
        let [fw, fh] = size(figure);
        let plt = match self.plot {
            Some(Region::Normalized(rect)) => rect,
            Some(Region::Size(wanted)) => centered(wanted, [fw, fh]),
            None => {
                let [b, l, t, r] = self.mar.inches(line);
                let [mut l, mut r, mut b, mut t] = [l / fw, 1.0 - r / fw, b / fh, 1.0 - t / fh];
                if self.text("pty") == "s" {
                    let [pw, ph] = [(r - l) * fw, (t - b) * fh];
                    let square = pw.min(ph);
                    let dx = (pw - square) / (2.0 * fw);
                    let dy = (ph - square) / (2.0 * fh);
                    l += dx;
                    r -= dx;
                    b += dy;
                    t -= dy;
                }
                [l, r, b, t]
            }
        };
        Geometry {
            device,
            inner,
            figure,
            plot: within(plt, figure),
            fig,
            plt,
            omd,
            omi,
        }
    }

    fn get(&self, name: &str) -> Option<Param> {
        use Param::*;
        let g = self.geometry();
        let flag = |b| Flag(Some(b));
        let [r, c] = self.panel.cell(self.index);
        let [nr, nc] = self.panel.dims();
        Some(match name {
            "cin" => Reals(self.cin.to_vec()),
            "cra" => Reals(self.cin.map(|x| x * 72.0).to_vec()),
            "csi" => Real(self.csi),
            "cxy" => {
                let [pw, ph] = size(g.plot);
                let [ux, uy] = size(self.usr());
                Reals(vec![self.cin[0] * ux / pw, self.cin[1] * uy / ph])
            }
            "din" => Reals(self.din.to_vec()),
            "page" => flag(self.page()),
            "oma" => Reals(
                if self.omd.is_some() {
                    g.omi.map(|x| x / self.line())
                } else {
                    self.oma.lines(self.line())
                }
                .to_vec(),
            ),
            "omi" => Reals(g.omi.to_vec()),
            "omd" => Reals(g.omd.to_vec()),
            "mar" => Reals(self.mar.lines(self.line()).to_vec()),
            "mai" => Reals(self.mar.inches(self.line()).to_vec()),
            "fig" => Reals(g.fig.to_vec()),
            "fin" => Reals(size(g.figure).to_vec()),
            "plt" => Reals(g.plt.to_vec()),
            "pin" => Reals(size(g.plot).to_vec()),
            "mfrow" | "mfcol" => Ints(vec![nr as i32, nc as i32]),
            "mfg" => Ints(vec![r as i32, c as i32, nr as i32, nc as i32]),
            "new" => flag(self.new),
            "usr" => Reals(self.usr().to_vec()),
            "xlog" => flag(self.log[0]),
            "ylog" => flag(self.log[1]),
            "xaxp" => Reals(self.axp[0].to_vec()),
            "yaxp" => Reals(self.axp[1].to_vec()),
            _ => self.value(name)?.clone(),
        })
    }

    fn keys(&self) -> Vec<&'static str> {
        let mut keys = self.params.keys().copied().collect::<Vec<_>>();
        keys.extend_from_slice(READ_ONLY);
        keys.extend([
            "oma", "omi", "omd", "mar", "mai", "fig", "fin", "plt", "pin", "mfg", "mfrow", "mfcol",
            "new", "usr", "xaxp", "yaxp",
        ]);
        keys.sort_unstable();
        keys.splice(0..0, ["xlog", "ylog"]);
        keys
    }
}

pub(super) fn state(slot: &mut DeviceSlot) -> EvalResult<&mut GPar> {
    let data = slot.par_data.get_or_insert_with(|| {
        let (w, h) = slot.device.size_in();
        let gc = GContext::default();
        // The device uses its initial point size when the context supplies zero.
        let reference = slot.device.metric_info('M', &gc).2;
        let nominal = slot.device.metric_info('M', &GContext { ps: 0.0, ..gc }).2;
        let inferred = (12.0 * nominal / reference).round();
        let ps = if inferred.is_finite() && inferred > 0.0 {
            inferred
        } else {
            12.0
        };
        let bg = slot
            .device
            .initial_background()
            .unwrap_or_else(|| match slot.device.name() {
                "svg" | "png" | "quartz_off_screen" => Rgba::WHITE,
                _ => Rgba::TRANSPARENT,
            });
        Box::new(GPar::new([w, h], ps, bg))
    });
    data.downcast_mut::<GPar>().ok_or_else(|| {
        roxide_eval::Unwind::Internal(roxide_eval::InternalError {
            message: "invalid base graphics state".into(),
        })
    })
}

fn invalid(ctx: &mut Ctx, call: &Call, name: &str) -> roxide_eval::Unwind {
    ctx.stop(
        call.err_call(),
        format!("invalid value specified for graphical parameter \"{name}\""),
    )
}

fn vector<const N: usize>(
    ctx: &mut Ctx,
    call: &Call,
    name: &str,
    value: SexpId,
) -> EvalResult<[f64; N]> {
    let values = nums(ctx, value)?;
    let Ok(values) = <[f64; N]>::try_from(values) else {
        return Err(invalid(ctx, call, name));
    };
    if values.iter().any(|x| !x.is_finite()) {
        return Err(invalid(ctx, call, name));
    }
    Ok(values)
}

pub(super) fn parse_color(ctx: &mut Ctx, value: SexpId) -> EvalResult<Rgba> {
    parse_rgba(ctx, value)
}

pub(super) fn choice(ctx: &mut Ctx, x: SexpId, choices: &[&str]) -> EvalResult<Option<String>> {
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

const LTY: &[&str] = &[
    "blank", "solid", "dashed", "dotted", "dotdash", "longdash", "twodash",
];

pub(super) fn line_type(ctx: &mut Ctx, value: SexpId) -> EvalResult<Option<String>> {
    if matches!(
        ctx.session().type_of(value)?,
        SexpType::Int | SexpType::Real
    ) {
        let value = nums(ctx, value)?.first().copied().unwrap_or(NA_REAL);
        return Ok(if value == 0.0 {
            Some("blank".into())
        } else if value.is_finite() && value >= 1.0 && value <= f64::from(i32::MAX) {
            Some(LTY[1 + (value as usize - 1) % (LTY.len() - 1)].into())
        } else {
            None
        });
    }
    if let Some(name) = choice(ctx, value, LTY)? {
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
            include_str!("../grdevices/named_colors.tsv")
                .lines()
                .filter_map(|line| line.split_once('\t'))
                .find_map(|(name, value)| (value == hex).then(|| name.to_owned()))
                .unwrap_or_else(|| format!("#{hex}"))
        }
        _ => format!("#{r:02X}{g:02X}{b:02X}{a:02X}"),
    }
}

fn parse(
    ctx: &mut Ctx,
    call: &Call,
    name: &str,
    template: &Param,
    value: SexpId,
) -> EvalResult<Param> {
    use Param::*;
    let result = match template {
        Color(..) => {
            let rgba = parse_color(ctx, value)?;
            let spelling = string(ctx, value)?;
            let rgba = if spelling.parse::<f64>().is_ok_and(|x| x.trunc() == 0.0) {
                state(active(ctx, call, true)?)?.color("bg")
            } else {
                rgba
            };
            let selected = color_name(rgba);
            Color(selected, rgba)
        }
        Flag(_) => {
            let flag = flag(ctx, value)?;
            if flag.is_none() && name != "xpd" {
                return Err(invalid(ctx, call, name));
            }
            Flag(flag)
        }
        Int(_) if name != "pch" || ctx.session().type_of(value)? != SexpType::Str => {
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
        Text(_) | Int(_) => {
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
        Real(_) => {
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
        Ints(_) => {
            let x = vector::<3>(ctx, call, name, value)?;
            if x.iter().any(|x| *x < 0.0 || *x > i32::MAX as f64) {
                return Err(invalid(ctx, call, name));
            }
            Ints(x.map(|x| x as i32).to_vec())
        }
        Reals(_) => Reals(vector::<3>(ctx, call, name, value)?.to_vec()),
    };
    Ok(result)
}

fn set(ctx: &mut Ctx, call: &Call, name: &str, value: SexpId) -> EvalResult<()> {
    let slot = active(ctx, call, true)?;
    let initialized = slot.frame_open;
    let par = state(slot)?;
    if READ_ONLY.contains(&name) {
        return ctx.warn_now(
            call.err_call(),
            format!("graphical parameter \"{name}\" cannot be set"),
        );
    }
    let old = par.value(name).cloned();
    if let Some(template) = old {
        let template = if name == "pch" {
            Param::Int(1)
        } else {
            template
        };
        let value = parse(ctx, call, name, &template, value)?;
        let par = state(active(ctx, call, true)?)?;
        if name == "fg" {
            par.params.insert("col", value.clone());
        }
        if let Param::Real(x) = value {
            if name == "tck" && (x.is_finite() || par.real("tcl").is_nan()) {
                par.params.insert(
                    "tcl",
                    Param::Real(if x.is_finite() { NA_REAL } else { -0.5 }),
                );
            }
            if name == "tcl" && x.is_finite() {
                par.params.insert("tck", Param::Real(NA_REAL));
            }
        }
        if let Some(entry) = par.params.get_mut(name) {
            *entry = value;
        }
        match name {
            "mex" => par.refresh(),
            "pty" => {
                par.plot = None;
                par.refresh();
            }
            "xpd" => par.clip = None,
            _ => {}
        }
        return Ok(());
    }
    match name {
        "oma" | "omi" | "omd" | "mar" | "mai" | "fig" | "plt" | "usr" => {
            let v = vector::<4>(ctx, call, name, value)?;
            let valid = match name {
                "usr" => true,
                "oma" | "omi" | "mar" | "mai" => v.iter().all(|&x| x >= 0.0),
                _ => {
                    v[0] >= 0.0
                        && v[1] <= 1.0
                        && v[2] >= 0.0
                        && v[3] <= 1.0
                        && v[0] < v[1]
                        && v[2] < v[3]
                }
            };
            if !valid {
                return Err(invalid(ctx, call, name));
            }
            let par = state(active(ctx, call, true)?)?;
            match name {
                "oma" => {
                    par.oma = Margins::Lines(v);
                    par.omd = None;
                }
                "omi" => {
                    par.oma = Margins::Inches(v);
                    par.omd = None;
                }
                "omd" => par.omd = Some(v),
                "mar" => {
                    par.mar = Margins::Lines(v);
                    par.plot = None;
                }
                "mai" => {
                    par.mar = Margins::Inches(v);
                    par.plot = None;
                }
                "fig" => {
                    par.panel = Panel::Custom(Region::Normalized(v));
                    par.index = 0;
                    par.new = false;
                }
                "plt" => par.plot = Some(Region::Normalized(v)),
                _ => {
                    for axis in 0..2 {
                        let [a, b] = [v[2 * axis], v[2 * axis + 1]];
                        par.set_usr(axis, [a, b]);
                        if !par.log[axis] {
                            let logged = [a, b].map(|x| if x > 0.0 { x.log10() } else { NA_REAL });
                            par.logusr[2 * axis..2 * axis + 2].copy_from_slice(&logged);
                        }
                        let limits = if par.log[axis] {
                            [10.0_f64.powf(a), 10.0_f64.powf(b)]
                        } else {
                            [a, b]
                        };
                        par.axp[axis] = super::coords::linear_ticks(limits, par.axis_count(axis));
                    }
                    return Ok(());
                }
            }
            par.refresh();
        }
        "fin" | "pin" | "mfrow" | "mfcol" => {
            let v = vector::<2>(ctx, call, name, value)?;
            if v.iter().any(|&x| x <= 0.0) {
                return Err(invalid(ctx, call, name));
            }
            let par = state(active(ctx, call, true)?)?;
            match name {
                "fin" => {
                    par.panel = Panel::Custom(Region::Size(v));
                    par.index = 0;
                    par.new = false;
                    par.refresh();
                }
                "pin" => {
                    par.plot = Some(Region::Size(v));
                    par.refresh();
                }
                _ => {
                    let [rows, cols] = v.map(|x| x as usize);
                    if rows == 0 || cols == 0 || rows > 200 || cols > 200 {
                        return Err(invalid(ctx, call, name));
                    }
                    par.set_panel(Panel::Grid {
                        rows,
                        cols,
                        by_row: name == "mfrow",
                    });
                }
            }
        }
        "mfg" => {
            let v = nums(ctx, value)?;
            if !matches!(v.len(), 2 | 4) || v.iter().any(|x| !x.is_finite()) {
                return Err(invalid(ctx, call, name));
            }
            let [rows, cols] = state(active(ctx, call, true)?)?.panel.dims();
            let [r, c] = [v[0] as usize, v[1] as usize];
            if r == 0 || r > rows || c == 0 || c > cols {
                return Err(invalid(ctx, call, name));
            }
            for (i, (expected, label)) in [(rows, "nr"), (cols, "nc")].into_iter().enumerate() {
                if v.get(2 + i).is_some_and(|&x| x as usize != expected) {
                    ctx.warn_now(
                        call.err_call(),
                        format!(r#"value of '{label}' in "mfg" is wrong and will be ignored"#),
                    )?;
                }
            }
            let par = state(active(ctx, call, true)?)?;
            par.index = (0..par.panel.len())
                .find(|&i| par.panel.cell(i) == [r, c])
                .unwrap_or(par.index);
            par.new = initialized;
            par.refresh();
        }
        "xaxp" | "yaxp" => {
            let v = vector::<3>(ctx, call, name, value)?;
            if v[2] < 1.0 {
                return Err(invalid(ctx, call, name));
            }
            state(active(ctx, call, true)?)?.axp[usize::from(name == "yaxp")] = v;
        }
        "new" | "xlog" | "ylog" => {
            let Some(v) = flag(ctx, value)? else {
                return Err(invalid(ctx, call, name));
            };
            if name == "new" && v && !initialized {
                ctx.warn_now(call.err_call(), "calling par(new=TRUE) with no plot")?;
            } else {
                let par = state(active(ctx, call, true)?)?;
                if name == "new" {
                    par.new = v;
                } else {
                    let axis = usize::from(name == "ylog");
                    par.log[axis] = v;
                }
            }
        }
        _ => ctx.warn_now(
            call.err_call(),
            format!("\"{name}\" is not a graphical parameter"),
        )?,
    }
    Ok(())
}

pub(super) fn par(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let input = args.get(0);
    let read_only = flag(ctx, args.get(1))? == Some(true);
    let scalar = flag(ctx, args.get(2))? == Some(true);
    let keys = state(active(ctx, call, true)?)?.keys();
    let entries = if input == SEXP_NIL {
        keys.into_iter()
            .filter(|name| !read_only || !READ_ONLY.contains(name))
            .map(|name| (name.to_owned(), None))
            .collect::<Vec<_>>()
    } else {
        let names = names(ctx, input)?;
        (0..ctx.session().xlength(input)?)
            .map(|i| {
                let value = ctx.session().list_get(input, i)?;
                if let Some(Some(name)) = names.get(i)
                    && !name.is_empty()
                {
                    Ok((name.clone(), Some(value)))
                } else {
                    let name = if ctx.session().type_of(value)? == SexpType::Str {
                        strings(ctx, value)?
                            .into_iter()
                            .next()
                            .flatten()
                            .unwrap_or_default()
                    } else {
                        String::new()
                    };
                    Ok((name, None))
                }
            })
            .collect::<EvalResult<Vec<_>>>()?
    };
    ctx.scoped(|ctx| {
        let result = ctx.session_mut().alloc_list(entries.len())?;
        ctx.protect(result);
        for (i, (name, input)) in entries.iter().enumerate() {
            let prev = state(active(ctx, call, true)?)?.get(name);
            if let Some(value) = prev {
                let value = value.alloc(ctx)?;
                if scalar {
                    return Ok(value);
                }
                ctx.session_mut().list_set(result, i, value)?;
            }
            if let Some(value) = input {
                set(ctx, call, name, *value)?;
            } else if state(active(ctx, call, true)?)?.get(name).is_none() {
                ctx.warn_now(
                    call.err_call(),
                    format!("\"{name}\" is not a graphical parameter"),
                )?;
                if scalar {
                    return Ok(SEXP_NIL);
                }
            }
        }
        if entries.iter().any(|(_, value)| value.is_some()) {
            record(ctx, call, "C_rx_par", &[input, args.get(1), args.get(2)])?;
        }
        let names = alloc_strings(ctx, entries.iter().map(|(name, _)| name.as_str()))?;
        ctx.protect(names);
        let key = ctx.session_mut().intern("names")?;
        ctx.session_mut().set_attrib(result, key, names)?;
        Ok(result)
    })
}

pub(super) fn snapshot(ctx: &mut Ctx, call: &Call) -> EvalResult {
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

pub(super) fn restore(ctx: &mut Ctx, call: &Call, input: SexpId) -> EvalResult<()> {
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
        let template = state(active(ctx, call, false)?)?.value(name).cloned();
        if let Some(template) = template {
            let key = state(active(ctx, call, false)?)?
                .params
                .get_key_value(name)
                .map(|(&key, _)| key);
            let template = if name == "pch" {
                Param::Int(1)
            } else {
                template
            };
            let param = parse(ctx, call, name, &template, value)?;
            if let Some(key) = key {
                state(active(ctx, call, false)?)?.params.insert(key, param);
            }
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
    slot.device.clip(super::coords::rect(clip));
    Ok(())
}
