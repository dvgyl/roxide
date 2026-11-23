//! Exercise package loading on an in-memory host.

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::{Host, PureHost};

fn run(host: PureHost, policy: GcPolicy, source: &str) -> String {
    let mut ctx = Ctx::new(Box::new(host), policy).expect("create evaluation context");
    roxide_base::register_all(&mut ctx).expect("register base functions");
    roxide_pkg::register(&mut ctx).expect("register package functions");
    let (result, output) = ctx
        .capture_streams(true, true, false, |ctx| {
            Ok(ctx.run_global("package test", source))
        })
        .expect("capture output");
    let output = String::from_utf8(output).expect("UTF-8 output");
    assert!(result.is_ok(), "{result:?}: {output}");
    output
}

#[test]
fn source_collate_imports_and_deferred_s3_survive_torture() {
    let mut host = PureHost::new();
    host.set_env_var("R_LIBS", Some(b"/work"))
        .expect("set library path");
    host.insert_file(
        "/work/provider/DESCRIPTION",
        b"Package: provider\nVersion: 1.0\n".to_vec(),
    );
    host.insert_file(
        "/work/provider/NAMESPACE",
        b"export(talk, visible, ignored)\n".to_vec(),
    );
    host.insert_file(
        "/work/provider/R/provider",
        b"talk <- function(x) UseMethod('talk')\nvisible <- 42L\nignored <- 5L\n".to_vec(),
    );
    host.insert_file("/work/consumer/DESCRIPTION", b"# metadata\nPackage: consumer\nVersion: 2.0\nTitle: Collate probe\nImports: provider (>= 1.0)\nCollate.unix: 'z.r'\n 'a.R'\n".to_vec());
    host.insert_file("/work/consumer/NAMESPACE", b"import(provider, except = ignored)\nexport(answer)\nS3method(provider::talk, foo, speak)\n".to_vec());
    host.insert_file("/work/consumer/R/z.r", b"first <- 3L\n".to_vec());
    host.insert_file(
        "/work/consumer/R/a.R",
        b"answer <- visible + first\nspeak <- function(x) answer\n".to_vec(),
    );
    let output = run(
        host,
        GcPolicy::Torture,
        r#"
        help <- library(help = consumer, lib.loc = "/work")
        stopifnot(identical(help$name, "consumer"), inherits(help, "packageInfo"))
        inventory <- library(lib.loc = "/work")
        stopifnot(inherits(inventory, "libraryIQR"),
                  inventory$results[1L, "Title"] == "Collate probe")
        dir.create("/work/consumer/Meta")
        saveRDS(parseNamespaceFile("consumer", "/work"), "/work/consumer/Meta/nsInfo.rds")
        writeLines("unsupported()", "/work/consumer/NAMESPACE")
        partial <- loadNamespace("consumer", partial = TRUE)
        stopifnot(!isNamespaceLoaded("consumer"), !environmentIsLocked(partial),
                  length(getNamespaceExports(partial)) == 0L)
        stopifnot(consumer::answer == 45L)
        stopifnot(!exists("ignored", envir = parent.env(getNamespace("consumer")), inherits = FALSE))
        stopifnot(tryCatch({unloadNamespace("provider"); FALSE}, error = function(e) TRUE))
        unloadNamespace("consumer")
        cat(provider::talk(structure(1L, class = "foo")))
    "#,
    );
    assert_eq!(output, "45");
}

#[test]
fn rejects_namespace_cycles_and_retries_after_failure() {
    let mut host = PureHost::new();
    host.set_env_var("R_LIBS", Some(b"/work"))
        .expect("set library path");
    for (name, other) in [("one", "two"), ("two", "one")] {
        host.insert_file(
            format!("/work/{name}/DESCRIPTION"),
            format!("Package: {name}\nVersion: 1.0\n"),
        );
        host.insert_file(
            format!("/work/{name}/NAMESPACE"),
            format!("import({other})"),
        );
    }
    let output = run(
        host,
        GcPolicy::Torture,
        r#"
        stopifnot(tryCatch({loadNamespace("one"); FALSE},
                           error = function(e) grepl("cyclic namespace dependency", conditionMessage(e))))
        stopifnot(!isNamespaceLoaded("one"), !isNamespaceLoaded("two"))
        writeLines("export()", "/work/two/NAMESPACE")
        stopifnot(isNamespace(loadNamespace("one")))
        cat("retry passed")
    "#,
    );
    assert_eq!(output, "retry passed");
}

#[test]
fn imports_formal_class_and_method_bindings() {
    let mut host = PureHost::new();
    host.set_env_var("R_LIBS", Some(b"/work"))
        .expect("set library path");
    for name in ["metaprov", "metacons"] {
        host.insert_file(
            format!("/work/{name}/DESCRIPTION"),
            format!("Package: {name}\nVersion: 1.0\n"),
        );
    }
    host.insert_file(
        "/work/metaprov/NAMESPACE",
        b"exportClassPattern('^Shape$')\nexportMethods(score)\n".to_vec(),
    );
    host.insert_file(
        "/work/metaprov/R/metaprov",
        b".__C__Shape <- 4L\n`.__T__score:metaprov` <- 8L\nscore <- function(x) 5L\n".to_vec(),
    );
    host.insert_file(
        "/work/metacons/NAMESPACE",
        b"importClassesFrom(metaprov, Shape)\nimportMethodsFrom(metaprov, score)\nexport(values)\n"
            .to_vec(),
    );
    host.insert_file(
        "/work/metacons/R/metacons",
        b"values <- function() c(.__C__Shape, `.__T__score:metaprov`, score(NULL))\n".to_vec(),
    );
    let output = run(
        host,
        GcPolicy::Torture,
        r#"
        cat(metacons::values())
        "#,
    );
    assert_eq!(output, "4 8 5");
}
