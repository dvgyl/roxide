use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{
    CharId, NA_REAL, RComplex, RLogical, RSession, SEXP_NIL, SexpType, ValueApi, is_na_real,
};

use crate::ops::{
    attributes::{has_class, row_count},
    common::{
        attr, ch, chars, concatenate, error, flag, gather, list, names, number, rank, set, string,
        text, vector_limit,
    },
};

use super::{axes, dimensions, numeric, real, set_axis_keys, shape, size};

pub(super) fn transpose(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = matrix_input(ctx, call, args.values()[0])?;
    let s = ctx.session();
    let (r, c, labels) = if let Some((r, c)) = dimensions(s, x) {
        let [row, col] = axes(s, x);
        (r, c, [col, row])
    } else if attr(s, x, "dim") == SEXP_NIL
        && matches!(
            s.type_of(x)?,
            SexpType::Lgl
                | SexpType::Int
                | SexpType::Real
                | SexpType::Cplx
                | SexpType::Str
                | SexpType::Raw
                | SexpType::Vec
                | SexpType::Expr
        )
    {
        (s.xlength(x)?, 1, [SEXP_NIL, attr(s, x, "names")])
    } else {
        return Err(error(ctx, call, "argument is not a matrix"));
    };
    let n = size(ctx, call, r, c, size_of::<Option<usize>>())?;
    let index: Vec<_> = (0..n).map(|i| Some((i % c) * r + i / c)).collect();
    let s = ctx.session_mut();
    let out = gather(s, x, &index)?;
    shape(s, out, c, r, labels)?;
    let original = attr(s, x, "dimnames");
    let old_names = names(s, original);
    if let [a, b] = old_names {
        let names = [*b, *a];
        let dn = attr(s, out, "dimnames");
        if dn != SEXP_NIL {
            crate::ops::common::set_names(s, dn, &names)?;
        }
    }
    Ok(out)
}

fn extent(ctx: &mut Ctx, call: &Call, value: SexpId) -> EvalResult<usize> {
    let n = number(ctx.session_mut(), value)?;
    if !n.is_finite() || n <= -1.0 || n > i32::MAX as f64 {
        return Err(error(
            ctx,
            call,
            if n.is_nan() {
                "invalid 'nrow' value (too large or NA)"
            } else {
                "invalid matrix extent"
            },
        ));
    }
    Ok(n as usize)
}

