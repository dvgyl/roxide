//! Matching, duplicate detection, identity, ordering, and search.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::ffi::c_int;
use std::ptr;
use std::slice;

use roxide_eval::{Ctx, EvalResult};
use roxide_gc::SexpId;
use roxide_value::{
    BASE_ENV, CharId, IdenticalOpts, NA_INTEGER, RComplex, RLogical, RSession, SEXP_NIL, SexpType,
    ValueApi, VecView, identical,
};

use super::pairlist::cells;
use super::random::{draw, get_state, put_state};
use super::string::utf8;
use super::vector::{MAX_VECTOR_BYTES, allocating, api, check_capacity, id};
use crate::abi::{R_xlen_t, Rboolean, Rcomplex, Sexp};

fn duplicates(
    ctx: &mut Ctx,
    x: Sexp,
    incomparables: Sexp,
    fromlast: Rboolean,
    first_only: bool,
) -> EvalResult<SexpId> {
    let x = id(ctx, x, "Rf_duplicated")?;
    let incomparables = id(ctx, incomparables, "Rf_duplicated")?;
    let fromlast = ctx
        .session_mut()
        .scalar_lgl(RLogical::from_option(Some(fromlast != 0)))?;
    let fromlast_arg = ctx.session_mut().intern("fromLast")?;
    let function = if first_only {
        "anyDuplicated.default"
    } else {
        "duplicated.default"
    };
    ctx.call_function(
        function,
        &[
            (None, x),
            (None, incomparables),
            (Some(fromlast_arg), fromlast),
        ],
        BASE_ENV,
    )
}

fn index(session: &RSession, x: SexpId) -> EvalResult<R_xlen_t> {
    Ok(match session.type_of(x)? {
        SexpType::Int => session.int(x)?.get(0) as R_xlen_t,
        _ => session.real(x)?.get(0) as R_xlen_t,
    })
}

