#![forbid(unsafe_code)]

//! Exercise reference classes with a deterministic host and collection at every safe point.

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::PureHost;

fn run(source: &'static str) -> Result<(), String> {
    const STACK_BYTES: usize = 32 * 1024 * 1024;
    std::thread::Builder::new()
        .stack_size(STACK_BYTES)
        .spawn(move || run_with_stack(source, STACK_BYTES - 2 * 1024 * 1024))
        .map_err(|error| format!("start test thread: {error}"))?
        .join()
        .map_err(|_| String::from("reference class test thread panicked"))?
}

fn run_with_stack(source: &str, stack_budget: usize) -> Result<(), String> {
    for policy in [GcPolicy::default(), GcPolicy::Torture] {
        let mut ctx = Ctx::new(Box::new(PureHost::new()), policy)
            .map_err(|error| format!("create context: {error:?}"))?;
        ctx.set_stack_budget(stack_budget);
        roxide_base::register_all(&mut ctx).map_err(|error| format!("register base: {error:?}"))?;
        roxide_methods::register(&mut ctx)
            .map_err(|error| format!("register methods: {error:?}"))?;
        let (result, output) = ctx
            .capture_streams(false, true, false, |ctx| {
                Ok(ctx.run_global("<reference class>", source))
            })
            .map_err(|error| format!("capture diagnostics: {error:?}"))?;
        result.map_err(|error| {
            format!(
                "evaluate reference class script with {policy:?}: {error:?}\n{}",
                String::from_utf8_lossy(&output)
            )
        })?;
    }
    Ok(())
}

#[test]
fn reference_alias_and_active_binding_survive_collection() -> Result<(), String> {
    run(r#"
G <- setRefClass("TortureActive", fields = list(
    n = "numeric", doubled = function(value) {
        if (missing(value)) n * 2 else n <<- value / 2
    }), methods = list(bump = function(by) {
        doubled <<- doubled + by
        .self
    }))
a <- G(n = 2)
b <- a
b$n <- 7L
saved <- a$bump
stopifnot(identical(saved(4), b), identical(a$n, 9), identical(b$doubled, 18))
stopifnot(identical(a$.self, a), bindingIsActive("doubled", a@.xData))
"#)
}

#[test]
fn inherited_methods_locks_and_nested_copies_survive_collection() -> Result<(), String> {
    run(r#"
P <- setRefClass("TortureParent", fields = c(n = "numeric"),
    methods = list(value = function() n))
invisible(P$lock("n"))
C <- setRefClass("TortureChild", contains = "TortureParent",
    methods = list(value = function() callSuper() * 3))
B <- setRefClass("TortureBox", fields = list(item = "TortureChild"))
item <- C(n = 2)
box <- B(item = item)
deep <- box$copy()
shallow <- box$copy(shallow = TRUE)
stopifnot(identical(deep$item$value(), 6))
stopifnot(!identical(deep$item, item), identical(shallow$item, item))
stopifnot(tryCatch({item$n <- 8; FALSE}, error = function(e) TRUE))
stopifnot(identical(item$n, 2))
"#)
}
