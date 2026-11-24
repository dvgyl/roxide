//! Foreign ALTREP allocation, access, payload transfer, and collection tests.

use roxide_gc::{GcPolicy, RootSet, SexpId, Tracer};
use roxide_value::{
    AltNeed, AltSlot, CharId, Encoding, ForeignAltInfo, RComplex, RLogical, RSession, SEXP_NIL,
    SYM_BASE, SexpType, Sortedness, ValueApi, ValueError,
};

struct EmptyRoots;

impl RootSet for EmptyRoots {
    fn trace_roots(&self, _: &mut Tracer<'_>) {}
}

fn session() -> RSession {
    RSession::new(GcPolicy::Torture).unwrap()
}

fn class(session: &mut RSession) -> SexpId {
    let name = session.intern("foreign_vector").unwrap();
    let package = session.intern("test_package").unwrap();
    session.alloc_alt_class(name, package, 0).unwrap()
}

fn assert_need<T>(result: Result<T, ValueError>, id: SexpId, need: AltNeed) {
    assert_eq!(result.err(), Some(ValueError::ForeignAltrep { id, need }));
}

#[test]
fn class_descriptors_store_symbols_and_encode_the_index() {
    let mut s = session();
    let name = s.intern("foreign_vector").unwrap();
    let package = s.intern("test_package").unwrap();
    let descriptor = s.alloc_alt_class(name, package, 12).unwrap();
    assert_eq!(s.type_of(descriptor).unwrap(), SexpType::ExtPtr);
    assert_eq!(s.extptr_addr(descriptor).unwrap(), 13);
    assert_eq!(s.extptr_tag(descriptor).unwrap(), name.as_sexp());
    assert_eq!(s.extptr_prot(descriptor).unwrap(), package.as_sexp());
    assert_eq!(
        s.alt_class_names(descriptor).unwrap(),
        Some((name, package))
    );
    assert_eq!(s.alt_class_index(descriptor).unwrap(), Some(12));
    assert!(s.alloc_alt_class(name, package, usize::MAX).is_err());

    let extptr = s
        .alloc_extptr(13, name.as_sexp(), package.as_sexp())
        .unwrap();
    assert_eq!(s.alt_class_index(extptr).unwrap(), None);
    assert_eq!(s.alt_class_names(extptr).unwrap(), None);
    assert_eq!(s.alt_class_index(SEXP_NIL).unwrap(), None);
    s.set_extptr_addr(descriptor, 0).unwrap();
    assert_eq!(s.alt_class_index(descriptor).unwrap(), None);
}

#[test]
fn foreign_vectors_move_the_dense_buffer_for_all_seven_types() {
    let mut s = session();
    let class = class(&mut s);
    let names = s.alt_class_names(class).unwrap();
    assert!(!s.foreign_alt_seen());
    assert!(
        s.alloc_foreign_alt(class, SexpType::Expr, SEXP_NIL, SEXP_NIL)
            .is_err()
    );
    assert!(
        s.alloc_foreign_alt(SEXP_NIL, SexpType::Int, SEXP_NIL, SEXP_NIL)
            .is_err()
    );
    assert!(!s.foreign_alt_seen());

    let z = RComplex { re: 3.0, im: 4.0 };
    let ch = s.mk_char(b"text", Encoding::Native).unwrap();
    let child = s.scalar_int(42).unwrap();
    let list = s.alloc_list(1).unwrap();
    s.list_set(list, 0, child).unwrap();
    let variants = [
        (SexpType::Lgl, s.alloc_lgl_from(&[RLogical::TRUE]).unwrap()),
        (SexpType::Int, s.alloc_int_from(&[9]).unwrap()),
        (SexpType::Real, s.alloc_real_from(&[2.5]).unwrap()),
        (SexpType::Cplx, s.alloc_cplx_from(&[z]).unwrap()),
        (SexpType::Raw, s.alloc_raw_from(&[0xff]).unwrap()),
        (SexpType::Str, s.alloc_str_from(&[ch]).unwrap()),
        (SexpType::Vec, list),
    ];
    for (ty, dense) in variants {
        let x = s.alloc_foreign_alt(class, ty, SEXP_NIL, SEXP_NIL).unwrap();
        assert!(s.foreign_alt_seen());
        assert!(s.is_altrep(x).unwrap());
        assert_eq!(s.type_of(x).unwrap(), ty);
        assert_eq!(s.alt_class_names(x).unwrap(), names);
        assert_eq!(s.builtin_alt_class(x).unwrap(), None);
        assert_eq!(
            s.foreign_alt(x).unwrap(),
            Some(ForeignAltInfo {
                class,
                ty,
                data1: SEXP_NIL,
                data2: SEXP_NIL,
                len: None,
                owned: false,
            })
        );
        assert_need(s.xlength(x), x, AltNeed::Length);
        s.seal_foreign_alt_len(x, 2).unwrap();
        s.seal_foreign_alt_len(x, 3).unwrap();
        assert_eq!(s.xlength(x).unwrap(), 2);
        assert_need(s.data_ptr_ro(x), x, AltNeed::Payload);
        assert_need(s.data_ptr_rw(x), x, AltNeed::Payload);
        assert_need(s.shallow_duplicate(x), x, AltNeed::Payload);
        assert_eq!(s.alt_is_sorted(x).unwrap(), Sortedness::Unknown);
        assert!(!s.alt_no_na(x).unwrap());
        match ty {
            SexpType::Lgl => assert_need(s.lgl(x), x, AltNeed::Payload),
            SexpType::Int => assert_need(s.int(x), x, AltNeed::Payload),
            SexpType::Real => assert_need(s.real(x), x, AltNeed::Payload),
            SexpType::Cplx => assert_need(s.cplx(x), x, AltNeed::Payload),
            SexpType::Raw => assert_need(s.raw(x), x, AltNeed::Payload),
            SexpType::Str => {
                assert_need(s.str_slice(x), x, AltNeed::Payload);
                assert_need(s.str_get(x, 0), x, AltNeed::Payload);
                assert_need(s.str_set(x, 0, ch), x, AltNeed::Payload);
                assert_need(s.str_dataptr_ro(x), x, AltNeed::Payload);
            }
            SexpType::Vec => {
                assert_need(s.list_slice(x), x, AltNeed::Payload);
                assert_need(s.list_get(x, 0), x, AltNeed::Payload);
                assert_need(s.list_set(x, 0, child), x, AltNeed::Payload);
            }
            _ => unreachable!(),
        }

        let before = s.data_ptr_ro(dense).unwrap();
        let heap_bytes = s.heap.live_bytes();
        assert!(s.adopt_foreign_alt_payload(x, dense).unwrap());
        assert_eq!(s.heap.live_bytes(), heap_bytes);
        assert_eq!(s.type_of(dense).unwrap(), ty);
        assert_eq!(s.xlength(dense).unwrap(), 0);
        assert!(s.is_altrep(x).unwrap());
        assert_eq!(s.type_of(x).unwrap(), ty);
        assert_eq!(s.xlength(x).unwrap(), 1);
        assert_eq!(s.foreign_alt(x).unwrap().unwrap().len, Some(1));
        assert!(s.foreign_alt(x).unwrap().unwrap().owned);
        assert_eq!(s.data_ptr_ro(x).unwrap(), before);
        assert_eq!(s.data_ptr_rw(x).unwrap(), before);
        assert_eq!(s.alt_is_sorted(x).unwrap(), Sortedness::Unknown);
        assert!(!s.alt_no_na(x).unwrap());
        assert!(!s.adopt_foreign_alt_payload(x, SEXP_NIL).unwrap());
        assert_eq!(s.data_ptr_ro(x).unwrap(), before);
        let copy = s.shallow_duplicate(x).unwrap();
        assert_eq!(s.type_of(copy).unwrap(), ty);
        assert!(!s.is_altrep(copy).unwrap());
        assert_ne!(s.data_ptr_ro(copy).unwrap(), before);
        for value in [x, copy] {
            match ty {
                SexpType::Lgl => assert_eq!(
                    s.lgl(value).unwrap().as_slice(),
                    Some(&[RLogical::TRUE][..])
                ),
                SexpType::Int => assert_eq!(s.int(value).unwrap().as_slice(), Some(&[9][..])),
                SexpType::Real => assert_eq!(s.real(value).unwrap().as_slice(), Some(&[2.5][..])),
                SexpType::Cplx => {
                    let actual = s.cplx(value).unwrap().get(0);
                    assert_eq!((actual.re, actual.im), (z.re, z.im));
                }
                SexpType::Raw => assert_eq!(s.raw(value).unwrap().as_slice(), Some(&[0xff][..])),
                SexpType::Str => {
                    assert_eq!(s.str_slice(value).unwrap(), &[ch]);
                    assert_eq!(s.str_dataptr_ro(value).unwrap(), &[ch]);
                    assert_eq!(s.str_get(value, 0).unwrap(), ch);
                }
                SexpType::Vec => {
                    assert_eq!(s.list_slice(value).unwrap(), &[child]);
                    assert_eq!(s.list_get(value, 0).unwrap(), child);
                }
                _ => unreachable!(),
            }
        }
    }
}

#[test]
fn adoption_rejects_wrong_type_and_altrep_sources_without_mutation() {
    let mut s = session();
    let class = class(&mut s);
    let x = s
        .alloc_foreign_alt(class, SexpType::Int, SEXP_NIL, SEXP_NIL)
        .unwrap();
    let real = s.scalar_real(1.0).unwrap();
    let alt = s.alloc_compact_intseq(1, 3, 1).unwrap();
    let initial = s.foreign_alt(x).unwrap();
    for source in [real, alt, x] {
        assert!(s.adopt_foreign_alt_payload(x, source).is_err());
        assert_eq!(s.foreign_alt(x).unwrap(), initial);
    }
    assert_eq!(s.real(real).unwrap().get(0), 1.0);
    assert_eq!(s.xlength(alt).unwrap(), 3);
    let empty = s.alloc_int_from(&[]).unwrap();
    assert!(s.adopt_foreign_alt_payload(x, empty).unwrap());
    assert_eq!(s.xlength(x).unwrap(), 0);
}

#[test]
fn foreign_slots_replace_edges_and_allow_self_assignment() {
    let mut s = session();
    let class = class(&mut s);
    let first = s.scalar_int(1).unwrap();
    let second = s.scalar_int(2).unwrap();
    let x = s
        .alloc_foreign_alt(class, SexpType::Int, first, second)
        .unwrap();
    assert_eq!(s.heap.refcnt(class), 1);
    assert_eq!(s.heap.refcnt(first), 1);
    assert_eq!(s.heap.refcnt(second), 1);
    s.set_foreign_alt_data(x, AltSlot::Data1, first).unwrap();
    assert_eq!(s.heap.refcnt(first), 1);
    s.set_foreign_alt_data(x, AltSlot::Data1, second).unwrap();
    assert_eq!(s.heap.refcnt(first), 0);
    assert_eq!(s.heap.refcnt(second), 2);
    s.set_foreign_alt_data(x, AltSlot::Data2, x).unwrap();
    assert_eq!(s.heap.refcnt(second), 1);
    assert_eq!(s.heap.refcnt(x), 1);
    let info = s.foreign_alt(x).unwrap().unwrap();
    assert_eq!((info.data1, info.data2), (second, x));
}

#[test]
fn deep_duplicate_shares_lazy_foreign_children() {
    let mut s = session();
    let class = class(&mut s);
    let x = s
        .alloc_foreign_alt(class, SexpType::Int, SEXP_NIL, SEXP_NIL)
        .unwrap();
    let list = s.alloc_list(2).unwrap();
    s.list_set(list, 0, x).unwrap();
    s.list_set(list, 1, x).unwrap();
    let copy = s.deep_duplicate(list).unwrap();
    assert_ne!(copy, list);
    assert_eq!(s.list_slice(copy).unwrap(), &[x, x]);
    assert_eq!(s.heap.refcnt(x), 4);
    assert_need(s.int_mut(x), x, AltNeed::Payload);
    assert_need(s.deep_duplicate(x), x, AltNeed::Payload);
    assert_need(s.alloc_alt_wrapper(x), x, AltNeed::Payload);
}

#[test]
fn torture_gc_traces_foreign_slots_and_list_elements_and_releases_their_counts() {
    let mut s = session();
    let class = class(&mut s);
    let first = s.scalar_int(1).unwrap();
    let second = s.scalar_int(2).unwrap();
    let x = s
        .alloc_foreign_alt(class, SexpType::Vec, first, second)
        .unwrap();
    let mark = s.heap.protect_mark();
    s.heap.protect(x);
    assert!(s.heap.wants_collection());
    s.collect(&EmptyRoots);
    assert_eq!(s.alt_class_index(class).unwrap(), Some(0));
    assert_eq!(s.int(first).unwrap().get(0), 1);
    assert_eq!(s.int(second).unwrap().get(0), 2);

    let child = s.scalar_int(3).unwrap();
    let dense = s.alloc_list(1).unwrap();
    s.list_set(dense, 0, child).unwrap();
    assert!(s.adopt_foreign_alt_payload(x, dense).unwrap());
    let replacement = s.scalar_int(4).unwrap();
    s.list_set(x, 0, replacement).unwrap();
    assert_eq!(s.heap.refcnt(child), 0);
    assert_eq!(s.heap.refcnt(replacement), 1);
    s.collect(&EmptyRoots);
    assert!(s.type_of(dense).is_err());
    assert!(s.type_of(child).is_err());
    assert_eq!(s.alt_class_index(class).unwrap(), Some(0));
    assert_eq!(s.int(first).unwrap().get(0), 1);
    assert_eq!(s.int(second).unwrap().get(0), 2);
    assert_eq!(s.list_slice(x).unwrap(), &[replacement]);
    assert_eq!(s.int(replacement).unwrap().get(0), 4);

    s.heap.unprotect_to(mark);
    for id in [class, first, second, replacement] {
        s.heap.protect(id);
    }
    s.collect(&EmptyRoots);
    assert!(s.type_of(x).is_err());
    assert!(s.foreign_alt_seen());
    for id in [class, first, second, replacement] {
        assert_eq!(s.heap.refcnt(id), 0);
    }
}

#[test]
fn foreign_string_payload_transfers_and_releases_character_counts() {
    let mut s = session();
    let class = class(&mut s);
    let x = s
        .alloc_foreign_alt(class, SexpType::Str, SEXP_NIL, SEXP_NIL)
        .unwrap();
    let a = s.mk_char(b"before", Encoding::Native).unwrap();
    let b = s.mk_char(b"after", Encoding::Native).unwrap();
    let dense = s.alloc_str_from(&[a]).unwrap();
    assert_eq!(s.heap.refcnt(a.as_sexp()), 1);
    assert!(s.adopt_foreign_alt_payload(x, dense).unwrap());
    assert_eq!(s.heap.refcnt(a.as_sexp()), 1);
    s.str_set(x, 0, b).unwrap();
    assert_eq!(s.heap.refcnt(a.as_sexp()), 0);
    assert_eq!(s.heap.refcnt(b.as_sexp()), 1);

    s.heap.protect(x);
    s.collect(&EmptyRoots);
    assert!(s.char_bytes(a).is_err());
    assert_eq!(s.char_bytes(b).unwrap(), b"after");
    assert_eq!(s.str_get(x, 0).unwrap(), b);
    s.heap.unprotect(1).unwrap();
    s.heap.protect(b.as_sexp());
    s.collect(&EmptyRoots);
    assert_eq!(s.heap.refcnt(b.as_sexp()), 0);
}

#[test]
fn builtin_class_names_match_r_for_sequences_and_wrappers() {
    let mut s = session();
    let vectors = [
        (s.alloc_lgl_from(&[]).unwrap(), "wrap_logical"),
        (s.alloc_int_from(&[]).unwrap(), "wrap_integer"),
        (s.alloc_real_from(&[]).unwrap(), "wrap_real"),
        (s.alloc_cplx_from(&[]).unwrap(), "wrap_complex"),
        (s.alloc_raw_from(&[]).unwrap(), "wrap_raw"),
        (s.alloc_str_from(&[CharId::BLANK]).unwrap(), "wrap_string"),
    ];
    for (dense, name) in vectors {
        assert_eq!(s.builtin_alt_class(dense).unwrap(), None);
        assert_eq!(s.alt_class_names(dense).unwrap(), None);
        let wrapper = s.alloc_alt_wrapper(dense).unwrap();
        assert_eq!(s.builtin_alt_class(wrapper).unwrap(), Some((name, "base")));
        let (class, package) = s.alt_class_names(wrapper).unwrap().unwrap();
        assert_eq!(s.sym_name(class).unwrap(), name);
        assert_eq!(package, SYM_BASE);
    }
    let int = s.alloc_compact_intseq(1, 2, 1).unwrap();
    let real = s.alloc_compact_realseq(1.0, 2, 1.0).unwrap();
    assert_eq!(
        s.builtin_alt_class(int).unwrap(),
        Some(("compact_intseq", "base"))
    );
    assert_eq!(
        s.builtin_alt_class(real).unwrap(),
        Some(("compact_realseq", "base"))
    );
    assert!(!s.foreign_alt_seen());
}