fn out_of_order<T: PartialOrd>(values: impl IntoIterator<Item = T>, strict: bool) -> bool {
    let mut values = values.into_iter();
    let Some(mut previous) = values.next() else {
        return false;
    };
    for next in values {
        if previous > next || strict && previous == next {
            return true;
        }
        previous = next;
    }
    false
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum SortChar {
    Text(Vec<u8>),
    Missing,
}

fn unsorted(ctx: &mut Ctx, x: SexpId, strict: bool) -> EvalResult<bool> {
    let session = ctx.session_mut();
    Ok(match session.type_of(x)? {
        SexpType::Lgl => out_of_order(session.lgl(x)?.iter().map(RLogical::raw), strict),
        SexpType::Int => out_of_order(session.int(x)?.iter(), strict),
        SexpType::Real => out_of_order(session.real(x)?.iter(), strict),
        SexpType::Cplx => out_of_order(session.cplx(x)?.iter().map(|v| (v.re, v.im)), strict),
        SexpType::Raw => out_of_order(session.raw(x)?.iter(), strict),
        SexpType::Str => {
            let mut previous = None;
            for i in 0..session.xlength(x)? {
                let ch = session.str_get(x, i)?;
                let next = if ch == CharId::NA {
                    SortChar::Missing
                } else {
                    SortChar::Text(
                        utf8(session.char_bytes(ch)?, session.char_encoding(ch)?).into_owned(),
                    )
                };
                if previous
                    .as_ref()
                    .is_some_and(|previous| previous > &next || strict && previous == &next)
                {
                    return Ok(true);
                }
                previous = Some(next);
            }
            false
        }
        _ => return Err(ctx.stop(SEXP_NIL, "only atomic vectors can be tested to be sorted")),
    })
}

api! {
    /// Return the index of the first duplicate, or zero.
    fn Rf_any_duplicated(x: Sexp, fromlast: Rboolean) -> R_xlen_t = 0; |ctx| {
        let answer = duplicates(ctx, x, Sexp::from_id(SEXP_NIL), fromlast, true)?;
        index(ctx.session(), answer)
    }

    /// Return the first duplicate index after excluding incomparable values.
    fn Rf_any_duplicated3(x: Sexp, incomparables: Sexp, fromlast: Rboolean) -> R_xlen_t = 0; |ctx| {
        let answer = duplicates(ctx, x, incomparables, fromlast, true)?;
        index(ctx.session(), answer)
    }

    /// Return one logical value per input element to identify duplicates.
    fn Rf_duplicated(x: Sexp, fromlast: Rboolean) -> Sexp = Sexp::NULL; |ctx| {
        duplicates(ctx, x, Sexp::from_id(SEXP_NIL), fromlast, false).map(Sexp::from_id)
    }

    /// Return the first position in the table for each input element.
    fn Rf_match(table: Sexp, x: Sexp, nomatch: c_int) -> Sexp = Sexp::NULL; |ctx| {
        let table = id(ctx, table, "Rf_match")?;
        let x = id(ctx, x, "Rf_match")?;
        let nomatch = ctx.session_mut().scalar_int(nomatch)?;
        ctx.call_function("match", &[(None, x), (None, table), (None, nomatch)], BASE_ENV)
            .map(Sexp::from_id)
    }

    /// Compare two R objects with the selected identity flags.
    fn R_compute_identical(x: Sexp, y: Sexp, flags: c_int) -> c_int = 0; |ctx| {
        let x = id(ctx, x, "R_compute_identical")?;
        let y = id(ctx, y, "R_compute_identical")?;
        let options = IdenticalOpts {
            num_eq: flags & 1 == 0,
            single_na: flags & 2 == 0,
            attrib_as_set: flags & 4 == 0,
            ignore_bytecode: flags & 8 == 0,
            ignore_environment: flags & 16 == 0,
            ignore_srcref: flags & 32 == 0,
        };
        Ok(c_int::from(identical(ctx.session(), x, y, options)?))
    }

    /// Test whether comparable values occur out of ascending order.
    fn Rf_isUnsorted(x: Sexp, strictly: Rboolean) -> Rboolean = 0; |ctx| {
        let x = id(ctx, x, "Rf_isUnsorted")?;
        allocating(ctx, &[x], |ctx| Ok(c_int::from(unsorted(ctx, x, strictly != 0)?)))
    }
}

fn compare_int(a: c_int, b: c_int) -> Ordering {
    (a == NA_INTEGER).cmp(&(b == NA_INTEGER)).then(a.cmp(&b))
}

fn compare_real(a: f64, b: f64) -> Ordering {
    a.partial_cmp(&b)
        .unwrap_or_else(|| a.is_nan().cmp(&b.is_nan()))
}

fn compare_complex(a: Rcomplex, b: Rcomplex) -> Ordering {
    if a.r == b.r {
        compare_real(a.i, b.i)
    } else {
        compare_real(a.r, b.r)
    }
}

#[derive(Clone, Copy)]
struct OrderFlags {
    na_last: bool,
    decreasing: bool,
}

impl OrderFlags {
    fn new(na_last: Rboolean, decreasing: Rboolean) -> Self {
        Self {
            na_last: na_last != 0,
            decreasing: decreasing == 1,
        }
    }

    fn compare<T>(
        self,
        a: Option<T>,
        b: Option<T>,
        compare: impl FnOnce(T, T) -> Ordering,
    ) -> Ordering {
        match (a, b) {
            (Some(a), Some(b)) if self.decreasing => compare(b, a),
            (Some(a), Some(b)) => compare(a, b),
            (None, None) => Ordering::Equal,
            (None, Some(_)) if self.na_last => Ordering::Greater,
            (Some(_), None) if self.na_last => Ordering::Less,
            (None, Some(_)) => Ordering::Less,
            (Some(_), None) => Ordering::Greater,
        }
    }
}

enum OrderColumn<'a> {
    Integer(VecView<'a, c_int>),
    Logical(VecView<'a, RLogical>),
    Real(VecView<'a, f64>),
    Complex(VecView<'a, RComplex>),
    Text(Vec<Option<Cow<'a, [u8]>>>),
}

impl<'a> OrderColumn<'a> {
    fn new(session: &'a RSession, x: SexpId, n: usize) -> EvalResult<Option<Self>> {
        Ok(match session.type_of(x)? {
            SexpType::Int => Some(Self::Integer(session.int(x)?)),
            SexpType::Lgl => Some(Self::Logical(session.lgl(x)?)),
            SexpType::Real => Some(Self::Real(session.real(x)?)),
            SexpType::Cplx => Some(Self::Complex(session.cplx(x)?)),
            SexpType::Str => {
                let text = session
                    .str_slice(x)?
                    .iter()
                    .take(n)
                    .map(|&ch| {
                        if ch == CharId::NA {
                            return Ok(None);
                        }
                        Ok(Some(utf8(
                            session.char_bytes(ch)?,
                            session.char_encoding(ch)?,
                        )))
                    })
                    .collect::<EvalResult<Vec<_>>>()?;
                Some(Self::Text(text))
            }
            _ => None,
        })
    }

    fn compare(&self, i: usize, j: usize, flags: OrderFlags) -> Ordering {
        match self {
            Self::Integer(v) => {
                let at = |i| {
                    let x = v.get(i);
                    (x != NA_INTEGER).then_some(x)
                };
                flags.compare(at(i), at(j), |a, b| a.cmp(&b))
            }
            Self::Logical(v) => flags.compare(v.get(i).get(), v.get(j).get(), |a, b| a.cmp(&b)),
            Self::Real(v) => {
                let at = |i| {
                    let x = v.get(i);
                    (!x.is_nan()).then_some(x)
                };
                flags.compare(at(i), at(j), compare_real)
            }
            Self::Complex(v) => {
                let at = |i| {
                    let x = v.get(i);
                    (!x.re.is_nan() && !x.im.is_nan()).then_some(Rcomplex { r: x.re, i: x.im })
                };
                flags.compare(at(i), at(j), compare_complex)
            }
            Self::Text(v) => flags.compare(v[i].as_deref(), v[j].as_deref(), <[u8]>::cmp),
        }
    }
}

fn order_rows(
    ctx: &mut Ctx,
    n: c_int,
    inputs: &[SexpId],
    flags: OrderFlags,
) -> EvalResult<Vec<c_int>> {
    if n == 1 {
        return Ok(vec![0]);
    }
    if n < 0 {
        return Err(ctx.stop(SEXP_NIL, "invalid order vector length"));
    }
    let len = n as usize;
    check_capacity(ctx, len, size_of::<c_int>())?;
    let mut index = Vec::new();
    index
        .try_reserve_exact(len)
        .map_err(|_| ctx.stop(SEXP_NIL, "cannot allocate order indexes"))?;
    allocating(ctx, inputs, |ctx| {
        for &x in inputs {
            if ctx.session().xlength(x)? < len {
                return Err(ctx.stop(
                    SEXP_NIL,
                    "order vector is shorter than the requested length",
                ));
            }
            if ctx.session().type_of(x)? == SexpType::Str {
                check_capacity(ctx, len, size_of::<Option<Cow<'_, [u8]>>>())?;
                ctx.session_mut().str_dataptr_ro(x)?;
            }
        }
        Ok(())
    })?;
    let session = ctx.session();
    let mut columns = Vec::new();
    for &x in inputs {
        let Some(column) = OrderColumn::new(session, x, len)? else {
            let name = session
                .type_of(x)
                .map_or("unknown", |ty| ty.type_name(false));
            drop(columns);
            return Err(ctx.stop(
                SEXP_NIL,
                format!("unimplemented type '{name}' in 'orderVector1'"),
            ));
        };
        columns.push(column);
    }
    index.extend(0..n);
    index.sort_unstable_by(|&i, &j| {
        for column in &columns {
            let cmp = column.compare(i as usize, j as usize, flags);
            if cmp != Ordering::Equal {
                return cmp;
            }
        }
        i.cmp(&j)
    });
    Ok(index)
}

api! {
    /// Write zero-based row indexes in order of the supplied columns.
    fn R_orderVector(index: *mut c_int, n: c_int, args: Sexp, na_last: Rboolean, decreasing: Rboolean) -> () = (); |ctx| {
        if n == 0 {
            return Ok(());
        }
        if index.is_null() {
            return Err(ctx.stop(SEXP_NIL, "missing order index buffer"));
        }
        let args = id(ctx, args, "R_orderVector")?;
        let columns: Vec<_> = cells(ctx, args)?.into_iter().map(|(_, car, _)| car).collect();
        let result = order_rows(ctx, n, &columns, OrderFlags::new(na_last, decreasing))?;
        // SAFETY: C supplies independent storage for n indexes.
        ptr::copy_nonoverlapping(result.as_ptr(), index, result.len());
        Ok(())
    }

    /// Write zero-based row indexes in order of the supplied vector.
    fn R_orderVector1(index: *mut c_int, n: c_int, x: Sexp, na_last: Rboolean, decreasing: Rboolean) -> () = (); |ctx| {
        if n == 0 {
            return Ok(());
        }
        if index.is_null() {
            return Err(ctx.stop(SEXP_NIL, "missing order index buffer"));
        }
        let x = id(ctx, x, "R_orderVector1")?;
        let result = order_rows(ctx, n, &[x], OrderFlags::new(na_last, decreasing))?;
        // SAFETY: C supplies independent storage for n indexes.
        ptr::copy_nonoverlapping(result.as_ptr(), index, result.len());
        Ok(())
    }
}

unsafe fn sort_buffer<'a, T>(x: *mut T, first: usize, last: usize) -> Option<&'a mut [T]> {
    if x.is_null()
        || !x.is_aligned()
        || first == 0
        || first > last
        || last > isize::MAX as usize / size_of::<T>()
    {
        return None;
    }
    // SAFETY: C supplies an initialized buffer of at least last elements.
    Some(unsafe { slice::from_raw_parts_mut(x.add(first - 1), last - first + 1) })
}

macro_rules! sorts {
    ($($sort:ident, $part:ident, $ty:ty, $compare:ident;)+) => {$(
        /// Sort the buffer in ascending order with missing values last.
        ///
        /// # Safety
        /// Supply an initialized buffer of n elements.
        #[allow(non_snake_case)]
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $sort(x: *mut $ty, n: c_int) {
            // SAFETY: The caller supplies the sort buffer.
            if let Some(x) = unsafe { sort_buffer(x, 1, n.max(0) as usize) } {
                x.sort_unstable_by(|&a, &b| $compare(a, b));
            }
        }

        /// Put the element at zero-based index k in its sorted position.
        ///
        /// # Safety
        /// Supply an initialized buffer of n elements.
        #[allow(non_snake_case)]
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $part(x: *mut $ty, n: c_int, k: c_int) {
            if !(0..n).contains(&k) {
                return;
            }
            // SAFETY: The caller supplies the sort buffer.
            if let Some(x) = unsafe { sort_buffer(x, 1, n as usize) } {
                x.select_nth_unstable_by(k as usize, |&a, &b| $compare(a, b));
            }
        }
    )+};
}

sorts! {
    R_isort, Rf_iPsort, c_int, compare_int;
    R_rsort, Rf_rPsort, f64, compare_real;
    R_csort, Rf_cPsort, Rcomplex, compare_complex;
}

fn sort_indexed<T: Copy>(
    values: &mut [T],
    indexes: &mut [c_int],
    compare: impl Fn(T, T) -> Ordering,
) {
    let len = values.len();
    let mut pairs = Vec::new();
    if len > MAX_VECTOR_BYTES / size_of::<(T, c_int)>() || pairs.try_reserve_exact(len).is_err() {
        return;
    }
    pairs.extend(values.iter().copied().zip(indexes.iter().copied()));
    pairs.sort_unstable_by(|&(a, _), &(b, _)| compare(a, b));
    for ((value, index), (sorted_value, sorted_index)) in values.iter_mut().zip(indexes).zip(pairs)
    {
        *value = sorted_value;
        *index = sorted_index;
    }
}

macro_rules! quicksorts {
    ($($sort:ident, $indexed:ident, $ty:ty, $compare:expr;)+) => {$(
        /// Sort a range with inclusive, one-based endpoints.
        ///
        /// # Safety
        /// Supply an initialized buffer of at least last elements.
        #[allow(non_snake_case)]
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $sort(x: *mut $ty, first: usize, last: usize) {
            // SAFETY: The caller supplies the sort buffer.
            if let Some(x) = unsafe { sort_buffer(x, first, last) } {
                let compare = $compare;
                x.sort_unstable_by(|&a, &b| compare(a, b));
            }
        }

        /// Sort a range and apply the same permutation to the index buffer.
        ///
        /// # Safety
        /// Supply distinct initialized buffers of at least last elements.
        #[allow(non_snake_case)]
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $indexed(x: *mut $ty, index: *mut c_int, first: c_int, last: c_int) {
            if first <= 0 || last < first {
                return;
            }
            // SAFETY: The caller supplies separate sort and index buffers.
            if let (Some(x), Some(index)) = unsafe {
                (sort_buffer(x, first as usize, last as usize),
                 sort_buffer(index, first as usize, last as usize))
            } {
                sort_indexed(x, index, $compare);
            }
        }
    )+};
}

quicksorts! {
    R_qsort, R_qsort_I, f64, compare_real;
    R_qsort_int, R_qsort_int_I, c_int, |a: c_int, b| a.cmp(&b);
}

/// Sort values and permute their indexes, with missing values last.
///
/// # Safety
/// Supply distinct initialized buffers of n elements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsort_with_index(x: *mut f64, index: *mut c_int, n: c_int) {
    // SAFETY: The caller supplies separate sort and index buffers.
    if let (Some(x), Some(index)) = unsafe {
        (
            sort_buffer(x, 1, n.max(0) as usize),
            sort_buffer(index, 1, n.max(0) as usize),
        )
    } {
        sort_indexed(x, index, compare_real);
    }
}

/// Sort values in descending order and permute their indexes.
///
/// # Safety
/// Supply distinct initialized buffers of n elements.
#[allow(non_snake_case)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn Rf_revsort(x: *mut f64, index: *mut c_int, n: c_int) {
    // SAFETY: The caller supplies separate sort and index buffers.
    if let (Some(x), Some(index)) = unsafe {
        (
            sort_buffer(x, 1, n.max(0) as usize),
            sort_buffer(index, 1, n.max(0) as usize),
        )
    } {
        sort_indexed(x, index, |a, b| compare_real(b, a));
    }
}

/// Sort a range with the Fortran calling convention.
///
/// # Safety
/// Supply readable endpoints and an initialized buffer of at least last elements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qsort3_(x: *mut f64, first: *mut c_int, last: *mut c_int) {
    if !first.is_null() && !last.is_null() {
        // SAFETY: C supplies readable endpoints and the sort buffer.
        unsafe { R_qsort(x, (*first).max(0) as usize, (*last).max(0) as usize) };
    }
}

/// Sort values and permute their indexes with the Fortran calling convention.
///
/// # Safety
/// Supply readable endpoints and distinct initialized buffers of at least last elements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qsort4_(
    x: *mut f64,
    index: *mut c_int,
    first: *mut c_int,
    last: *mut c_int,
) {
    if !first.is_null() && !last.is_null() {
        // SAFETY: C supplies readable endpoints and separate sort and index buffers.
        unsafe { R_qsort_I(x, index, *first, *last) };
    }
}

fn interval(
    points: &[f64],
    x: f64,
    rightmost_closed: bool,
    all_inside: bool,
    left_open: bool,
    guess: usize,
) -> (c_int, c_int) {
    let Some((&first, &last)) = points.first().zip(points.last()) else {
        return (0, 0);
    };
    if x.is_nan() {
        return (0, 0);
    }
    let n = points.len();
    let precedes = |point: &f64| if left_open { *point < x } else { *point <= x };
    if !precedes(&first) {
        return (
            c_int::from(all_inside || (left_open && rightmost_closed && x == first)),
            -1,
        );
    }
    if precedes(&last) {
        let closed = !left_open && rightmost_closed && x == last;
        return ((n - usize::from(all_inside || closed)) as c_int, 1);
    }
    let guess = guess.clamp(1, n - 1);
    if precedes(&points[guess - 1]) {
        if !precedes(&points[guess]) {
            (guess as c_int, 0)
        } else {
            let offset = points[guess + 1..].partition_point(precedes);
            ((guess + 1 + offset) as c_int, 0)
        }
    } else {
        (points[..guess - 1].partition_point(precedes) as c_int, 0)
    }
}

/// Find the interval for a value in an ordered buffer.
///
/// # Safety
/// Supply n readable points and writable storage for the boundary flag.
#[allow(non_snake_case)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn findInterval(
    points: *mut f64,
    n: c_int,
    x: f64,
    rightmost_closed: Rboolean,
    all_inside: Rboolean,
    guess: c_int,
    flag: *mut c_int,
) -> c_int {
    // SAFETY: The caller supplies the points and flag.
    unsafe { findInterval2(points, n, x, rightmost_closed, all_inside, 0, guess, flag) }
}

/// Find the interval with the selected endpoint rules.
///
/// # Safety
/// Supply n readable points and writable storage for the boundary flag.
#[allow(non_snake_case)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn findInterval2(
    points: *mut f64,
    n: c_int,
    x: f64,
    rightmost_closed: Rboolean,
    all_inside: Rboolean,
    left_open: Rboolean,
    guess: c_int,
    flag: *mut c_int,
) -> c_int {
    let points = if n > 0 && !points.is_null() {
        // SAFETY: C supplies n initialized points.
        unsafe { slice::from_raw_parts(points, n as usize) }
    } else {
        &[]
    };
    let (index, boundary) = interval(
        points,
        x,
        rightmost_closed != 0,
        all_inside != 0,
        left_open != 0,
        guess.max(0) as usize,
    );
    if !flag.is_null() {
        // SAFETY: C supplies storage for the output flag.
        unsafe { flag.write(boundary) };
    }
    index
}

