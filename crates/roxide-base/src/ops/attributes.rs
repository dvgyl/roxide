use roxide_eval::{Args, Call, Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{CharId, NA_INTEGER, RLogical, RSession, SEXP_NIL, SexpType, ValueApi};

use super::common::{
    self, attr as get, ch, chars, error, flag, formals, list, set, string, tag, text,
};
use SexpType::*;

pub(super) fn attributes(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let s = ctx.session_mut();
    let attrs = s.attrs(args.get(0))?;
    if attrs.is_empty() {
        return Ok(SEXP_NIL);
    }
    let mut names = std::vec::Vec::new();
    let mut values = std::vec::Vec::new();
    for (sym, _) in attrs {
        let name = s.sym_name(sym)?.to_owned();
        names.push(ch(s, &name)?);
        values.push(s.get_attrib_expanded(args.get(0), sym)?);
    }
    let out = list(s, &values)?;
    common::set_names(s, out, &names)?;
    Ok(out)
}

pub(super) fn attr(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, name, exact] = formals(ctx.session(), args, ["x", "which", "exact"]);
    let Some(name) = string(ctx.session(), name) else {
        return Err(error(ctx, call, "'which' must be of mode character"));
    };
    let exact = flag(ctx, exact, false)?;
    let s = ctx.session_mut();
    let sym = s.intern(&name)?;
    let v = s.get_attrib_expanded(x, sym)?;
    if exact || v != SEXP_NIL {
        return Ok(v);
    }
    let matches: std::vec::Vec<_> = s
        .attrs(x)?
        .into_iter()
        .filter(|(n, _)| s.sym_name(*n).is_ok_and(|n| n.starts_with(&name)))
        .collect();
    if let [(sym, _)] = matches.as_slice() {
        Ok(s.get_attrib_expanded(x, *sym)?)
    } else {
        Ok(SEXP_NIL)
    }
}

pub(super) fn set_attr(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, name, value] = formals(ctx.session(), args, ["x", "which", "value"]);
    let Some(name) = string(ctx.session(), name) else {
        return Err(error(ctx, call, "'which' must be of mode character"));
    };
    replace_attr(ctx, call, x, &name, value)
}

pub(super) fn set_attributes(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, value] = formals(ctx.session(), args, ["x", "value"]);
    if x == SEXP_NIL && value == SEXP_NIL {
        return Ok(x);
    }
    let s = ctx.session_mut();
    if value != SEXP_NIL && (s.type_of(value)? != Vec || common::names(s, value).is_empty()) {
        return Err(error(ctx, call, "attributes must be named"));
    }
    let x = s.unshare(x)?;
    s.clear_attrs(x)?;
    if value == SEXP_NIL {
        return Ok(x);
    }
    let attrs: std::vec::Vec<_> = common::names(s, value)
        .iter()
        .zip(s.list_slice(value)?)
        .map(|(&n, &v)| (text(s, n), v))
        .collect();
    let mut out = x;
    for (n, v) in attrs {
        out = replace_attr(ctx, call, out, &n, v)?;
    }
    Ok(out)
}

