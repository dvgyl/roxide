use roxide_gc::GcPolicy;

use crate::testing::run_with_policy;

fn check(source: &str, stdout: &str, stderr: &str) {
    for policy in [GcPolicy::default(), GcPolicy::Torture] {
        let outcome = run_with_policy(source, policy);
        assert_eq!(outcome.status, 0, "{outcome:?}");
        assert_eq!(outcome.stdout, stdout);
        assert_eq!(outcome.stderr, stderr);
    }
}

#[test]
fn closure_matches_in_three_passes_and_keeps_dots_lazy() {
    check(
        r#"
seen <- 0L
target <- function(alpha, alpine = 8L, ..., after = 9L) {
    dput(c(alpha, alpine, after, ...length()))
    dput(missing(alpine))
    dput(..2)
    dput(...elt(2))
    dput(deparse1(match.call(expand.dots = FALSE), width.cutoff = 500L))
}
relay <- function(...) target(..., after = 6L)
relay(al = 2L, alpha =, 4L, note = stop("unused"),
      value = { seen <- seen + 1L; 7L })
dput(seen)
dput(tryCatch((function(alpha, alpine) NULL)(al = 5L), error = conditionMessage))
dput(tryCatch((function(alpha) NULL)(alpha = 1, alpha = 2), error = conditionMessage))
dput(tryCatch((function(alpha) NULL)(alpha = 1, extra = stop("unused")), error = conditionMessage))
"#,
        concat!(
            "c(4L, 2L, 6L, 2L)\nFALSE\n7L\n7L\n",
            "\"target(alpha = ..2, alpine = 2L, ... = pairlist(4L, note = ..4, value = ..5), after = 6L)\"\n1L\n",
            "\"argument 1 matches multiple formal arguments\"\n",
            "\"formal argument \\\"alpha\\\" matched by multiple actual arguments\"\n",
            "\"unused argument (extra = stop(\\\"unused\\\"))\"\n",
        ),
        "",
    );
}

#[test]
fn substitute_and_missing_inspect_the_original_promise() {
    check(
        r#"
capture <- function(x = 2L + 3L, ...) {
    force(x)
    dput(missing(x))
    dput(substitute(x))
    dput(substitute(list(...)))
    x <- NULL
    dput(missing(x))
}
caller <- function(value) capture(value * 3L, bad = stop("unused"))
caller(3L)
capture()
inner <- function(x) missing(x)
outer <- function(x) inner(x)
defaulted <- function(x = 8L) inner(x)
dput(c(outer(), outer(NULL), defaulted()))
dput(do.call(substitute, list(quote(a + b), list(a = 5L, b = quote(c)))))
dput(eval(parse(text = c("x <- 5L", "x + y")), list(y = 3L)))
dput(deparse1(quote(x <- 2L)))
"#,
        concat!(
            "FALSE\nvalue * 3L\nlist(bad = stop(\"unused\"))\nFALSE\n",
            "TRUE\n2L + 3L\nlist()\nFALSE\n",
            "c(TRUE, FALSE, FALSE)\n5L + c\n8L\n\"x <- 2L\"\n",
        ),
        "",
    );
}

#[test]
fn condition_handlers_resume_or_unwind_with_cleanup() {
    check(
        r#"
inner <- function(x) {
    on.exit(cat("inner\n"))
    x
}
outer <- function() {
    on.exit(cat("outer\n"))
    inner(return(invisible(4L)))
    stop("unreachable")
}
dput(withVisible(outer()))
f <- function() {
    on.exit(cat("last\n"))
    on.exit(stop("cleanup"), add = TRUE, after = FALSE)
    stop("body")
}
dput(tryCatch(f(), error = conditionMessage, finally = cat("finally\n")))
dput(withCallingHandlers(withCallingHandlers({
    warning("warn")
    message("message")
    6L
}, warning = function(w) cat("new\n")), warning = function(w) {
    cat("old: ", conditionMessage(w), "\n", sep = "")
    invokeRestart("muffleWarning")
}, message = function(m) {
    cat(conditionMessage(m), sep = "")
    invokeRestart(findRestart("muffleMessage"))
}))
dput(withRestarts(inner(invokeRestart("retry", 9L)), retry = function(x) x + 1L))
dput(withVisible(tryCatch(invisible(2L), finally = 3L)))
"#,
        concat!(
            "inner\nouter\nlist(value = 4L, visible = FALSE)\n",
            "last\nfinally\n\"cleanup\"\nnew\nold: warn\nmessage\n6L\ninner\n10L\n",
            "list(value = 2L, visible = FALSE)\n",
        ),
        "",
    );
}

