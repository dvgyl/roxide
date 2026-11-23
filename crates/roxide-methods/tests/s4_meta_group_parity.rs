//! Test S4 dispatch and namespace metadata with a deterministic host.

use roxide_eval::{Ctx, NsRecord};
use roxide_gc::GcPolicy;
use roxide_host::PureHost;
use roxide_methods::registry::MethodsExt;
use roxide_value::{GLOBAL_ENV, ValueApi};

fn context(policy: GcPolicy) -> Result<Ctx, String> {
    let mut ctx = Ctx::new(Box::new(PureHost::new()), policy).map_err(|e| format!("{e:?}"))?;
    roxide_base::register_all(&mut ctx).map_err(|e| format!("{e:?}"))?;
    roxide_methods::register(&mut ctx).map_err(|e| format!("{e:?}"))?;
    Ok(ctx)
}

#[test]
fn group_dispatch_uses_the_best_signature_and_survives_collection() -> Result<(), String> {
    let mut ctx = context(GcPolicy::Torture)?;
    ctx.run_script(
        "s4_meta_group.R",
        r#"
        setClass("RxParent", slots = c(data = "numeric"))
        setClass("RxChild", contains = "RxParent")
        a <- new("RxParent", data = 5)
        b <- new("RxChild", data = 7)
        setMethod("+", c("RxParent", "RxParent"), function(e1, e2) "operator")
        setMethod("Arith", c("RxParent", "RxParent"), function(e1, e2) paste0("arith", .Generic))
        setMethod("Ops", c("RxChild", "RxChild"), function(e1, e2) .Generic)
        stopifnot(identical(a + a, "operator"), identical(a * a, "arith*"))
        stopifnot(identical(b + b, structure("+", package = "base")))
        setMethod("Summary", "RxChild", function(x, ..., na.rm) list(sum(...), na.rm))
        stopifnot(identical(sum(b, 1, 2), list(3, FALSE)))
        setMethod("names", "RxChild", function(x) "name")
        stopifnot(identical(names(b), "name"))
        setMethod("+", c("RxChild", "numeric"), function(e1, e2) callGeneric(e1@data, e2))
        stopifnot(identical(b + 3, 10))
        stopifnot(identical(getGroup("+", recursive = TRUE), list("Arith", "Ops")))
        "#,
        GLOBAL_ENV,
        false,
    )
    .map_err(|e| format!("{e:?}"))?;
    Ok(())
}

#[test]
fn registering_and_uncaching_namespace_metadata_invalidates_the_epoch() -> Result<(), String> {
    let mut ctx = context(GcPolicy::Torture)?;
    ctx.run_script(
        "s4_meta_global.R",
        r#"
        setClass("RxGlobal", slots = c(data = "numeric"))
        setMethod("show", "RxGlobal", function(object) "global")
        "#,
        GLOBAL_ENV,
        false,
    )
    .map_err(|e| format!("{e:?}"))?;
    let name = "rxfixture";
    let (ns, imports) = ctx
        .ns_create(name, "1.0", None)
        .map_err(|e| format!("{e:?}"))?;
    ctx.protect(ns);
    ctx.ns_register(NsRecord {
        name: name.into(),
        ns,
        imports,
        attached: None,
        path: None,
        builtin: false,
    })
    .map_err(|e| format!("{e:?}"))?;
    ctx.run_script(
        "s4_meta_namespace.R",
        r#"
        methods::setClass("RxCached", slots = c(data = "numeric"))
        methods::setGeneric("rxCached", function(x) standardGeneric("rxCached"))
        methods::setMethod("rxCached", "RxCached", function(x) 17)
        methods::setMethod("names", "RxCached", function(x) "cached")
        methods::setMethod("show", "RxCached", function(object) "package")
        "#,
        ns,
        false,
    )
    .map_err(|e| format!("{e:?}"))?;
    let class_key = ("RxCached".to_owned(), name.to_owned());
    let method_key = ("rxCached".to_owned(), name.to_owned());
    let primitive_key = ("names".to_owned(), "base".to_owned());
    let registry = ctx.ext::<MethodsExt>();
    registry.classes.remove(&class_key);
    registry.generics.remove(&method_key);
    let before = registry.epoch;
    let methods = ctx
        .ns_lookup("methods")
        .ok_or("missing methods namespace")?;
    ctx.call_function(".rx_cacheMetaData", &[(None, ns)], methods)
        .map_err(|e| format!("{e:?}"))?;
    let registry = ctx.ext::<MethodsExt>();
    assert!(registry.epoch > before);
    assert_eq!(
        registry.classes[&class_key].slots,
        [("data".into(), "numeric".into())]
    );
    assert_eq!(registry.generics[&method_key].signature, ["x"]);
    assert_eq!(registry.generics[&primitive_key].signature, ["x"]);
    ctx.session_mut()
        .env_lock_all_bindings(ns)
        .map_err(|error| format!("{error:?}"))?;
    ctx.session_mut()
        .env_set_locked(ns, true)
        .map_err(|error| format!("{error:?}"))?;
    let before = ctx.ext::<MethodsExt>().epoch;
    ctx.run_script(
        "s4_meta_combined.R",
        r#"
        stopifnot(identical(show(new("RxGlobal")), "global"))
        stopifnot(identical(show(new("RxCached")), "package"))
        setClass("RxSubclass", contains = "RxCached")
        stopifnot(identical(new("RxSubclass", data = 2)@data, 2))
        stopifnot(identical(getClass("RxCached")@subclasses$RxSubclass@distance, 1))
        "#,
        GLOBAL_ENV,
        false,
    )
    .map_err(|e| format!("{e:?}"))?;
    let detach = ctx
        .session_mut()
        .scalar_lgl(roxide_value::RLogical::FALSE)
        .map_err(|e| format!("{e:?}"))?;
    ctx.call_function(".rx_cacheMetaData", &[(None, ns), (None, detach)], methods)
        .map_err(|e| format!("{e:?}"))?;
    ctx.ns_unregister(name);
    let registry = ctx.ext::<MethodsExt>();
    assert!(registry.epoch > before);
    assert!(!registry.classes.contains_key(&class_key));
    assert!(!registry.generics.contains_key(&method_key));
    assert!(!registry.generics.contains_key(&primitive_key));
    Ok(())
}