fn replace_attr(ctx: &mut Ctx, call: &Call, x: SexpId, name: &str, value: SexpId) -> EvalResult {
    if x == SEXP_NIL {
        return if value == SEXP_NIL {
            Ok(x)
        } else {
            Err(error(ctx, call, "attempt to set an attribute on NULL"))
        };
    }
    let s = ctx.session_mut();
    if name == "names" && s.type_of(x)? == List {
        let values: std::vec::Vec<_> = s.pairlist_iter(x).map(|(_, v)| v).collect();
        let labels = if value == SEXP_NIL {
            std::vec::Vec::new()
        } else {
            let (value, _) = s.coerce(value, Str)?;
            s.str_slice(value)?.to_vec()
        };
        if labels.len() > values.len() {
            return Err(error(
                ctx,
                call,
                format!(
                    "'names' attribute [{}] must be the same length as the vector [{}]",
                    labels.len(),
                    values.len()
                ),
            ));
        }
        let mut result = SEXP_NIL;
        for (i, val) in values.into_iter().enumerate().rev() {
            let tag = match labels.get(i) {
                Some(&id) if id != CharId::NA && id != CharId::BLANK => {
                    let label = text(s, id);
                    s.intern(&label)?.as_sexp()
                }
                _ => SEXP_NIL,
            };
            result = s.cons(val, result, tag)?;
        }
        common::copy_attrs(s, x, result, |name| name != "names")?;
        return Ok(result);
    }
    let x = s.unshare(x)?;
    let mut value = value;
    if value != SEXP_NIL {
        match name {
            "dim" => {
                (value, _) = s.coerce(value, Int)?;
                let d = s.int(value)?;
                let product = d.iter().try_fold(1usize, |acc, v| {
                    usize::try_from(v).ok().and_then(|v| acc.checked_mul(v))
                });
                if d.is_empty() || product != Some(s.xlength(x)?) {
                    return Err(error(ctx, call, "dims do not match the length of object"));
                }
            }
            "class" | "levels" => {
                (value, _) = s.coerce(value, Str)?;
                if name == "class" && s.xlength(value)? == 0 {
                    value = SEXP_NIL;
                }
            }
            "dimnames" => {
                let dims = get(s, x, "dim");
                if dims == SEXP_NIL {
                    return Err(error(ctx, call, "'dimnames' applied to non-array"));
                }
                let dims = s.int(dims)?.to_vec();
                let values = s.list_slice(value)?.to_vec();
                if values.len() > dims.len() {
                    return Err(error(
                        ctx,
                        call,
                        "length of 'dimnames' must match that of 'dims'",
                    ));
                }
                let out = s.alloc_list(dims.len())?;
                for (i, v) in values.into_iter().enumerate() {
                    if v == SEXP_NIL {
                        continue;
                    }
                    let (v, _) = s.coerce(v, Str)?;
                    let n = s.xlength(v)?;
                    if n != 0 && n != dims[i] as usize {
                        return Err(error(
                            ctx,
                            call,
                            format!("length of 'dimnames' [{}] not equal to array extent", i + 1),
                        ));
                    }
                    if n != 0 {
                        s.list_set(out, i, v)?;
                    }
                }
                let names = get(s, value, "names");
                set(s, out, "names", names)?;
                value = out;
            }
            _ => {}
        }
    }
    if name == "dim" {
        set(s, x, "dimnames", SEXP_NIL)?;
        set(s, x, "names", SEXP_NIL)?;
    }
    if name == "names" {
        let d = get(s, x, "dim");
        if s.xlength(d)? == 1 {
            let v = if value == SEXP_NIL {
                SEXP_NIL
            } else {
                list(s, &[value])?
            };
            return replace_attr(ctx, call, x, "dimnames", v);
        }
    }
    set(s, x, name, value).map_err(|e| error(ctx, call, e.to_string()))?;
    Ok(x)
}

macro_rules! access {
    ($($get:ident, $set:ident, $name:literal),* $(,)?) => {$(
        pub(super) fn $get(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
            Ok(get(ctx.session(), args.get(0), $name))
        }
        pub(super) fn $set(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
            let [x, value] = formals(ctx.session(), args, ["x", "value"]);
            replace_attr(ctx, call, x, $name, value)
        }
    )*};
}
access!(old_class, set_class, "class",);

pub(super) fn dimnames(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let s = ctx.session_mut();
    if has_class(s, x, "data.frame") {
        let rows = s.attr_row_names(x)?;
        let (rows, _) = s.coerce(rows, Str)?;
        return Ok(list(s, &[rows, get(s, x, "names")])?);
    }
    Ok(get(s, x, "dimnames"))
}

