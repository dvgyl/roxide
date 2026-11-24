//! Cons cells, pairlists, language calls (`CAR`/`CDR`/`TAG`/`Rf_cons`/`Rf_lang*`).

use std::collections::HashSet;
use std::ffi::c_int;

use roxide_eval::{Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{RObject, SEXP_NIL, ValueApi};

use super::vector::{allocating, api, check_capacity, checked_len, id};
use crate::abi::Sexp;

fn parts(ctx: &Ctx, x: SexpId) -> EvalResult<(SexpId, SexpId, SexpId)> {
    if x == SEXP_NIL {
        return Ok((x, x, x));
    }
    Ok(ctx.session().cons_parts(x)?)
}

pub(crate) fn cells(ctx: &mut Ctx, mut x: SexpId) -> EvalResult<Vec<(SexpId, SexpId, SexpId)>> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    while x != SEXP_NIL {
        if !seen.insert(x) {
            return Err(ctx.stop(SEXP_NIL, "cyclic pairlist"));
        }
        let (car, cdr, tag) = parts(ctx, x)?;
        result.push((x, car, tag));
        x = cdr;
    }
    Ok(result)
}

pub(crate) fn alloc_cells(ctx: &mut Ctx, n: usize, lang: bool) -> EvalResult<SexpId> {
    check_capacity(ctx, n, size_of::<RObject>())?;
    let mut out = SEXP_NIL;
    let session = ctx.session_mut();
    for i in 0..n {
        out = if lang && i == n - 1 {
            session.lcons(SEXP_NIL, out, SEXP_NIL)?
        } else {
            session.cons(SEXP_NIL, out, SEXP_NIL)?
        };
    }
    Ok(out)
}

fn construct(ctx: &mut Ctx, args: &[Sexp], name: &str, lang: bool) -> EvalResult<Sexp> {
    let args = args
        .iter()
        .map(|&arg| id(ctx, arg, name))
        .collect::<EvalResult<Vec<_>>>()?;
    allocating(ctx, &args, |ctx| {
        let mut head = SEXP_NIL;
        let s = ctx.session_mut();
        for (i, &arg) in args.iter().enumerate().rev() {
            head = if i == 0 && lang {
                s.lcons(arg, head, SEXP_NIL)?
            } else {
                s.cons(arg, head, SEXP_NIL)?
            };
        }
        Ok(Sexp::from_id(head))
    })
}

macro_rules! constructors {
    ($($name:ident, $lang:expr, [$($arg:ident),+]);+ $(;)?) => {
        api! {$(
            /// Construct a pairlist or language call from the supplied elements.
            fn $name($($arg: Sexp),+) -> Sexp = Sexp::NULL; |ctx| {
                construct(ctx, &[$($arg),+], stringify!($name), $lang)
            }
        )+}
    };
}

constructors!(
    Rf_list1, false, [a];
    Rf_list2, false, [a, b];
    Rf_list3, false, [a, b, c];
    Rf_list4, false, [a, b, c, d];
    Rf_list5, false, [a, b, c, d, e];
    Rf_list6, false, [a, b, c, d, e, f];
    Rf_lang1, true, [a];
    Rf_lang2, true, [a, b];
    Rf_lang3, true, [a, b, c];
    Rf_lang4, true, [a, b, c, d];
    Rf_lang5, true, [a, b, c, d, e];
    Rf_lang6, true, [a, b, c, d, e, f];
);

fn walk(ctx: &Ctx, mut x: SexpId, path: &[usize]) -> EvalResult<SexpId> {
    for &part in path {
        let (car, cdr, tag) = parts(ctx, x)?;
        x = [car, cdr, tag][part];
    }
    Ok(x)
}

macro_rules! selectors {
    ($($name:ident => [$($step:literal),+]),+ $(,)?) => {
        api! {$(
            /// Follow the indicated pairlist fields.
            fn $name(x: Sexp) -> Sexp = Sexp::NULL; |ctx| {
                let x = id(ctx, x, stringify!($name))?;
                Ok(Sexp::from_id(walk(ctx, x, &[$($step),+])?))
            }
        )+}
    };
}
selectors!(CAR => [0], CDR => [1], TAG => [2], CAAR => [0, 0],
           CDAR => [0, 1], CADR => [1, 0], CDDR => [1, 1],
           CADDR => [1, 1, 0], CADDDR => [1, 1, 1, 0], CAD4R => [1, 1, 1, 1, 0],
           CAD5R => [1, 1, 1, 1, 1, 0], CDDDR => [1, 1, 1]);

