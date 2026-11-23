#![forbid(unsafe_code)]

//! Checks the S4 value layout and class cache roots under both GC policies.

use roxide_gc::{GcPolicy, RootSet, Trace, Tracer};
use roxide_methods::registry::{ClassInfo, MethodsExt};
use roxide_value::{RSession, SEXP_NIL, SEXP_S4_NULL, SexpType, ValueApi, ValueError};

struct ClassRoots(MethodsExt);

impl RootSet for ClassRoots {
    fn trace_roots(&self, tracer: &mut Tracer<'_>) {
        self.0.trace(tracer);
    }
}

#[test]
fn class_cache_keeps_prototype_and_null_slots_alive() -> Result<(), ValueError> {
    for policy in [GcPolicy::default(), GcPolicy::Torture] {
        let mut session = RSession::new(policy)?;
        let def = session.alloc_s4_object()?;
        let object = session.alloc_s4_object()?;
        let x = session.intern("x")?;
        let two = session.scalar_real(2.0)?;
        session.set_attrib(object, x, two)?;
        let n = session.intern("n")?;
        session.set_attrib(object, n, SEXP_S4_NULL)?;
        let class_attr = session.intern("class")?;
        let package_attr = session.intern("package")?;
        let name = session.scalar_str("A")?;
        let package = session.scalar_str(".GlobalEnv")?;
        session.set_attrib(name, package_attr, package)?;
        session.set_attrib(object, class_attr, name)?;
        let prototype = session.intern("prototype")?;
        session.set_attrib(def, prototype, object)?;
        let info = ClassInfo {
            name: "A".into(),
            package: ".GlobalEnv".into(),
            slots: vec![("x".into(), "numeric".into()), ("n".into(), "NULL".into())],
            superclasses: Vec::new(),
            is_virtual: false,
            class_def: def,
        };
        let key = (info.name.clone(), info.package.clone());
        let mut roots = ClassRoots(MethodsExt::default());
        roots.0.classes.insert(key, info);
        session.collect(&roots);
        let duplicate = session.shallow_duplicate(session.attr(def, prototype))?;
        assert!(session.is_s4(duplicate)?);
        assert_eq!(session.type_of(duplicate)?, SexpType::Obj);
        assert_eq!(
            session.attrs(duplicate)?,
            vec![(x, two), (n, SEXP_S4_NULL), (class_attr, name)]
        );
        assert_eq!(session.attr(name, package_attr), package);
        let undefined = session.intern("undefined")?;
        assert_eq!(session.attr(duplicate, undefined), SEXP_NIL);
    }
    Ok(())
}

#[test]
fn data_storage_type_survives_s4_copy() -> Result<(), ValueError> {
    let mut session = RSession::new(GcPolicy::Torture)?;
    let vector = session.alloc_int_from(&[1, 2, 3])?;
    session.set_s4_flag(vector, true)?;
    let tag_symbol = session.intern("tag")?;
    let tag = session.scalar_str("a")?;
    session.set_attrib(vector, tag_symbol, tag)?;
    let duplicate = session.shallow_duplicate(vector)?;
    session.clear_attrs(duplicate)?;
    session.set_s4_flag(duplicate, false)?;
    assert_eq!(session.int(duplicate)?.to_vec(), vec![1, 2, 3]);
    assert!(!session.is_s4(duplicate)?);
    assert!(session.is_s4(vector)?);
    assert_eq!(session.attr(vector, tag_symbol), tag);
    Ok(())
}

#[test]
fn coercion_resolves_class_again_after_test_removes_it() -> Result<(), String> {
    use roxide_eval::Ctx;
    use roxide_host::PureHost;

    let mut ctx = Ctx::new(Box::new(PureHost::new()), GcPolicy::Torture)
        .map_err(|error| format!("{error:?}"))?;
    roxide_base::register_all(&mut ctx).map_err(|error| format!("{error:?}"))?;
    roxide_methods::register(&mut ctx).map_err(|error| format!("{error:?}"))?;
    ctx.run_global(
        "<coercion test>",
        r#"
setClass("From", slots = c(x = "numeric"))
setClass("To", slots = c(x = "numeric"))
setIs("From", "To",
      test = function(object) { removeClass("From"); TRUE },
      coerce = function(from) new("To", x = from@x),
      replace = function(from, value) from)
obj <- new("From")
message <- tryCatch(as(obj, "To"), error = conditionMessage)
stopifnot(identical(message, "no method or default for coercing “From” to “To”"))
"#,
    )
    .map_err(|error| format!("{error:?}"))?;
    Ok(())
}