pub(super) fn set_dimnames(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, value] = formals(ctx.session(), args, ["x", "value"]);
    if !has_class(ctx.session(), x, "data.frame") {
        return replace_attr(ctx, call, x, "dimnames", value);
    }
    let s = ctx.session_mut();
    let nrow = row_count(s, x)?;
    let ncol = s.xlength(x)?;
    let [rows, cols] = if value == SEXP_NIL {
        [SEXP_NIL, SEXP_NIL]
    } else if let [rows, cols] = s.list_slice(value)? {
        [*rows, *cols]
    } else {
        return Err(error(ctx, call, "invalid 'dimnames' given for data frame"));
    };
    let rows = if rows == SEXP_NIL {
        if nrow == 0 {
            s.alloc_int_from(&[])?
        } else {
            s.alloc_int_from(&[NA_INTEGER, -(nrow as i32)])?
        }
    } else {
        let (rows, _) = s.coerce(rows, Str)?;
        let labels = s.str_slice(rows)?;
        let mut seen = std::collections::HashSet::new();
        if labels.len() != nrow
            || labels
                .iter()
                .any(|&id| id == CharId::NA || !seen.insert(id))
        {
            return Err(error(ctx, call, "invalid 'row.names' length or values"));
        }
        rows
    };
    if cols != SEXP_NIL && s.xlength(cols)? != ncol {
        return Err(error(ctx, call, "invalid 'dimnames' given for data frame"));
    }
    let x = s.unshare(x)?;
    set(s, x, "row.names", rows)?;
    set(s, x, "names", cols)?;
    Ok(x)
}

pub(super) fn levels(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    Ok(get(ctx.session(), args.get(0), "levels"))
}

pub(super) fn set_levels(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, value] = formals(ctx.session(), args, ["x", "value"]);
    if !has_class(ctx.session(), x, "factor") {
        return replace_attr(ctx, call, x, "levels", value);
    }
    let s = ctx.session_mut();
    let old = get(s, x, "levels");
    let (value, _) = s.coerce(value, Str)?;
    let labels = s.str_slice(value)?;
    if labels.len() < s.xlength(old)? {
        return Err(error(ctx, call, "number of levels differs"));
    }
    let mut new = std::vec::Vec::new();
    let mut lookup = std::collections::HashMap::new();
    let map: std::vec::Vec<_> = labels
        .iter()
        .map(|&label| {
            if label == CharId::NA {
                return NA_INTEGER;
            }
            let pos = (new.len() + 1) as i32;
            *lookup.entry(label).or_insert_with(|| {
                new.push(label);
                pos
            })
        })
        .collect();
    let codes: std::vec::Vec<_> = s
        .int(x)?
        .iter()
        .map(|v| {
            usize::try_from(v.saturating_sub(1))
                .ok()
                .and_then(|i| map.get(i))
                .copied()
                .unwrap_or(NA_INTEGER)
        })
        .collect();
    let out = s.alloc_int_from(&codes)?;
    let levels = s.alloc_str_from(&new)?;
    for (sym, v) in s.attrs(x)? {
        s.set_attrib(
            out,
            sym,
            if s.sym_name(sym)? == "levels" {
                levels
            } else {
                v
            },
        )?;
    }
    Ok(out)
}

pub(super) fn names(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let s = ctx.session_mut();
    let d = get(s, x, "dim");
    if s.xlength(d)? == 1 {
        let names = get(s, x, "dimnames");
        if names != SEXP_NIL {
            return Ok(s.list_get(names, 0)?);
        }
    }
    if s.type_of(x)? == List {
        let names: std::vec::Vec<_> = s
            .pairlist_iter(x)
            .map(|(name, _)| tag(s, name).unwrap_or("").to_owned())
            .collect();
        if names.iter().all(String::is_empty) {
            return Ok(SEXP_NIL);
        }
        let names = names
            .iter()
            .map(|n| ch(s, n))
            .collect::<Result<std::vec::Vec<_>, _>>()?;
        return Ok(s.alloc_str_from(&names)?);
    }
    Ok(get(s, x, "names"))
}

