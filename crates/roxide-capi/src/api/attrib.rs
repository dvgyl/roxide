//! Attribute getters, setters, names, dim, dimnames, class, and inherits.

use std::ffi::{c_char, c_int, c_void};

use roxide_eval::{Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{
    CharId, SEXP_NIL, SYM_CLASS, SYM_DIM, SYM_DIMNAMES, SYM_LEVELS, SYM_NAMES, SYM_ROW_NAMES,
    SexpType, Sym, ValueApi,
};

use super::pairlist::cells;
use super::string::{c_text, utf8};
use super::sym::{install, print_name};
use super::vector::{allocating, api, id, length};
use crate::abi::{Rboolean, Sexp};

pub(crate) fn inherits(ctx: &mut Ctx, x: SexpId, klass: &[u8]) -> EvalResult<bool> {
    let s = ctx.session();
    if !s.is_object(x)? {
        return Ok(false);
    }
    let classes = s.attr(x, SYM_CLASS);
    if s.type_of(classes)? != SexpType::Str {
        return Ok(false);
    }
    let n = s.xlength(classes)?;
    for i in 0..n {
        let ch = ctx.session_mut().str_get(classes, i)?;
        let s = ctx.session();
        if ch != CharId::NA && utf8(s.char_bytes(ch)?, s.char_encoding(ch)?) == klass {
            return Ok(true);
        }
    }
    Ok(false)
}

fn attr_name(ctx: &mut Ctx, key: SexpId) -> EvalResult<Sym> {
    if ctx.session().type_of(key)? == SexpType::Str && ctx.session().xlength(key)? > 0 {
        let ch = ctx.session_mut().str_get(key, 0)?;
        let s = ctx.session();
        let bytes = utf8(s.char_bytes(ch)?, s.char_encoding(ch)?).into_owned();
        let key = install(ctx, &bytes)?;
        return Ok(ctx.session().sexp_as_sym(key)?);
    }
    Ok(ctx.session().sexp_as_sym(key)?)
}

fn has_tag_names(ty: SexpType) -> bool {
    matches!(ty, SexpType::List | SexpType::Lang | SexpType::Dot)
}

fn names_from_tags(ctx: &mut Ctx, x: SexpId) -> EvalResult<SexpId> {
    let entries = cells(ctx, x)?;
    if entries.iter().all(|&(_, _, tag)| tag == SEXP_NIL) {
        return Ok(SEXP_NIL);
    }
    let names = entries
        .into_iter()
        .map(|(_, _, tag)| {
            if tag == SEXP_NIL {
                Ok(CharId::BLANK)
            } else {
                print_name(ctx, tag)
            }
        })
        .collect::<EvalResult<Vec<_>>>()?;
    Ok(ctx.session_mut().alloc_str_from(&names)?)
}

fn set_tag_names(ctx: &mut Ctx, x: SexpId, value: SexpId) -> EvalResult<()> {
    let entries = cells(ctx, x)?;
    let names = ctx.session_mut().coerce(value, SexpType::Str)?.0;
    let n = ctx.session().xlength(names)?;
    if n > entries.len() {
        return Err(ctx.stop(SEXP_NIL, "'names' attribute is longer than the pairlist"));
    }
    let mut tags = Vec::with_capacity(n);
    for i in 0..n {
        let name = ctx.session_mut().str_get(names, i)?;
        let s = ctx.session();
        let bytes = s.char_bytes(name)?;
        let tag = if name == CharId::NA || bytes.is_empty() {
            SEXP_NIL
        } else {
            let bytes = utf8(bytes, s.char_encoding(name)?).into_owned();
            install(ctx, &bytes)?
        };
        tags.push(tag);
    }
    let s = ctx.session_mut();
    for (i, (cell, _, _)) in entries.into_iter().enumerate() {
        s.set_tag(cell, tags.get(i).copied().unwrap_or(SEXP_NIL))?;
    }
    Ok(())
}

pub(crate) fn set_attribute(
    ctx: &mut Ctx,
    x: SexpId,
    key: Sym,
    mut value: SexpId,
) -> EvalResult<()> {
    if x == SEXP_NIL {
        if value == SEXP_NIL {
            return Ok(());
        }
        return Err(ctx.stop(SEXP_NIL, "attempt to set an attribute on NULL"));
    }
    if key == SYM_NAMES && has_tag_names(ctx.session().type_of(x)?) {
        return set_tag_names(ctx, x, value);
    }
    if value != SEXP_NIL {
        if key == SYM_DIM {
            let dims = ctx.session_mut().coerce(value, SexpType::Int)?.0;
            let dimensions = ctx.session().int(dims)?.to_vec();
            if dimensions.is_empty() {
                return Err(ctx.stop(SEXP_NIL, "length-0 dimension vector is invalid"));
            }
            let product = super::vector::dimensions_length(ctx, &dimensions)?;
            let n = length(ctx, x)?;
            if product != n {
                return Err(ctx.stop(
                    SEXP_NIL,
                    format!("dims [product {product}] do not match the length of object [{n}]"),
                ));
            }
            value = dims;
        } else if key == SYM_CLASS {
            let s = ctx.session();
            if s.type_of(value)? != SexpType::Str {
                return Err(ctx.stop(SEXP_NIL, "class attribute must be a character vector"));
            }
            if s.xlength(value)? == 0 {
                value = SEXP_NIL;
            }
        } else if key == SYM_DIMNAMES {
            value = dimnames(ctx, x, value)?;
        }
    }

    let s = ctx.session_mut();
    if key == SYM_DIM {
        s.set_attrib(x, SYM_DIMNAMES, SEXP_NIL)?;
        s.set_attrib(x, SYM_NAMES, SEXP_NIL)?;
    }
    Ok(s.set_attrib(x, key, value)?)
}

fn dimnames(ctx: &mut Ctx, x: SexpId, value: SexpId) -> EvalResult<SexpId> {
    let s = ctx.session();
    let dimensions = s.attr(x, SYM_DIM);
    if dimensions == SEXP_NIL {
        return Err(ctx.stop(SEXP_NIL, "'dimnames' applied to non-array"));
    }
    if s.type_of(value)? != SexpType::Vec {
        return Err(ctx.stop(SEXP_NIL, "'dimnames' must be a list"));
    }
    let dims = s.int(dimensions)?.to_vec();
    let items = s.list_slice(value)?.to_vec();
    if items.is_empty() {
        return Ok(SEXP_NIL);
    }
    if items.len() > dims.len() {
        return Err(ctx.stop(SEXP_NIL, "length of 'dimnames' must match that of 'dims'"));
    }
    let out = ctx.session_mut().alloc_list(dims.len())?;
    for (axis, (&item, &dim)) in items.iter().zip(&dims).enumerate() {
        if item == SEXP_NIL || ctx.session().xlength(item)? == 0 {
            continue;
        }
        let labels = ctx.session_mut().coerce(item, SexpType::Str)?.0;
        if ctx.session().xlength(labels)? != dim as usize {
            return Err(ctx.stop(
                SEXP_NIL,
                format!(
                    "length of 'dimnames' [{}] not equal to array extent",
                    axis + 1
                ),
            ));
        }
        ctx.session_mut().list_set(out, axis, labels)?;
    }
    let s = ctx.session_mut();
    s.set_attrib(out, SYM_NAMES, s.attr(value, SYM_NAMES))?;
    Ok(out)
}

pub(super) fn copy_attrs(
    ctx: &mut Ctx,
    source: SexpId,
    target: SexpId,
    most: bool,
) -> EvalResult<()> {
    if target == SEXP_NIL || source == target {
        return Ok(());
    }
    let s = ctx.session_mut();
    let mut entries = s.attrs(source)?;
    if most {
        entries.retain(|(key, _)| ![SYM_NAMES, SYM_DIM, SYM_DIMNAMES].contains(key));
        let mut target_entries = s.attrs(target)?;
        target_entries.extend(entries);
        entries = target_entries;
    }
    s.replace_attributes(target, entries)?;
    if !most || s.is_object(source)? {
        s.set_object_flag(target, s.is_object(source)?)?;
    }
    if !most || s.is_s4(source)? {
        s.set_s4_flag(target, s.is_s4(source)?)?;
    }
    Ok(())
}

pub(crate) fn factor_strings(ctx: &mut Ctx, x: SexpId) -> EvalResult<SexpId> {
    if ctx.session().type_of(x)? != SexpType::Int || !inherits(ctx, x, b"factor")? {
        return Err(ctx.stop(SEXP_NIL, "attempting to coerce non-factor"));
    }
    let levels = ctx.session().attr(x, SYM_LEVELS);
    let nlevels = ctx.session().xlength(levels)?;
    let codes = ctx.session().int(x)?.to_vec();
    let mut strings = Vec::with_capacity(codes.len());
    for code in codes {
        let ch = if code == roxide_value::NA_INTEGER {
            CharId::NA
        } else {
            if code < 1 || code as usize > nlevels {
                return Err(ctx.stop(SEXP_NIL, "malformed factor"));
            }
            ctx.session_mut().str_get(levels, code as usize - 1)?
        };
        strings.push(ch);
    }
    Ok(ctx.session_mut().alloc_str_from(&strings)?)
}

fn attr_names(ctx: &mut Ctx, attrs: &[(Sym, SexpId)]) -> EvalResult<SexpId> {
    let names = attrs
        .iter()
        .map(|(name, _)| print_name(ctx, name.as_sexp()))
        .collect::<EvalResult<Vec<_>>>()?;
    Ok(ctx.session_mut().alloc_str_from(&names)?)
}

type MapFn = unsafe extern "C" fn(Sexp, Sexp, *mut c_void) -> Sexp;

struct AttrCall {
    fun: MapFn,
    name: Sexp,
    value: Sexp,
    data: *mut c_void,
    result: Sexp,
}

unsafe extern "C" fn invoke_map(data: *mut c_void) {
    // SAFETY: The trampoline passes an exclusive pointer to its live call record.
    let call = unsafe { &mut *data.cast::<AttrCall>() };
    // SAFETY: The caller supplies the callback and its data. The handles are rooted.
    call.result = unsafe { (call.fun)(call.name, call.value, call.data) };
}

fn map_attributes(ctx: &mut Ctx, x: SexpId, fun: MapFn, data: *mut c_void) -> EvalResult<Sexp> {
    allocating(ctx, &[x], |ctx| {
        let attrs = ctx.session().attrs(x)?;
        for &(name, value) in &attrs {
            ctx.protect(name.as_sexp());
            ctx.protect(value);
        }
        for (name, value) in attrs {
            let mut call = AttrCall {
                fun,
                name: Sexp::from_id(name.as_sexp()),
                value: Sexp::from_id(value),
                data,
                result: Sexp::NULL,
            };
            // SAFETY: The call record is live until the callback returns or jumps.
            unsafe {
                crate::jump::enter_trampoline_with(
                    ctx,
                    crate::jump::FrameKind::Callback,
                    invoke_map,
                    std::ptr::from_mut(&mut call).cast(),
                )?
            };
            if call.result != Sexp::NULL {
                return Ok(call.result);
            }
        }
        Ok(Sexp::NULL)
    })
}

fn class(ctx: &mut Ctx, x: SexpId) -> EvalResult<SexpId> {
    let s = ctx.session_mut();
    let explicit = s.attr(x, SYM_CLASS);
    if explicit != SEXP_NIL && s.xlength(explicit)? > 0 {
        return Ok(explicit);
    }
    let names: &[&str] = match s.xlength(s.attr(x, SYM_DIM))? {
        0 => &[match s.type_of(x)? {
            SexpType::Real => "numeric",
            SexpType::Clo | SexpType::Builtin | SexpType::Special => "function",
            SexpType::Lang => s.call_class(x)?,
            SexpType::Sym => "name",
            ty => ty.type_name(s.is_s4(x)?),
        }],
        2 => &["matrix", "array"],
        _ => &["array"],
    };
    let names = names
        .iter()
        .map(|name| s.mk_char(name.as_bytes(), roxide_value::Encoding::Native))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(s.alloc_str_from(&names)?)
}

api! {
    /// Read an attribute by name and expand compact row names.
    fn Rf_getAttrib(x: Sexp, key: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        let x = id(ctx, x, "Rf_getAttrib")?;
        let key = id(ctx, key, "Rf_getAttrib")?;
        let ty = ctx.session().type_of(key)?;
        let tag_names = has_tag_names(ctx.session().type_of(x)?);
        if ty == SexpType::Sym && key != SYM_ROW_NAMES.as_sexp()
            && !(key == SYM_NAMES.as_sexp() && tag_names)
        {
            return Ok(Sexp::from_id(ctx.session().attr(x, ctx.session().sexp_as_sym(key)?)));
        }
        allocating(ctx, &[x, key], |ctx| {
            let key = attr_name(ctx, key)?;
            let value = if key == SYM_NAMES && tag_names {
                names_from_tags(ctx, x)?
            } else {
                ctx.session_mut().get_attrib_expanded(x, key)?
            };
            Ok(Sexp::from_id(value))
        })
    }
    /// Set or remove an attribute and return the supplied value.
    fn Rf_setAttrib(x: Sexp, key: Sexp, value: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        let x = id(ctx, x, "Rf_setAttrib")?;
        let key = id(ctx, key, "Rf_setAttrib")?;
        let v = id(ctx, value, "Rf_setAttrib")?;
        allocating(ctx, &[x, key, v], |ctx| {
            let key = attr_name(ctx, key)?;
            set_attribute(ctx, x, key, v)?;
            Ok(value)
        })
    }
    /// Copy attributes other than names, dimensions and dimension names.
    fn Rf_copyMostAttrib(source: Sexp, target: Sexp) -> () = (); |ctx| {
        let source = id(ctx, source, "Rf_copyMostAttrib")?;
        let target = id(ctx, target, "Rf_copyMostAttrib")?;
        allocating(ctx, &[source, target], |ctx| copy_attrs(ctx, source, target, true))
    }
    /// Copy attributes and object flags from the source to the target.
    fn Rf_copyAttributes(source: Sexp, target: Sexp) -> () = (); |ctx| {
        let source = id(ctx, source, "Rf_copyAttributes")?;
        let target = id(ctx, target, "Rf_copyAttributes")?;
        allocating(ctx, &[source, target], |ctx| copy_attrs(ctx, source, target, false))
    }
    /// Test inheritance from a named explicit class.
    fn Rf_inherits(x: Sexp, klass: *const c_char) -> Rboolean = 0; |ctx| {
        let x = id(ctx, x, "Rf_inherits")?;
        let klass = c_text(ctx, klass, "Rf_inherits")?;
        Ok(c_int::from(inherits(ctx, x, klass)?))
    }
    /// Test whether an object has at least one attribute.
    fn ANY_ATTRIB(x: Sexp) -> c_int = 0; |ctx| {
        let x = id(ctx, x, "ANY_ATTRIB")?;
        Ok(c_int::from(!ctx.session().attrs(x)?.is_empty()))
    }
    /// Remove all attributes and clear both class bits.
    fn CLEAR_ATTRIB(x: Sexp) -> () = (); |ctx| {
        let x = id(ctx, x, "CLEAR_ATTRIB")?;
        let s = ctx.session_mut();
        s.clear_attrs(x)?;
        Ok(s.set_s4_flag(x, false)?)
    }
    /// Return the number of attributes.
    fn R_getAttribCount(x: Sexp) -> c_int = 0; |ctx| {
        let x = id(ctx, x, "R_getAttribCount")?;
        let n = ctx.session().attrs(x)?.len();
        c_int::try_from(n).map_err(|_| ctx.stop(SEXP_NIL, "too many attributes"))
    }
    /// Test whether the named attribute is present.
    fn R_hasAttrib(x: Sexp, key: Sexp) -> bool = false; |ctx| {
        let x = id(ctx, x, "R_hasAttrib")?;
        let key = id(ctx, key, "R_hasAttrib")?;
        let key = ctx.session().sexp_as_sym(key)?;
        Ok(ctx.session().attr(x, key) != SEXP_NIL)
    }
    /// Return the attribute names in order.
    fn R_getAttribNames(x: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        let x = id(ctx, x, "R_getAttribNames")?;
        allocating(ctx, &[x], |ctx| {
            let attrs = ctx.session().attrs(x)?;
            attr_names(ctx, &attrs).map(Sexp::from_id)
        })
    }
    /// Return a named list of attributes, or R NULL if there are none.
    fn R_getAttributes(x: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        let x = id(ctx, x, "R_getAttributes")?;
        allocating(ctx, &[x], |ctx| {
            let attrs = ctx.session().attrs(x)?;
            if attrs.is_empty() { return Ok(Sexp::from_id(SEXP_NIL)); }
            let list = ctx.session_mut().alloc_list(attrs.len())?;
            let names = attr_names(ctx, &attrs)?;
            let s = ctx.session_mut();
            for (i, &(name, value)) in attrs.iter().enumerate() {
                let item = if name == SYM_ROW_NAMES { s.get_attrib_expanded(x, name)? } else { value };
                s.list_set(list, i, item)?;
            }
            s.set_attrib(list, SYM_NAMES, names)?;
            Ok(Sexp::from_id(list))
        })
    }
    /// Get row names with compact row names expanded.
    fn Rf_GetRowNames(x: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        let x = id(ctx, x, "Rf_GetRowNames")?;
        allocating(ctx, &[x], |ctx| {
            Ok(Sexp::from_id(ctx.session_mut().attr_row_names(x)?))
        })
    }
    /// Return the second component of a dimension names list.
    fn Rf_GetColNames(x: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        let x = id(ctx, x, "Rf_GetColNames")?;
        if x == SEXP_NIL {
            return Ok(Sexp::from_id(SEXP_NIL));
        }
        Ok(Sexp::from_id(ctx.session().list_get(x, 1)?))
    }
    /// Return explicit or implicit class names.
    fn R_class(x: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        let x = id(ctx, x, "R_class")?;
        allocating(ctx, &[x], |ctx| class(ctx, x).map(Sexp::from_id))
    }
    /// Visit attributes until the callback returns a non-null pointer.
    fn R_mapAttrib(x: Sexp, fun: Option<MapFn>, data: *mut c_void) -> Sexp = Sexp::NULL; |ctx| {
        let x = id(ctx, x, "R_mapAttrib")?;
        let fun = fun.ok_or_else(|| ctx.stop(SEXP_NIL, "invalid NULL attribute callback"))?;
        map_attributes(ctx, x, fun, data)
    }
    /// Copy the attribute list and share its values.
    fn SHALLOW_DUPLICATE_ATTRIB(target: Sexp, source: Sexp) -> () = (); |ctx| {
        let target = id(ctx, target, "SHALLOW_DUPLICATE_ATTRIB")?;
        let source = id(ctx, source, "SHALLOW_DUPLICATE_ATTRIB")?;
        allocating(ctx, &[target, source], |ctx| copy_attrs(ctx, source, target, false))
    }
    /// Copy the attribute list and duplicate its values.
    fn DUPLICATE_ATTRIB(target: Sexp, source: Sexp) -> () = (); |ctx| {
        let target = id(ctx, target, "DUPLICATE_ATTRIB")?;
        let source = id(ctx, source, "DUPLICATE_ATTRIB")?;
        allocating(ctx, &[target, source], |ctx| {
            if target == source || target == SEXP_NIL { return Ok(()); }
            let s = ctx.session_mut();
            let mut entries = s.attrs(source)?;
            for (_, value) in &mut entries {
                *value = s.deep_duplicate(*value)?;
            }
            s.replace_attributes(target, entries)?;
            s.set_object_flag(target, s.is_object(source)?)?;
            Ok(s.set_s4_flag(target, s.is_s4(source)?)?)
        })
    }
    /// Return the explicit S3 class.
    fn Rf_S3Class(x: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        let x = id(ctx, x, "Rf_S3Class")?;
        Ok(Sexp::from_id(ctx.session().attr(x, SYM_CLASS)))
    }
    /// Convert a factor to its character labels.
    fn Rf_asCharacterFactor(x: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        let x = id(ctx, x, "Rf_asCharacterFactor")?;
        allocating(ctx, &[x], |ctx| factor_strings(ctx, x).map(Sexp::from_id))
    }
}

macro_rules! attrib_setters {
    ($($name:ident => $sym:ident),+ $(,)?) => {
        api! {$(
            /// Set the corresponding attribute and return the object.
            fn $name(x: Sexp, value: Sexp) -> Sexp = Sexp::NULL; |ctx| {
                let x = id(ctx, x, stringify!($name))?;
                let value = id(ctx, value, stringify!($name))?;
                allocating(ctx, &[x, value], |ctx| {
                    set_attribute(ctx, x, $sym, value)?;
                    Ok(Sexp::from_id(x))
                })
            }
        )+}
    }
}

attrib_setters!(Rf_classgets => SYM_CLASS, Rf_namesgets => SYM_NAMES,
               Rf_dimgets => SYM_DIM, Rf_dimnamesgets => SYM_DIMNAMES);

macro_rules! dimnames_exports {
    ($($name:ident),+ $(,)?) => {
        api! {$(
            /// Return the dimension names attribute.
            fn $name(x: Sexp) -> Sexp = Sexp::NULL; |ctx| {
                let x = id(ctx, x, stringify!($name))?;
                Ok(Sexp::from_id(ctx.session().attr(x, SYM_DIMNAMES)))
            }
        )+}
    }
}

dimnames_exports!(Rf_GetMatrixDimnames, Rf_GetArrayDimnames);

macro_rules! duplicates {
    ($($name:ident => $method:ident, $deep:expr);+ $(;)?) => {
        api! {$(
            /// Duplicate an R object.
            fn $name(x: Sexp) -> Sexp = Sexp::NULL; |ctx| {
                let x = id(ctx, x, stringify!($name))?;
                allocating(ctx, &[x], |ctx| {
                    if let Some(value) = super::altrep::c_duplicate(ctx, x, $deep)? { return Ok(Sexp::from_id(value)); }
                    Ok(Sexp::from_id(ctx.session_mut().$method(x)?))
                })
            }
        )+}
    }
}

duplicates!(Rf_shallow_duplicate => shallow_duplicate, false; Rf_duplicate => deep_duplicate, true);

api! {
    /// Mark an object as shared and return it.
    fn Rf_lazy_duplicate(x: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        let value = id(ctx, x, "Rf_lazy_duplicate")?;
        ctx.session_mut().heap.mark_immutable(value);
        Ok(x)
    }
}