#[test]
fn interrupted_promise_retries_and_can_muffle_the_warning() {
    check(
        r#"
force_twice <- function(x) {
    dput(tryCatch(x, error = conditionMessage))
    withCallingHandlers(dput(x), warning = function(w) {
        dput(conditionMessage(w))
        invokeRestart("muffleWarning")
    })
}
count <- 0L
force_twice({ count <- count + 1L; if (count == 1L) stop("first"); count })
dput(count)
"#,
        "\"first\"\n\"restarting interrupted promise evaluation\"\n2L\n2L\n",
        "",
    );
}

#[test]
fn loop_and_replacement_preserve_values_across_safepoints() {
    check(
        r#"
x <- list(c(1L, 2L), c(3L, 4L))
names(x) <- c("a", "-")
names(x)[2L] <- "b"
x[[1L]][2L] <- 99L
attr(x, "unit") <- "m"
attr(x, "unit") <- NULL
dput(x)
third <- function(x) x[[1L]]
`third<-` <- function(x, value) {
    x[[1L]] <- value
    x
}
f <- function() third(x)[1L] <<- 7L
dput(withVisible(f()))
dput(x)
result <- 0L
for (item in 1L:7L) {
    if (item == 2L) next
    if (item > 4L) break
    result <- result + item
}
dput(c(result, item))
for (item in NULL) stop("unreachable")
dput(item)
for (label in structure(c(2L, 1L), class = "factor", levels = c("a", "b"))) dput(label)
for (value in pairlist(3L, 4L)) dput(value)
dput(tryCatch(if (c(TRUE, FALSE)) 1, error = conditionMessage))
"#,
        concat!(
            "list(a = c(1L, 99L), b = 3:4)\n",
            "list(value = 7L, visible = FALSE)\n",
            "list(a = c(7L, 99L), b = 3:4)\n",
            "c(8L, 5L)\nNULL\n\"b\"\n\"a\"\n3L\n4L\n",
            "\"the condition has length > 1\"\n",
        ),
        "",
    );
}

#[test]
fn s3_methods_forward_original_calls_then_current_bindings() {
    check(
        r#"
describe <- function(x, ...) {
    hidden <- TRUE
    x <- 77L
    on.exit(cat("generic exit\n"))
    UseMethod("describe")
    stop("unreachable")
}
describe.one <- function(x, ...) {
    dput(exists("hidden"))
    dput(.Class)
    x <- 4L
    NextMethod()
}
describe.two <- function(x, label) {
    dput(.Class)
    list(x, label, .Generic, .Method)
}
dput(describe(structure(3L, class = c("skip", "one", "two")), label = "ok"))
Ops.unit <- function(e1, e2) {
    dput(.Method)
    NextMethod()
}
Math.unit <- function(x, ...) { dput(.Group); NextMethod() }
Summary.unit <- function(..., na.rm) { dput(missing(na.rm)); NextMethod() }
dput(structure(8L, class = "unit") + 3L)
dput(3L < structure(8L, class = "unit"))
dput(abs(structure(-3L, class = "unit")))
dput(sum(na.rm = TRUE, structure(c(1, NA_real_), class = "unit")))
dput(max(structure(c(1, NA_real_), class = "unit")))
"#,
        concat!(
            "FALSE\nstructure(c(\"one\", \"two\"), previous = c(\"skip\", \"one\", \"two\"))\n",
            "structure(\"two\", previous = structure(c(\"one\", \"two\"), previous = c(\"skip\", \n\"one\", \"two\")))\n",
            "generic exit\nlist(4L, \"ok\", \"describe\", \"describe.two\")\n",
            "c(\"Ops.unit\", \"\")\nstructure(11L, class = \"unit\")\n",
            "c(\"\", \"Ops.unit\")\nTRUE\n\"Math\"\nstructure(3L, class = \"unit\")\n",
            "FALSE\n1\nFALSE\nNA_real_\n",
        ),
        "",
    );
}