pub(super) fn set_names(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, value] = formals(ctx.session(), args, ["x", "value"]);
    replace_attr(ctx, call, x, "names", value)
}

pub(super) fn dim(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    let s = ctx.session_mut();
    if has_class(s, x, "data.frame") {
        let nrow = row_count(s, x)?;
        return Ok(s.alloc_int_from(&[nrow as i32, s.xlength(x)? as i32])?);
    }
    Ok(get(s, x, "dim"))
}

pub(super) fn set_dim(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, value] = formals(ctx.session(), args, ["x", "value"]);
    replace_attr(ctx, call, x, "dim", value)
}

pub(crate) fn row_count(s: &mut RSession, x: SexpId) -> Result<usize, roxide_value::ValueError> {
    let rows = get(s, x, "row.names");
    if s.type_of(rows)? == Int {
        let v = s.int(rows)?;
        if v.len() == 2 && v.get(0) == NA_INTEGER && v.get(1) != NA_INTEGER {
            return Ok(v.get(1).unsigned_abs() as usize);
        }
    }
    s.xlength(rows)
}

fn axis_names(ctx: &mut Ctx, x: SexpId, axis: usize) -> EvalResult {
    let s = ctx.session_mut();
    if has_class(s, x, "data.frame") {
        if axis == 0 {
            let rows = s.attr_row_names(x)?;
            return Ok(s.coerce(rows, Str)?.0);
        }
        return Ok(get(s, x, "names"));
    }
    let dims = get(s, x, "dimnames");
    Ok(s.list_get(dims, axis).unwrap_or(SEXP_NIL))
}

pub(super) fn rownames(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    axis_names(ctx, args.get(0), 0)
}
pub(super) fn colnames(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    axis_names(ctx, args.get(0), 1)
}

pub(crate) fn has_class(s: &RSession, x: SexpId, name: &str) -> bool {
    chars(s, get(s, x, "class"))
        .iter()
        .any(|&id| s.char_bytes(id).ok() == Some(name.as_bytes()))
}

fn class_names(s: &RSession, x: SexpId) -> Result<std::vec::Vec<String>, roxide_value::ValueError> {
    let classes = chars(s, get(s, x, "class"));
    if !classes.is_empty() {
        return Ok(classes.iter().map(|&v| text(s, v)).collect());
    }
    match s.xlength(get(s, x, "dim"))? {
        0 => {}
        2 => return Ok(vec!["matrix".into(), "array".into()]),
        _ => return Ok(vec!["array".into()]),
    }
    let ty = s.type_of(x)?;
    Ok(vec![
        match ty {
            Real => "numeric",
            Clo | Builtin | Special => "function",
            Lang => s.call_class(x)?,
            Sym => "name",
            _ => ty.type_name(s.is_s4(x)?),
        }
        .into(),
    ])
}

pub(super) fn class(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let s = ctx.session_mut();
    let explicit = get(s, args.get(0), "class");
    if explicit != SEXP_NIL && s.xlength(explicit)? > 0 {
        return Ok(explicit);
    }
    let names = class_names(s, args.get(0))?;
    let names = names
        .iter()
        .map(|n| ch(s, n))
        .collect::<Result<std::vec::Vec<_>, _>>()?;
    Ok(s.alloc_str_from(&names)?)
}