pub(super) fn diag(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, nr, nc, named] = args.values() else {
        return Err(error(ctx, call, "invalid diagonal arguments"));
    };
    let s = ctx.session();
    if let Some((r, c)) = dimensions(s, *x) {
        if *nr != SEXP_NIL || *nc != SEXP_NIL {
            return Err(error(
                ctx,
                call,
                "'nrow' or 'ncol' cannot be specified when 'x' is a matrix",
            ));
        }
        let n = r.min(c);
        let [a, b] = axes(s, *x);
        let named = flag(ctx, *named, true)?;
        vector_limit(ctx, call, n, size_of::<Option<usize>>())?;
        let index: Vec<_> = (0..n).map(|i| Some(i * (r + 1))).collect();
        let s = ctx.session_mut();
        let out = gather(s, *x, &index)?;
        let rows = chars(s, a);
        let cols = chars(s, b);
        if named && n != 0 && rows.get(..n) == cols.get(..n) && rows.len() >= n {
            let keys = rows[..n].to_vec();
            crate::ops::common::set_names(s, out, &keys)?;
        }
        return Ok(out);
    }
    let len = s.xlength(*x)?;
    let repeat_one = len == 1 && *nr == SEXP_NIL && *nc == SEXP_NIL;
    let r = if repeat_one {
        extent(ctx, call, *x)?
    } else if *nr != SEXP_NIL {
        extent(ctx, call, *nr)?
    } else {
        len
    };
    let c = if *nc == SEXP_NIL {
        r
    } else {
        extent(ctx, call, *nc)?
    };
    let n = size(ctx, call, r, c, size_of::<f64>().max(size_of::<RComplex>()))?;
    if len == 0 && n != 0 {
        return Err(error(ctx, call, "'x' must have positive length"));
    }
    vector_limit(ctx, call, len, size_of::<RComplex>())?;
    let ty = ctx.session().type_of(*x)?;
    let s = ctx.session_mut();
    macro_rules! build {
        ($access:ident,$alloc:ident,$zero:expr,$missing:expr) => {{
            let values: Vec<_> = s.$access(*x)?.iter().collect();
            let mut data = vec![$zero; n];
            for i in 0..r.min(c) {
                data[i * (r + 1)] = if len == 0 { $missing } else { values[i % len] };
            }
            s.$alloc(&data)?
        }};
    }
    let out = if repeat_one {
        let mut data = vec![0.0; n];
        for i in 0..r.min(c) {
            data[i * (r + 1)] = 1.0;
        }
        s.alloc_real_from(&data)?
    } else {
        match ty {
            SexpType::Lgl => build!(lgl, alloc_lgl_from, RLogical::FALSE, RLogical::NA),
            SexpType::Int => build!(int, alloc_int_from, 0, roxide_value::NA_INTEGER),
            SexpType::Raw => build!(raw, alloc_raw_from, 0, 0),
            SexpType::Cplx => build!(
                cplx,
                alloc_cplx_from,
                RComplex { re: 0.0, im: 0.0 },
                RComplex::NA
            ),
            _ => {
                let (x, warn) = s.coerce(*x, SexpType::Real)?;
                let mut data = vec![0.0; n];
                let values = s.real(x)?;
                for i in 0..r.min(c) {
                    data[i * (r + 1)] = if len == 0 {
                        NA_REAL
                    } else {
                        values.get(i % len)
                    };
                }
                if warn.na_introduced {
                    ctx.warn_now(call.err_call(), "NAs introduced by coercion")?;
                }
                ctx.session_mut().alloc_real_from(&data)?
            }
        }
    };
    shape(ctx.session_mut(), out, r, c, [SEXP_NIL; 2])
}

pub(super) fn diag_set(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, value] = args.values() else {
        return Err(error(ctx, call, "invalid diagonal arguments"));
    };
    let Some((r, c)) = dimensions(ctx.session(), *x) else {
        return Err(error(ctx, call, "only matrix diagonals can be replaced"));
    };
    let n = r.min(c);
    let m = ctx.session().xlength(*value)?;
    if m != 1 && m != n {
        return Err(error(ctx, call, "replacement diagonal has wrong length"));
    }
    vector_limit(ctx, call, n, size_of::<f64>())?;
    let indices: Vec<_> = (0..n).map(|i| (i * (r + 1) + 1) as f64).collect();
    let ix = ctx.session_mut().alloc_real_from(&indices)?;
    ctx.call_function("[<-", &[(None, *x), (None, ix), (None, *value)], call.env)
}

pub(super) fn triangle(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, diagonal, upper] = args.values() else {
        return Err(error(ctx, call, "invalid triangle arguments"));
    };
    let x = matrix_input(ctx, call, *x)?;
    let s = ctx.session();
    let (r, c) = dimensions(s, x).unwrap_or((s.xlength(x)?, 1));
    let diagonal = flag(ctx, *diagonal, false)?;
    let upper = flag(ctx, *upper, false)?;
    let n = size(ctx, call, r, c, size_of::<RLogical>())?;
    let data: Vec<_> = (0..n)
        .map(|i| {
            let (row, col) = (i % r, i / r);
            RLogical::from_option(Some(
                (diagonal && row == col) || if upper { row < col } else { row > col },
            ))
        })
        .collect();
    let s = ctx.session_mut();
    let out = s.alloc_lgl_from(&data)?;
    shape(s, out, r, c, [SEXP_NIL; 2])
}

