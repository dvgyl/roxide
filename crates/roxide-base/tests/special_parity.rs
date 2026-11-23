//! Check special-function argument handling under garbage collection stress.

#![forbid(unsafe_code)]

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::PureHost;
use roxide_value::GLOBAL_ENV;

#[test]
fn special_vectors_preserve_attributes_and_warn_on_partial_recycling() {
    let mut ctx =
        Ctx::new(Box::new(PureHost::new()), GcPolicy::Torture).expect("Create the R session");
    roxide_base::register_all(&mut ctx).expect("Register the base package");
    ctx.run_script(
        "special.rs",
        r#"
x <- structure(c(1, 2, 3), names = c("a", "b", "c"))
warnings <- character()
y <- withCallingHandlers(choose(x, c(1, 0)), warning = function(w) {
    warnings <<- c(warnings, conditionMessage(w))
    invokeRestart("muffleWarning")
})
if (length(warnings) != 0L)
    stop("GNU R recycles special-function lengths silently without warning")
if (!identical(y, structure(c(1, 1, 3), names = names(x))))
    stop("The result must use the attributes of the longer input")
y <- withCallingHandlers(
    besselK(c(NA_real_, NaN, NaN, 0, Inf), c(NaN, NA_real_, 0, 0, 0), TRUE),
    warning = function(w) stop("Missing values must propagate without a warning")
)
if (!identical(y, c(NA_real_, NA_real_, NaN, Inf, 0)))
    stop("NA must take precedence over NaN")
y <- besselI(0, 0, structure(c(TRUE, FALSE, NA), names = names(x)))
if (!identical(y, c(1, 1, NA_real_)))
    stop("The scale argument must recycle without supplying attributes")
"#,
        GLOBAL_ENV,
        false,
    )
    .expect("Check the R bindings");
}