/// Find the interval with the Fortran calling convention.
///
/// # Safety
/// Supply readable input parameters, n readable points, and writable flag storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn interv_(
    points: *mut f64,
    n: *mut c_int,
    x: *mut f64,
    rightmost_closed: *mut c_int,
    all_inside: *mut c_int,
    guess: *mut c_int,
    flag: *mut c_int,
) -> c_int {
    if n.is_null()
        || x.is_null()
        || rightmost_closed.is_null()
        || all_inside.is_null()
        || guess.is_null()
    {
        return 0;
    }
    // SAFETY: C supplies the search parameters and the output flag.
    unsafe { findInterval(points, *n, *x, *rightmost_closed, *all_inside, *guess, flag) }
}

/// Return a double precision machine constant for Fortran code.
///
/// # Safety
/// Supply a readable index.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn d1mach_(index: *const c_int) -> f64 {
    // SAFETY: The caller supplies a valid index pointer or null.
    match unsafe { index.as_ref() }.copied() {
        Some(1) => f64::MIN_POSITIVE,
        Some(2) => f64::MAX,
        Some(3) => f64::EPSILON / f64::from(f64::RADIX),
        Some(4) => f64::EPSILON,
        Some(5) => std::f64::consts::LOG10_2,
        _ => 0.0,
    }
}

