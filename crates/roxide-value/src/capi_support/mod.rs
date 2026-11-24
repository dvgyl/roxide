//! Safe low-level [`crate::RSession`] accessors supporting `roxide-capi` and `roxide-eval::vm`
//! while keeping `roxide-value` strictly `#![forbid(unsafe_code)]` (ADRs 0017–0020).

mod dataptr;
mod extptr;
mod mutate;
mod strings;
mod weakref;

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use roxide_gc::{GcPolicy, RootSet, Tracer};

    use crate::alt::Sortedness;
    use crate::api::{ValueApi, ValueError};
    use crate::object::{CharId, SexpType};
    use crate::session::{BASE_NAMESPACE, RSession, SEXP_NIL};
    use crate::session::{
        BLANK_SCALAR_STRING, EMPTY_ENV, METHODS_NAMESPACE, NAMESPACE_REGISTRY, SYM_BASE, SYM_CLASS,
        SYM_DOTS,
    };

    struct EmptyRoots;
    impl RootSet for EmptyRoots {
        fn trace_roots(&self, _t: &mut Tracer<'_>) {}
    }

    #[test]
    fn permanent_capi_singletons_and_symbols() {
        let mut s = RSession::new(GcPolicy::default()).unwrap();
        assert_eq!(s.type_of(BLANK_SCALAR_STRING).unwrap(), SexpType::Str);
        assert_eq!(s.xlength(BLANK_SCALAR_STRING).unwrap(), 1);
        assert_eq!(s.type_of(NAMESPACE_REGISTRY).unwrap(), SexpType::Env);
        assert_eq!(s.type_of(METHODS_NAMESPACE).unwrap(), SexpType::Env);
        assert_eq!(
            s.env_get(NAMESPACE_REGISTRY, SYM_BASE).unwrap().value,
            BASE_NAMESPACE
        );
        assert_eq!(s.intern("class").unwrap(), SYM_CLASS);
        assert_eq!(s.intern("...").unwrap(), SYM_DOTS);
        assert_eq!(s.intern("base").unwrap(), SYM_BASE);
    }

    #[test]
    fn resizable_vector_keeps_pinned_address() {
        let mut s = RSession::new(GcPolicy::default()).unwrap();
        let v = s.alloc_resizable(SexpType::Int, 2, 8).unwrap();
        assert!(s.is_resizable(v).unwrap());
        assert_eq!(s.xlength(v).unwrap(), 2);
        assert_eq!(s.vec_truelength(v).unwrap(), 8);
        let (p0, l0) = s.data_ptr_ro(v).unwrap();
        assert_eq!(l0, 2);
        s.resize_vector(v, 5).unwrap();
        let (p1, l1) = s.data_ptr_rw(v).unwrap();
        assert_eq!(p0, p1);
        assert_eq!(l1, 5);
        assert!(s.resize_vector(v, 9).is_err());
    }

    #[test]
    fn grow_vector_geometric_reuses_buffer_and_hides_spare_capacity() {
        let mut s = RSession::new(GcPolicy::default()).unwrap();
        let v0 = s.alloc_real_from(&[1.0, 2.0]).unwrap();
        let v1 = s.grow_vector_geometric(v0, 3).unwrap().unwrap();
        assert_ne!(v0, v1);
        assert!(!s.is_resizable(v1).unwrap());
        assert_eq!(s.xlength(v1).unwrap(), 3);
        assert!(s.real(v1).unwrap().get(2).is_nan());
        let (p1, _) = s.data_ptr_ro(v1).unwrap();
        let v2 = s.grow_vector_geometric(v1, 10).unwrap().unwrap();
        assert_eq!(v1, v2);
        let (p2, l2) = s.data_ptr_ro(v2).unwrap();
        assert_eq!(p1, p2);
        assert_eq!(l2, 10);
    }

    #[test]
    fn extptr_weakref_and_env_shape_version() {
        let mut s = RSession::new(GcPolicy::Torture).unwrap();
        let env = s.alloc_env(EMPTY_ENV).unwrap();
        let v0 = s.env_shape_version(env).unwrap();
        let k = s.intern("x").unwrap();
        let val = s.scalar_int(42).unwrap();
        s.env_define(env, k, val).unwrap();
        let v1 = s.env_shape_version(env).unwrap();
        assert_eq!(v1, v0 + 1);
        s.env_define(env, k, val).unwrap();
        assert_eq!(s.env_shape_version(env).unwrap(), v1);
        assert!(s.env_remove(env, k).unwrap());
        assert_eq!(s.env_shape_version(env).unwrap(), v1 + 1);

        let ep = s.alloc_extptr(0x1234, SYM_CLASS.as_sexp(), env).unwrap();
        assert_eq!(s.extptr_addr(ep).unwrap(), 0x1234);
        assert_eq!(s.extptr_tag(ep).unwrap(), SYM_CLASS.as_sexp());
        assert_eq!(s.extptr_prot(ep).unwrap(), env);
        let old_count = s.heap.refcnt(env);
        let new_count = s.heap.refcnt(val);
        s.set_extptr_prot(ep, val).unwrap();
        assert_eq!(s.extptr_prot(ep).unwrap(), val);
        assert_eq!(s.heap.refcnt(env), old_count - 1);
        assert_eq!(s.heap.refcnt(val), new_count + 1);
        s.set_extptr_addr(ep, 0).unwrap();
        assert_eq!(s.extptr_addr(ep).unwrap(), 0);

        let key = s.alloc_env(EMPTY_ENV).unwrap();
        let wval = s.scalar_int(7).unwrap();
        let wr = s.alloc_weakref(key, wval, None, false).unwrap();
        let m = s.heap.protect_mark();
        s.heap.protect(wr);
        s.heap.protect(key);
        s.collect(&EmptyRoots);
        assert_eq!(s.weakref_key(wr).unwrap(), key);
        assert_eq!(s.weakref_value(wr).unwrap(), wval);
        s.heap.unprotect(1).unwrap();
        s.collect(&EmptyRoots);
        assert_eq!(s.weakref_key(wr).unwrap(), SEXP_NIL);
        assert_eq!(s.weakref_value(wr).unwrap(), SEXP_NIL);
        s.heap.unprotect_to(m);
    }

    #[test]
    fn str_helpers_and_alt_metadata_under_torture_gc() {
        use crate::alt::{AltClass, AltString, CompactIntSeq};
        use crate::object::Encoding;
        use roxide_gc::Trace;

        #[derive(Debug)]
        struct TestAltStr(usize);
        impl Trace for TestAltStr {
            fn trace(&self, _t: &mut Tracer<'_>) {}
        }
        impl AltClass for TestAltStr {
            fn class_name(&self) -> &'static str {
                "test_alt_str"
            }
            fn package_name(&self) -> &'static str {
                "test"
            }
            fn length(&self, _heap: &crate::session::RHeap) -> usize {
                self.0
            }
        }
        impl AltString for TestAltStr {
            fn elt(&self, session: &mut RSession, i: usize) -> Result<CharId, ValueError> {
                session.mk_char(format!("e{i}").as_bytes(), Encoding::Utf8)
            }
            fn set_elt(
                &self,
                _session: &mut RSession,
                _i: usize,
                _val: CharId,
            ) -> Result<(), ValueError> {
                Err(ValueError::InvalidAttribute("read-only alt string".into()))
            }
            fn is_sorted(&self) -> Sortedness {
                Sortedness::Increasing
            }
            fn no_na(&self) -> bool {
                true
            }
        }

        let mut s = RSession::new(GcPolicy::Torture).unwrap();
        let m = s.heap.protect_mark();
        let a = s.mk_char(b"alpha", Encoding::Utf8).unwrap();
        s.heap.protect(a.as_sexp());
        let b = s.mk_char(b"beta", Encoding::Utf8).unwrap();
        s.heap.protect(b.as_sexp());
        assert_eq!(s.sexp_as_char(a.as_sexp()).unwrap(), a);
        assert!(s.sexp_as_char(SEXP_NIL).is_err());

        let dense = s.alloc_str_from(&[a, a]).unwrap();
        s.heap.protect(dense);
        assert_eq!(s.str_get(dense, 1).unwrap(), a);
        s.str_set(dense, 1, b).unwrap();
        assert_eq!(s.str_get(dense, 1).unwrap(), b);
        assert_eq!(s.str_dataptr_ro(dense).unwrap(), &[a, b]);

        let alt_str = s.alloc_alt_str(Rc::new(TestAltStr(3))).unwrap();
        s.heap.protect(alt_str);
        assert_eq!(s.alt_is_sorted(alt_str).unwrap(), Sortedness::Increasing);
        assert!(s.alt_no_na(alt_str).unwrap());
        let e2 = s.str_get(alt_str, 2).unwrap();
        assert_eq!(s.char_bytes(e2).unwrap(), b"e2");
        let ptr_slice = s.str_dataptr_ro(alt_str).unwrap().to_vec();
        assert_eq!(ptr_slice.len(), 3);
        assert_eq!(s.char_bytes(ptr_slice[0]).unwrap(), b"e0");
        assert_eq!(s.char_bytes(ptr_slice[2]).unwrap(), b"e2");
        s.str_set(alt_str, 0, b).unwrap();
        assert_eq!(s.str_get(alt_str, 0).unwrap(), b);

        let seq = s
            .alloc_alt_int(Rc::new(CompactIntSeq {
                start: 1,
                len: 5,
                incr: 1,
            }))
            .unwrap();
        assert_eq!(s.alt_is_sorted(seq).unwrap(), Sortedness::Increasing);
        assert!(s.alt_no_na(seq).unwrap());
        s.heap.unprotect_to(m);
    }
}
