//! Compare the datasets with GNU R and test the utility functions with a pure host.

#![forbid(unsafe_code)]

use std::process::Command;

use roxide_eval::{Ctx, Module, Package};
use roxide_gc::GcPolicy;
use roxide_host::PureHost;
use roxide_value::{GLOBAL_ENV, ValueApi};

const DATASET_OBJECTS: &str = r#"c("iris", "mtcars", "cars", "women", "pressure", "BOD",
    "state.abb", "state.name", "state.region", "state.division")"#;
const CASES: &str = include_str!("../../../conformance/layer3/26_utils_datasets_tier2.rcase");

fn session() -> Ctx {
    let mut ctx = Ctx::new(Box::new(PureHost::new()), GcPolicy::Torture).expect("Create session");
    roxide_base::register_all(&mut ctx).expect("Register base");
    roxide_utils::register(&mut ctx).expect("Register utils");
    ctx
}

fn run(ctx: &mut Ctx, name: &str, script: &str) {
    if let Err(error) = ctx.run_script(name, script, GLOBAL_ENV, false) {
        let _ = ctx.run_script(
            "Get error",
            "test_failure <- geterrmessage()",
            GLOBAL_ENV,
            false,
        );
        let session = ctx.session_mut();
        let symbol = session.intern("test_failure").expect("Create the symbol");
        if let Some(binding) = session.env_get(GLOBAL_ENV, symbol)
            && let Ok([value]) = session.str_slice(binding.value)
            && let Ok(bytes) = session.char_bytes(*value)
        {
            eprintln!("{}", String::from_utf8_lossy(bytes));
        }
        panic!("{name}: {error:?}");
    }
}

#[test]
fn datasets_match_gnu_r_in_full() {
    let Some(rscript) = std::env::var_os("ROXIDE_ORACLE_RSCRIPT") else {
        eprintln!("Set ROXIDE_ORACLE_RSCRIPT to compare every dataset with GNU R 4.6.2.");
        return;
    };
    let query = format!(
        r#"
stopifnot(as.character(getRversion()) == "4.6.2")
objects <- mget({DATASET_OBJECTS}, as.environment("package:datasets"), inherits = FALSE)
cat(sprintf("%02x", as.integer(serialize(objects, NULL, version = 2))), sep = "")
"#
    );
    let output = Command::new(rscript)
        .args(["--vanilla", "-e", &query])
        .output()
        .expect("Run GNU R");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let hex = std::str::from_utf8(&output.stdout).expect("Read the hexadecimal response");
    assert_eq!(hex.len() % 2, 0);
    let bytes: Vec<_> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("Decode a byte"))
        .collect();

    let mut ctx = session();
    roxide_utils::datasets::register(&mut ctx).expect("Register datasets");
    let session = ctx.session_mut();
    let name = session.intern("oracle_bytes").expect("Create the symbol");
    let raw = session.alloc_raw_from(&bytes).expect("Store GNU R data");
    session
        .env_define(GLOBAL_ENV, name, raw)
        .expect("Bind GNU R data");
    run(
        &mut ctx,
        "Compare each dataset",
        &format!(
            r#"
expected <- unserialize(oracle_bytes)
objects <- mget({DATASET_OBJECTS}, as.environment("package:datasets"), inherits = FALSE)
for (name in names(objects)) {{
    stopifnot(identical(objects[[name]], expected[[name]], num.eq = FALSE, attrib.as.set = FALSE))
    stopifnot(identical(get(name, envir = asNamespace("datasets"), inherits = FALSE),
                        expected[[name]], num.eq = FALSE, attrib.as.set = FALSE))
}}
"#
        ),
    );
}

#[test]
fn tier2_conformance_works_with_a_pure_host() {
    for case in CASES.split("=== case: ").skip(1) {
        let (name, script) = case.split_once(" ===\n").expect("Read the case name");
        if name == "tier2_install_local_packages" {
            continue;
        }
        let mut ctx = session();
        if name == "tier2_datasets_exports" {
            roxide_utils::datasets::register(&mut ctx).expect("Register datasets");
        }
        run(&mut ctx, name, script);
    }
}

#[test]
fn assign_in_my_namespace_updates_the_callers_binding() {
    const MODULE: Module = Module {
        prims: &[],
        prelude: r#"
value <- 1L
replace <- function() utils::assignInMyNamespace("value", 2L)
"#,
    };
    const PACKAGE: Package = Package {
        name: "waveNamespace",
        attach: false,
        exports: None,
        modules: &[&MODULE],
    };
    let mut ctx = session();
    ctx.register_package(&PACKAGE)
        .expect("Register test package");
    run(
        &mut ctx,
        "Assign in the calling namespace",
        r#"
ns <- asNamespace("waveNamespace")
was.locked <- bindingIsLocked("value", ns)
waveNamespace::replace()
stopifnot(identical(utils::getFromNamespace("value", "waveNamespace"), 2L),
          identical(bindingIsLocked("value", ns), was.locked))
"#,
    );
}

#[test]
fn install_packages_reports_offline_mode() {
    run(
        &mut session(),
        "Report offline installation",
        r#"
message <- tryCatch(utils::install.packages("waveTest", repos = "https://example.invalid"),
                    error = conditionMessage)
stopifnot(grepl("offline mode", message, fixed = TRUE))
"#,
    );
}