pub(super) fn inherits(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let [x, what, which] = formals(ctx.session(), args, ["x", "what", "which"]);
    let which = flag(ctx, which, false)?;
    let classes = if ctx.session().is_s4(x)?
        && let Some(chain) = ctx.s4_class_chain(x)?
        && let Ok(slice) = ctx.session().str_slice(chain)
        && !slice.is_empty()
    {
        slice
            .iter()
            .map(|&id| text(ctx.session(), id))
            .collect::<std::vec::Vec<_>>()
    } else {
        class_names(ctx.session_mut(), x)?
    };
    let s = ctx.session_mut();
    let what = s.str_slice(what)?;
    let matches: std::vec::Vec<_> = what
        .iter()
        .map(|&id| {
            classes
                .iter()
                .position(|v| s.char_bytes(id).ok() == Some(v.as_bytes()))
                .map_or(0, |i| (i + 1) as i32)
        })
        .collect();
    if which {
        Ok(s.alloc_int_from(&matches)?)
    } else {
        Ok(s.scalar_lgl(RLogical::from_option(Some(matches.iter().any(|&i| i != 0))))?)
    }
}

pub(super) fn structure(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let data_pos = args
        .iter()
        .position(|(n, _)| tag(ctx.session(), n) == Some(".Data"))
        .or_else(|| args.iter().position(|(n, _)| n.is_none()));
    let Some(i) = data_pos else {
        return Err(error(
            ctx,
            call,
            "argument '.Data' is missing, with no default",
        ));
    };
    let updates: std::vec::Vec<_> = args
        .iter()
        .enumerate()
        .filter(|(j, _)| *j != i)
        .map(|(_, (n, v))| (tag(ctx.session(), n).unwrap_or("").to_owned(), v))
        .collect();
    let mut x = args.get(i);
    if x == SEXP_NIL {
        return Err(error(ctx, call, "attempt to set an attribute on NULL"));
    }
    for (name, value) in updates {
        let name = match name.as_str() {
            ".Names" => "names",
            ".Dim" => "dim",
            ".Dimnames" => "dimnames",
            ".Tsp" => "tsp",
            ".Label" => "levels",
            n => n,
        };
        if name.is_empty() {
            return Err(error(ctx, call, "attributes must be named"));
        }
        x = replace_attr(ctx, call, x, name, value)?;
    }
    Ok(x)
}

pub(super) fn unname(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, force] = formals(ctx.session(), args, ["obj", "force"]);
    let force = flag(ctx, force, false)?;
    let x = if force || !has_class(ctx.session(), x, "data.frame") {
        replace_attr(ctx, call, x, "names", SEXP_NIL)?
    } else {
        x
    };
    if get(ctx.session(), x, "dimnames") != SEXP_NIL {
        replace_attr(ctx, call, x, "dimnames", SEXP_NIL)
    } else {
        Ok(x)
    }
}

pub(super) fn unclass(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let x = args.get(0);
    if matches!(ctx.session().type_of(x)?, Env | ExtPtr) {
        return Err(error(
            ctx,
            call,
            "cannot unclass an environment or external pointer",
        ));
    }
    replace_attr(ctx, call, x, "class", SEXP_NIL)
}