macro_rules! setters {
    ($($name:ident => $setter:ident [$($step:literal),*]),+ $(,)?) => {
        api! {$(
            /// Set the indicated pairlist field and return the value.
            fn $name(x: Sexp, value: Sexp) -> Sexp = Sexp::NULL; |ctx| {
                let name = stringify!($name);
                let x = id(ctx, x, name)?;
                let v = id(ctx, value, name)?;
                let x = walk(ctx, x, &[$($step),*])?;
                ctx.session_mut().$setter(x, v)?;
                Ok(value)
            }
        )+}
    };
}

setters!(SETCAR => set_car [], SETCDR => set_cdr [], SETCADR => set_car [1],
         SETCADDR => set_car [1, 1], SETCADDDR => set_car [1, 1, 1],
         SETCAD4R => set_car [1, 1, 1, 1]);

api! {
    /// Allocate a pairlist cell.
    fn Rf_cons(car: Sexp, cdr: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        let car = id(ctx, car, "Rf_cons")?;
        let cdr = id(ctx, cdr, "Rf_cons")?;
        allocating(ctx, &[car, cdr], |ctx| {
            Ok(Sexp::from_id(ctx.session_mut().cons(car, cdr, SEXP_NIL)?))
        })
    }
    /// Allocate a language call cell.
    fn Rf_lcons(car: Sexp, cdr: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        let car = id(ctx, car, "Rf_lcons")?;
        let cdr = id(ctx, cdr, "Rf_lcons")?;
        allocating(ctx, &[car, cdr], |ctx| {
            Ok(Sexp::from_id(ctx.session_mut().lcons(car, cdr, SEXP_NIL)?))
        })
    }
    /// Allocate a pairlist of empty cells.
    fn Rf_allocList(n: c_int) -> Sexp = Sexp::NULL; |ctx| {
        let n = checked_len(ctx, n as isize)?;
        allocating(ctx, &[], |ctx| alloc_cells(ctx, n, false).map(Sexp::from_id))
    }
    /// Allocate a language call with empty cells.
    fn Rf_allocLang(n: c_int) -> Sexp = Sexp::NULL; |ctx| {
        let n = checked_len(ctx, n as isize)?;
        allocating(ctx, &[], |ctx| alloc_cells(ctx, n, true).map(Sexp::from_id))
    }
    /// Set the tag of a pairlist cell.
    fn SET_TAG(x: Sexp, tag: Sexp) -> () = (); |ctx| {
        let x = id(ctx, x, "SET_TAG")?;
        let tag = id(ctx, tag, "SET_TAG")?;
        Ok(ctx.session_mut().set_tag(x, tag)?)
    }
    /// Follow the specified number of CDR links.
    fn Rf_nthcdr(x: Sexp, n: c_int) -> Sexp = Sexp::NULL; |ctx| {
        let mut x = id(ctx, x, "Rf_nthcdr")?;
        let n = checked_len(ctx, n as isize)?;
        let mut seen = HashSet::new();
        for _ in 0..n {
            if x == SEXP_NIL {
                return Err(ctx.stop(SEXP_NIL, "list is shorter than requested"));
            }
            if !seen.insert(x) {
                return Err(ctx.stop(SEXP_NIL, "cyclic pairlist"));
            }
            x = parts(ctx, x)?.1;
        }
        Ok(Sexp::from_id(x))
    }
    /// Return the CAR of the element at the zero-based position.
    fn Rf_elt(x: Sexp, i: c_int) -> Sexp = Sexp::NULL; |ctx| {
        let mut x = id(ctx, x, "Rf_elt")?;
        let mut seen = HashSet::new();
        for _ in 0..i {
            if x == SEXP_NIL { break; }
            if !seen.insert(x) {
                return Err(ctx.stop(SEXP_NIL, "cyclic pairlist"));
            }
            x = parts(ctx, x)?.1;
        }
        let car = if i < 0 { SEXP_NIL } else { parts(ctx, x)?.0 };
        Ok(Sexp::from_id(car))
    }
    /// Return the last cell of a pairlist.
    fn Rf_lastElt(x: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        let x = id(ctx, x, "Rf_lastElt")?;
        Ok(Sexp::from_id(cells(ctx, x)?.last().map_or(SEXP_NIL, |cell| cell.0)))
    }
    /// Link the last cell of the first pairlist to the second object.
    fn Rf_listAppend(x: Sexp, y: Sexp) -> Sexp = Sexp::NULL; |ctx| {
        let first = id(ctx, x, "Rf_listAppend")?;
        let tail = id(ctx, y, "Rf_listAppend")?;
        if let Some(&(last, _, _)) = cells(ctx, first)?.last() {
            ctx.session_mut().set_cdr(last, tail)?;
            Ok(x)
        } else {
            Ok(y)
        }
    }
}