fn matrix_input(ctx: &mut Ctx, call: &Call, x: SexpId) -> EvalResult {
    let s = ctx.session();
    if !has_class(s, x, "data.frame") {
        return Ok(x);
    }
    let cols = s.list_slice(x)?.to_vec();
    let nc = cols.len();
    let cn = attr(s, x, "names");
    let rn = attr(s, x, "row.names");
    let ty = cols
        .iter()
        .filter_map(|&v| s.type_of(v).ok())
        .max_by_key(|&ty| rank(ty))
        .unwrap_or(SexpType::Lgl);
    let nr = row_count(ctx.session_mut(), x)?;
    size(ctx, call, nr, nc, size_of::<RComplex>())?;
    let s = ctx.session_mut();
    let out = concatenate(s, &cols, ty)?;
    let rn = if s.type_of(rn)? == SexpType::Str {
        rn
    } else {
        SEXP_NIL
    };
    shape(s, out, nr, nc, [rn, cn])
}

fn dims(ctx: &mut Ctx, call: &Call, x: SexpId) -> EvalResult<Vec<usize>> {
    let s = ctx.session();
    let d = attr(s, x, "dim");
    if d == SEXP_NIL {
        return Ok(vec![s.xlength(x)?]);
    }
    let shape: Vec<_> = s
        .int(d)?
        .iter()
        .filter_map(|x| usize::try_from(x).ok())
        .collect();
    if shape.len() != s.xlength(d)?
        || shape.iter().copied().fold(1usize, usize::saturating_mul) != s.xlength(x)?
    {
        return Err(error(ctx, call, "invalid array dimensions"));
    }
    Ok(shape)
}

fn axis_names(s: &RSession, x: SexpId, count: usize) -> Vec<SexpId> {
    let dn = attr(s, x, "dimnames");
    if dn == SEXP_NIL && attr(s, x, "dim") == SEXP_NIL {
        return vec![attr(s, x, "names")];
    }
    (0..count)
        .map(|i| s.list_get(dn, i).unwrap_or(SEXP_NIL))
        .collect()
}

fn array_keys(s: &RSession, x: SexpId, count: usize) -> Vec<CharId> {
    let mut keys = names(s, attr(s, x, "dimnames")).to_vec();
    keys.resize(count, CharId::BLANK);
    keys
}

fn array_shape(s: &mut RSession, out: SexpId, dims: &[usize], labels: &[SexpId]) -> EvalResult {
    let d = s.alloc_int_from(&dims.iter().map(|&d| d as i32).collect::<Vec<_>>())?;
    set(s, out, "dim", d)?;
    if labels.iter().any(|&v| v != SEXP_NIL) {
        let dn = list(s, labels)?;
        set(s, out, "dimnames", dn)?;
    }
    Ok(out)
}

#[derive(Default)]
struct Sum {
    total: f64,
    count: usize,
}

impl Sum {
    fn add(&mut self, value: f64, rows: bool) {
        if is_na_real(value) || (rows && value.is_nan()) {
            self.total = value;
        } else if !self.total.is_nan() {
            self.total += value;
        }
        self.count += 1;
    }

    fn finish(self, mean: bool) -> f64 {
        if mean {
            self.total / self.count as f64
        } else {
            self.total
        }
    }
}