#[test]
fn environments_and_options_keep_values_across_safepoints() {
    check(
        r#"
e <- new.env(parent = baseenv())
assign("x", 3L, envir = e)
assign(".hidden", 7L, envir = e)
dput(ls(e))
dput(get("x", envir = e))
f <- function() x
environment(f) <- e
dput(f())
rm(x, envir = e)
dput(exists("x", envir = e, inherits = FALSE))
dput(base::sum(c(1L, 2L)))
dput(identical(topenv(environment()), globalenv()))
options(custom = list(4L))
dput(getOption("custom"))
dput(options(custom = NULL))
dput(getOption("custom", 10L))
f <- function() g()
g <- function() list(sys.call(), sys.call(-1), sys.nframe())
result <- f()
dput(result)
"#,
        concat!(
            "\"x\"\n3L\n3L\nFALSE\n3L\nTRUE\n",
            "list(4L)\nlist(custom = list(4L))\n10L\nlist(g(), f(), 2L)\n",
        ),
        "",
    );
}

#[test]
fn unbounded_r_recursion_returns_an_error() {
    let outcome = run_with_policy("f <- function() f()\nf()", GcPolicy::default());
    assert_eq!(outcome.status, 1);
    assert!(outcome.stderr.contains("evaluation nested too deeply"));
}

#[test]
fn ops_conflicts_use_the_choice_method_or_warn_and_use_the_primitive() {
    check(
        r#"
Ops.a <- function(e1, e2) { dput(.Group); dput(.Method); NextMethod() }
Ops.b <- function(e1, e2) "b"
chooseOpsMethod.a <- function(x, y, mx, my, cl, reverse) TRUE
a <- structure(3L, class = "a")
b <- structure(7L, class = "b")
dput(a + b)
dput(b + a)
rm(chooseOpsMethod.a)
withCallingHandlers(dput(a + b), warning = function(w) {
    dput(conditionMessage(w))
    invokeRestart("muffleWarning")
})
"#,
        concat!(
            "\"Ops\"\nc(\"Ops.a\", \"\")\nstructure(10L, class = \"a\")\n",
            "\"Ops\"\nc(\"\", \"Ops.a\")\nstructure(10L, class = \"b\")\n",
            "\"Incompatible methods (\\\"Ops.a\\\", \\\"Ops.b\\\") for \\\"+\\\"\"\n",
            "structure(10L, class = \"a\")\n",
        ),
        "",
    );
}

#[test]
fn eval_has_its_own_return_context_and_parse_can_keep_source() {
    check(
        r#"
g <- function() {
    dput(evalq(return(4L)))
    evalq(on.exit(cat("eval exit\n")))
    8L
}
dput(g())
p <- parse(text = c("x <- 1L", "x"), keep.source = TRUE)
dput(attr(p, "srcref")[[1L]][1:8])
dput(eval(p, new.env()))
"#,
        "4L\neval exit\n8L\nc(1L, 1L, 1L, 7L, 1L, 7L, 1L, 1L)\n1L\n",
        "",
    );
}

#[test]
fn apply_passes_evaluated_language_to_a_closure_as_data() -> crate::EvalResult<()> {
    use roxide_value::{GLOBAL_ENV, SEXP_NIL, SexpType, ValueApi};

    let mut ctx = crate::Ctx::new(Box::new(roxide_host::PureHost::new()), GcPolicy::Torture)
        .map_err(crate::Unwind::Internal)?;
    ctx.run_script(
        "<test>",
        "f <- function(x) x\nv <- quote(stop(\"must remain data\"))",
        GLOBAL_ENV,
        false,
    )?;
    let f = ctx.session_mut().intern("f")?;
    let v = ctx.session_mut().intern("v")?;
    let Some(f) = ctx.get_var(f, GLOBAL_ENV)? else {
        return Err(crate::Unwind::TopLevel);
    };
    let Some(v) = ctx.get_var(v, GLOBAL_ENV)? else {
        return Err(crate::Unwind::TopLevel);
    };
    let result = ctx.apply(f, &[(None, v)], SEXP_NIL, GLOBAL_ENV)?;
    assert_eq!(ctx.session().type_of(result)?, SexpType::Lang);
    assert_eq!(result, v);
    Ok(())
}

#[test]
fn value_errors_are_catchable() {
    check(
        r#"tryCatch(structure(1L, names = c("one", "two")), error = function(e) cat("caught\n"))"#,
        "caught\n",
        "",
    );
}

#[test]
fn active_bindings_read_call_and_write() {
    check(
        r#"
saved <- 3L
makeActiveBinding("active", function(value) {
    if (missing(value)) return(function() saved)
    saved <<- value
}, .GlobalEnv)
dput(active())
active <- 4L
dput(get("active", inherits = FALSE)())
dput(active())
"#,
        "3L\n4L\n4L\n",
        "",
    );
}