pub(super) fn set_length(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let [x, value] = formals(ctx.session(), args, ["x", "value"]);
    let (len_sexp, _) = ctx.session_mut().coerce(value, Real)?;
    if ctx.session().xlength(len_sexp)? != 1 {
        return Err(error(ctx, call, "invalid value"));
    }
    let raw_len = ctx.session().real(len_sexp)?.get(0);
    if raw_len.is_nan() {
        return Err(error(ctx, call, "vector size cannot be NA"));
    }
    if raw_len < 0.0 || !raw_len.is_finite() {
        return Err(error(ctx, call, "vector size cannot be negative"));
    }
    let new_len = raw_len.trunc() as usize;
    if x == SEXP_NIL {
        if new_len > 0 {
            ctx.warn_now(call.err_call(), "length of NULL cannot be changed")?;
        }
        return Ok(SEXP_NIL);
    }
    let max_bytes = ctx.host().limits().max_vector_bytes;
    let ty = ctx.session().type_of(x)?;
    let elem_bytes = match ty {
        Raw => 1,
        Lgl | Int => 4,
        Real | Str | Vec => 8,
        Cplx => 16,
        _ => 16,
    };
    if new_len > max_bytes / elem_bytes {
        return Err(error(ctx, call, "vector size limit exceeded"));
    }
    ctx.scoped(|ctx| {
        ctx.protect(x);
        let old_names = get(ctx.session(), x, "names");
        let old_len = ctx.session().xlength(x)?;
        let out = match ty {
            Lgl => {
                let mut vals = vec![RLogical::NA; new_len];
                let src = ctx.session().lgl(x)?;
                for (i, slot) in vals.iter_mut().take(old_len.min(new_len)).enumerate() {
                    *slot = src.get(i);
                }
                ctx.session_mut().alloc_lgl_from(&vals)?
            }
            Int => {
                let mut vals = vec![NA_INTEGER; new_len];
                let src = ctx.session().int(x)?;
                for (i, slot) in vals.iter_mut().take(old_len.min(new_len)).enumerate() {
                    *slot = src.get(i);
                }
                ctx.session_mut().alloc_int_from(&vals)?
            }
            Real => {
                let mut vals = vec![roxide_value::NA_REAL; new_len];
                let src = ctx.session().real(x)?;
                for (i, slot) in vals.iter_mut().take(old_len.min(new_len)).enumerate() {
                    *slot = src.get(i);
                }
                ctx.session_mut().alloc_real_from(&vals)?
            }
            Cplx => {
                let mut vals = vec![roxide_value::RComplex::NA; new_len];
                let src = ctx.session().cplx(x)?;
                for (i, slot) in vals.iter_mut().take(old_len.min(new_len)).enumerate() {
                    *slot = src.get(i);
                }
                ctx.session_mut().alloc_cplx_from(&vals)?
            }
            Str => {
                let mut vals = vec![CharId::NA; new_len];
                let src = ctx.session().str_slice(x)?;
                let copy = old_len.min(new_len);
                vals[..copy].copy_from_slice(&src[..copy]);
                ctx.session_mut().alloc_str_from(&vals)?
            }
            Raw => {
                let mut vals = vec![0u8; new_len];
                let src = ctx.session().raw(x)?;
                for (i, slot) in vals.iter_mut().take(old_len.min(new_len)).enumerate() {
                    *slot = src.get(i);
                }
                ctx.session_mut().alloc_raw_from(&vals)?
            }
            Vec => {
                let src = ctx.session().list_slice(x)?.to_vec();
                let out = ctx.session_mut().alloc_list(new_len)?;
                ctx.protect(out);
                for (i, &item) in src.iter().take(new_len).enumerate() {
                    ctx.session_mut().list_set(out, i, item)?;
                }
                out
            }
            List => {
                let mut items: std::vec::Vec<_> = ctx.session().pairlist_iter(x).collect();
                items.truncate(new_len);
                while items.len() < new_len {
                    items.push((None, SEXP_NIL));
                }
                let mut out = SEXP_NIL;
                for (tag, val) in items.into_iter().rev() {
                    let tag_sexp = tag.map_or(SEXP_NIL, |t| t.as_sexp());
                    out = ctx.session_mut().cons(val, out, tag_sexp)?;
                    ctx.protect(out);
                }
                out
            }
            _ => {
                return Err(error(
                    ctx,
                    call,
                    format!("invalid argument type '{}'", ty.type_name(false)),
                ));
            }
        };
        ctx.protect(out);
        if old_names != SEXP_NIL && matches!(ty, Lgl | Int | Real | Cplx | Str | Raw | Vec) {
            let src_names = ctx.session().str_slice(old_names)?.to_vec();
            let mut new_names = vec![CharId::BLANK; new_len];
            let copy = src_names.len().min(new_len);
            new_names[..copy].copy_from_slice(&src_names[..copy]);
            let names_sexp = ctx.session_mut().alloc_str_from(&new_names)?;
            ctx.protect(names_sexp);
            set(ctx.session_mut(), out, "names", names_sexp)?;
        }
        Ok(out)
    })
}