pub(super) fn summary(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, rm, k, op] = args.values() else {
        return Err(error(ctx, call, "invalid matrix summary arguments"));
    };
    let x = matrix_input(ctx, call, *x)?;
    let s = ctx.session();
    let ty = s.type_of(x)?;
    if !numeric(ty) {
        return Err(error(ctx, call, "'x' must be numeric"));
    }
    let dim = dims(ctx, call, x)?;
    let axes = axis_names(ctx.session(), x, dim.len());
    let keys = array_keys(ctx.session(), x, dim.len());
    if dim.len() < 2 {
        return Err(error(
            ctx,
            call,
            "'x' must be an array of at least two dimensions",
        ));
    }
    let kind = number(ctx.session_mut(), *op)? as u8;
    let rows = kind.is_multiple_of(2);
    let mean = kind >= 2;
    let (rm, _) = ctx.session_mut().coerce(*rm, SexpType::Lgl)?;
    let rm = ctx.session().lgl(rm)?.iter().next().and_then(RLogical::get);
    let rm = rm.ok_or_else(|| error(ctx, call, "invalid 'na.rm' argument"))?;
    let k = number(ctx.session_mut(), *k)?;
    if !k.is_finite() || k < 1.0 || k >= dim.len() as f64 {
        return Err(error(ctx, call, "invalid 'dims'"));
    }
    let k = k as usize;
    let (nr, nc) = (
        dim[..k].iter().copied().fold(1usize, usize::saturating_mul),
        dim[k..].iter().copied().fold(1usize, usize::saturating_mul),
    );
    let m = if rows { nr } else { nc };
    vector_limit(
        ctx,
        call,
        m,
        size_of::<Sum>() * if ty == SexpType::Cplx { 2 } else { 1 },
    )?;
    let index = |i| if rows { i % nr } else { i / nr };
    let mut sums: Vec<_> = (0..m).map(|_| Sum::default()).collect();
    let out = if ty == SexpType::Cplx {
        let mut imag: Vec<_> = (0..m).map(|_| Sum::default()).collect();
        for (i, z) in ctx.session().cplx(x)?.iter().enumerate() {
            if !rm || !(z.re.is_nan() || z.im.is_nan()) {
                sums[index(i)].add(z.re, rows);
                imag[index(i)].add(z.im, rows);
            }
        }
        let data: Vec<_> = sums
            .into_iter()
            .zip(imag)
            .map(|(a, b)| RComplex {
                re: a.finish(mean),
                im: b.finish(mean),
            })
            .collect();
        ctx.session_mut().alloc_cplx_from(&data)?
    } else {
        for (i, v) in real(ctx, call, x)?.into_iter().enumerate() {
            if !rm || !v.is_nan() {
                sums[index(i)].add(v, rows);
            }
        }
        let data: Vec<_> = sums.into_iter().map(|sum| sum.finish(mean)).collect();
        ctx.session_mut().alloc_real_from(&data)?
    };
    let (dim, axes, keys) = if rows {
        (&dim[..k], &axes[..k], &keys[..k])
    } else {
        (&dim[k..], &axes[k..], &keys[k..])
    };
    let s = ctx.session_mut();
    if let [n] = axes {
        set(s, out, "names", *n)?;
        Ok(out)
    } else {
        array_shape(s, out, dim, axes)?;
        set_axis_keys(s, out, keys)
    }
}