/// Return an integer machine constant for Fortran code.
///
/// # Safety
/// Supply a readable index.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn i1mach_(index: *const c_int) -> c_int {
    // SAFETY: The caller supplies a valid index pointer or null.
    match unsafe { index.as_ref() }.copied() {
        Some(1) => 5,
        Some(2) => 6,
        Some(5) => c_int::BITS as c_int,
        Some(6) => size_of::<c_int>() as c_int,
        Some(7) => 2,
        Some(8) => c_int::BITS as c_int - 1,
        Some(9) => c_int::MAX,
        Some(10) => f32::RADIX as c_int,
        Some(11) => f32::MANTISSA_DIGITS as c_int,
        Some(12) => f32::MIN_EXP,
        Some(13) => f32::MAX_EXP,
        Some(14) => f64::MANTISSA_DIGITS as c_int,
        Some(15) => f64::MIN_EXP,
        Some(16) => f64::MAX_EXP,
        _ => 0,
    }
}

/// Check for a user interrupt from Fortran code.
///
/// # Safety
/// Call from foreign code inside an active trampoline.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rchkusr_() {
    // SAFETY: The caller supplies the foreign trampoline.
    unsafe { super::error::R_CheckUserInterrupt() };
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MaxTies {
    Random,
    First,
    Last,
}