#[test]
fn non_escaping_promises_release_their_cached_values() -> crate::EvalResult<()> {
    use roxide_value::GLOBAL_ENV;

    for policy in [GcPolicy::default(), GcPolicy::Torture] {
        let mut ctx = crate::Ctx::new(Box::new(roxide_host::PureHost::new()), policy)
            .map_err(crate::Unwind::Internal)?;
        ctx.run_script(
            "<test>",
            "x <- c(1L, 2L); f <- function(arg) arg; for (i in 1:12) f(x)",
            GLOBAL_ENV,
            false,
        )?;
        let x = ctx.session_mut().intern("x")?;
        let Some(x) = ctx.get_var(x, GLOBAL_ENV)? else {
            return Err(crate::Unwind::TopLevel);
        };
        assert_eq!(ctx.session().heap.refcnt(x), 1);
        ctx.run_script(
            "<test>",
            "keep <- function(arg) { force(arg); function() arg }; escaped <- keep(x)",
            GLOBAL_ENV,
            false,
        )?;
        assert_eq!(ctx.call_function("escaped", &[], GLOBAL_ENV)?, x);
    }
    Ok(())
}

#[test]
fn s4_dispatch_can_call_another_s4_generic() -> crate::EvalResult<()> {
    use roxide_value::{GLOBAL_ENV, Sym, ValueApi};

    struct Hooks;

    impl crate::S4Hooks for Hooks {
        fn dispatch(
            &self,
            ctx: &mut crate::Ctx,
            generic: Sym,
            call: &crate::Call,
            args: &crate::Args,
        ) -> crate::EvalResult<Option<roxide_gc::SexpId>> {
            match ctx.session().sym_name(generic)? {
                "+" => ctx
                    .call_function("-", &[(None, args.get(0))], call.env)
                    .map(Some),
                "-" => Ok(Some(ctx.session_mut().scalar_int(42)?)),
                _ => Ok(None),
            }
        }
    }

    let mut ctx = crate::Ctx::new(Box::new(roxide_host::PureHost::new()), GcPolicy::Torture)
        .map_err(crate::Unwind::Internal)?;
    ctx.set_s4_hooks(&Hooks);
    let value = ctx.session_mut().scalar_int(1)?;
    ctx.session_mut().set_s4_flag(value, true)?;
    let result = ctx.call_function("+", &[(None, value)], GLOBAL_ENV)?;
    assert_eq!(ctx.session().int(result)?.to_vec(), [42]);
    Ok(())
}

#[test]
fn forcing_promises_preserves_the_visibility_of_the_first_evaluation() {
    check(
        r#"
f <- function(x) {
    dput(withVisible(x))
    dput(withVisible(x))
}
f(invisible(7L))
g <- function(...) dput(withVisible(..1))
g(invisible(8L))
"#,
        concat!(
            "list(value = 7L, visible = FALSE)\n",
            "list(value = 7L, visible = TRUE)\n",
            "list(value = 8L, visible = FALSE)\n",
        ),
        "",
    );
}

#[test]
fn exit_finalizers_run_once_and_keep_their_arguments_alive() {
    use crate::Ctx;
    use roxide_host::PureHost;
    use roxide_value::{GLOBAL_ENV, ValueApi};

    let mut ctx = Ctx::new(Box::new(PureHost::default()), GcPolicy::Torture).unwrap();
    ctx.run_global(
        "finalizers",
        r#"
result <- 0L
local({
    e <- new.env()
    assign("n", 5L, envir=e)
    reg.finalizer(e, function(e) result <<- result + get("n", envir=e), onexit=TRUE)
})
"#,
    )
    .unwrap();
    ctx.run_exit_finalizers();
    ctx.run_exit_finalizers();
    let result = ctx.session_mut().intern("result").unwrap();
    let value = ctx.get_var(result, GLOBAL_ENV).unwrap().unwrap();
    assert_eq!(ctx.session().int(value).unwrap().to_vec(), vec![5]);
}

#[test]
fn missing_namespaces_signal_the_package_error_class() {
    check(
        r#"
dput(tryCatch(roxideMissingPackage::value, error = function(e) class(e)))
"#,
        "c(\"packageNotFoundError\", \"error\", \"condition\")\n",
        "",
    );
}