pub(super) fn outer(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, y, fun, dots, kind] = args.values() else {
        return Err(error(ctx, call, "invalid outer product arguments"));
    };
    let kind = number(ctx.session_mut(), *kind)? as u8;
    let s = ctx.session();
    let (nx, ny) = (s.xlength(*x)?, s.xlength(*y)?);
    let (mut dx, mut dy) = (dims(ctx, call, *x)?, dims(ctx, call, *y)?);
    let s = ctx.session();
    let (mut names_x, mut names_y) = (axis_names(s, *x, dx.len()), axis_names(s, *y, dy.len()));
    let n = nx
        .checked_mul(ny)
        .ok_or_else(|| error(ctx, call, "vector size limit exceeded"))?;
    vector_limit(ctx, call, n, size_of::<Option<usize>>())?;
    let multiply = string(ctx.session(), *fun).as_deref() == Some("*");
    let out = if multiply {
        if ctx.session().xlength(*dots)? != 0 {
            return Err(error(ctx, call, "using ... with FUN = \"*\" is an error"));
        }
        if !numeric(ctx.session().type_of(*x)?) || !numeric(ctx.session().type_of(*y)?) {
            return Err(error(
                ctx,
                call,
                "requires numeric/complex matrix/vector arguments",
            ));
        }
        vector_limit(ctx, call, nx.max(ny), size_of::<Option<usize>>())?;
        let x = plain_vector(ctx, *x, nx)?;
        ctx.protect(x);
        let y = plain_vector(ctx, *y, ny)?;
        let value = ctx.call_function("tcrossprod", &[(None, x), (None, y)], call.env)?;
        plain_vector(ctx, value, n)?
    } else {
        let s = ctx.session_mut();
        let i: Vec<_> = (0..n).map(|i| Some(i % nx)).collect();
        let a = gather(s, *x, &i)?;
        let i: Vec<_> = (0..n).map(|i| Some(i / nx)).collect();
        let b = gather(s, *y, &i)?;
        let extra = s.list_slice(*dots)?.to_vec();
        let names = names(s, *dots).to_vec();
        let mut argv = vec![(None, a), (None, b)];
        for (i, v) in extra.into_iter().enumerate() {
            let name = names.get(i).copied().unwrap_or(CharId::BLANK);
            let tag = if name == CharId::BLANK {
                None
            } else {
                Some(s.intern(&text(s, name))?)
            };
            argv.push((tag, v));
        }
        ctx.protect(a);
        ctx.protect(b);
        let fun = if let Some(name) = string(ctx.session(), *fun) {
            let sym = ctx.session_mut().intern(&name)?;
            ctx.find_fun(sym, call.env)?
        } else {
            *fun
        };
        let value = ctx.apply(fun, &argv, call.err_call(), call.env)?;
        let len = ctx.session().xlength(value)?;
        if len != n {
            return Err(error(
                ctx,
                call,
                format!("dims [product {n}] do not match the length of object [{len}]"),
            ));
        }
        plain_vector(ctx, value, n)?
    };
    if kind == 0 {
        let s = ctx.session_mut();
        let mut keys = array_keys(s, *x, dx.len());
        keys.extend(array_keys(s, *y, dy.len()));
        dx.extend(dy);
        names_x.extend(names_y);
        array_shape(s, out, &dx, &names_x)?;
        return set_axis_keys(s, out, &keys);
    }
    let axes = dx.len().max(dy.len());
    dx.resize(axes, 1);
    dy.resize(axes, 1);
    names_x.resize(axes, SEXP_NIL);
    names_y.resize(axes, SEXP_NIL);
    let dest: Vec<_> = dx
        .iter()
        .zip(&dy)
        .map(|(&a, &b)| a.saturating_mul(b))
        .collect();
    if dest.iter().any(|&d| d > i32::MAX as usize) {
        return Err(error(ctx, call, "invalid matrix extent"));
    }
    let order: Vec<_> = (0..n)
        .map(|mut i| {
            let (mut a, mut b, mut ax, mut bx) = (0, 0, 1, 1);
            for ((&d, &r), &c) in dest.iter().zip(&dx).zip(&dy) {
                let p = i % d;
                i /= d;
                a += (p / c) * ax;
                b += (p % c) * bx;
                ax *= r;
                bx *= c;
            }
            Some(a + b * nx)
        })
        .collect();
    let s = ctx.session_mut();
    let value = gather(s, out, &order)?;
    let mut labels = vec![SEXP_NIL; axes];
    if kind == 2 {
        for (axis, (a, b)) in names_x.into_iter().zip(names_y).enumerate() {
            if a == SEXP_NIL && b == SEXP_NIL {
                continue;
            }
            let n = dest[axis];
            vector_limit(ctx, call, n, size_of::<CharId>())?;
            let s = ctx.session_mut();
            let a = chars(s, a).to_vec();
            let b = chars(s, b).to_vec();
            let mut names = Vec::with_capacity(n);
            for i in 0..n {
                let prefix = a.get(i / dy[axis]).map_or(String::new(), |&v| text(s, v));
                let suffix = b.get(i % dy[axis]).map_or(String::new(), |&v| text(s, v));
                names.push(ch(s, &format!("{prefix}:{suffix}"))?);
            }
            labels[axis] = s.alloc_str_from(&names)?;
        }
    }
    array_shape(ctx.session_mut(), value, &dest, &labels)
}

fn plain_vector(ctx: &mut Ctx, value: SexpId, n: usize) -> EvalResult {
    let index: Vec<_> = (0..n).map(Some).collect();
    Ok(gather(ctx.session_mut(), value, &index)?)
}