fn row_maximum(
    mut row: impl Iterator<Item = f64> + Clone,
    ties: MaxTies,
    mut random: impl FnMut() -> EvalResult<f64>,
) -> EvalResult<c_int> {
    let Some(mut maximum) = row.next() else {
        return Ok(NA_INTEGER);
    };
    if maximum.is_nan() || row.clone().any(f64::is_nan) {
        return Ok(NA_INTEGER);
    }
    let tolerance = if ties == MaxTies::Random {
        row.clone()
            .chain([maximum])
            .filter(|value| value.is_finite())
            .map(f64::abs)
            .fold(0.0, f64::max)
            * 1e-5
    } else {
        0.0
    };
    let mut selected = 1;
    let mut count = 1;
    for (offset, value) in row.enumerate() {
        let column = offset as c_int + 2;
        let difference = value - maximum;
        if difference > tolerance {
            maximum = value;
            selected = column;
            count = 1;
        } else if value == maximum || difference.abs() <= tolerance {
            match ties {
                MaxTies::Random => {
                    count += 1;
                    if random()? < 1.0 / f64::from(count) {
                        selected = column;
                    }
                }
                MaxTies::Last => selected = column,
                MaxTies::First => {}
            }
        }
    }
    Ok(selected)
}

api! {
    /// Write the one-based maximum column index for each matrix row.
    fn R_max_col(matrix: *mut f64, rows: *mut c_int, columns: *mut c_int, maxima: *mut c_int, ties: *mut c_int) -> () = (); |ctx| {
        if rows.is_null() || columns.is_null() || ties.is_null() {
            return Err(ctx.stop(SEXP_NIL, "missing maximum column parameters"));
        }
        let (rows, columns) = (*rows, *columns);
        if rows == 0 {
            return Ok(());
        }
        if rows < 0 || columns < 0 || maxima.is_null() || (columns > 0 && matrix.is_null()) {
            return Err(ctx.stop(SEXP_NIL, "invalid maximum column buffers"));
        }
        let ties = match *ties {
            1 => MaxTies::Random,
            2 => MaxTies::First,
            3 => MaxTies::Last,
            _ => return Err(ctx.stop(SEXP_NIL, "invalid maximum column tie method")),
        };
        let rows = rows as usize;
        let count = rows * columns as usize;
        if count > isize::MAX as usize / size_of::<f64>() {
            return Err(ctx.stop(SEXP_NIL, "invalid maximum column matrix size"));
        }
        let matrix = if count == 0 {
            &[]
        } else {
            // SAFETY: C supplies a column-major matrix with count initialized elements.
            slice::from_raw_parts(matrix, count)
        };
        let mut used_rng = false;
        for i in 0..rows {
            let row = matrix.iter().skip(i).step_by(rows).copied();
            let index = row_maximum(row, ties, || {
                if !used_rng {
                    get_state(ctx)?;
                    used_rng = true;
                }
                draw(ctx, |rng| rng.unif_rand())
            })?;
            // SAFETY: C supplies independent output storage for one index per row.
            maxima.add(i).write(index);
        }
        if used_rng {
            put_state(ctx)?;
        }
        Ok(())
    }
}
