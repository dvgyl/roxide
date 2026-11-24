//! Check register VM behavior against AST evaluation and known results.

use roxide_eval::vm::{compile_expr, is_compiled, run_compiled};
use roxide_eval::{
    ArgEval, Args, Arity, Call, Ctx, Dispatch, EvalResult, Module, PrimDef, PrimKind, S4Hooks,
    Visibility,
};
use roxide_gc::{GcPolicy, SexpId};
use roxide_host::PureHost;
use roxide_value::{GLOBAL_ENV, RLogical, SEXP_NIL, SexpType, Sym, ValueApi};

fn session(policy: GcPolicy, level: u8) -> Ctx {
    let mut ctx = Ctx::new(Box::new(PureHost::new()), policy).unwrap();
    ctx.set_jit_level(level);
    ctx
}

fn script(ctx: &mut Ctx, source: &str) {
    ctx.run_global("<vm parity>", source).unwrap();
}

fn get(ctx: &mut Ctx, name: &str) -> SexpId {
    let sym = ctx.session_mut().intern(name).unwrap();
    ctx.get_var(sym, GLOBAL_ENV).unwrap().unwrap()
}

fn result(ctx: &mut Ctx) -> String {
    let value = get(ctx, "result");
    ctx.deparse_lines(
        value,
        roxide_value::DeparseOpts {
            width_cutoff: usize::MAX,
            ..Default::default()
        },
    )
    .unwrap()
    .join("\n")
}

fn parity(source: &str, expected: &str) {
    for policy in [GcPolicy::default(), GcPolicy::Torture] {
        for level in [0, 3] {
            let mut ctx = session(policy, level);
            script(&mut ctx, source);
            assert_eq!(result(&mut ctx), expected, "JIT {level}, {policy:?}");
        }
    }
}

#[test]
fn scalar_arithmetic_and_comparison_preserve_types_and_missing_values() {
    parity(
        r#"
        compute <- function() {
            a <- 2147483647L + 1L
            b <- -2147483647L - 1L
            c <- 1073741824L * 2L
            div <- 4L / 2L
            na <- NA_real_ + NaN
            nan <- 0 / 0
            p <- NA_real_^0
            q <- 1^NA_real_
            r <- 0^0
            pos <- +TRUE
            neg <- -3L
            not <- !NA
            lt <- 1L < 2
            le <- 1 <= 1
            gt <- 2L > 1
            ge <- 2L >= 2
            eq <- 2L == 2
            ne <- 1L != 2L
            cmp <- NaN == 1
            list(c(a, b, c), div, na, nan, c(p, q, r), pos, neg, not,
                 c(lt, le, gt, ge, eq, ne, cmp))
        }
        warn <- function(w) {
            warnings <<- c(warnings, conditionMessage(w))
            invokeRestart("muffleWarning")
        }
        warnings <- NULL
        withCallingHandlers(compute(), warning = warn)
        warnings <- NULL
        values <- withCallingHandlers(compute(), warning = warn)
        result <- list(values, length(warnings), warnings[[1L]])
        "#,
        concat!(
            "list(list(c(NA_integer_, NA_integer_, NA_integer_), 2, NA_real_, NaN, ",
            "c(1, 1, 1), 1L, -3L, NA, c(TRUE, TRUE, TRUE, TRUE, TRUE, TRUE, NA)), ",
            "3L, \"NAs produced by integer overflow\")"
        ),
    );
}

#[test]
fn real_power_matches_r_at_zero_and_infinity() {
    parity(
        r#"
        powers <- function() {
            c((-0)^-1, (-0)^3, 1 / ((-0)^3), (-2)^Inf, (-1)^Inf,
              (-1)^(-Inf), (-Inf)^0.5, (-Inf)^(-0.5), (-Inf)^3,
              (-Inf)^2, 1 / ((-Inf)^-3), Inf^-1, Inf^0.5, (-2)^0.5,
              NA_real_^NaN, NaN^NA_real_, 1^NaN, NaN^0)
        }
        powers()
        result <- powers()
        "#,
        concat!(
            "c(Inf, 0, Inf, NaN, NaN, NaN, NaN, NaN, -Inf, Inf, Inf, 0, Inf, ",
            "NaN, NA, NA, 1, 1)"
        ),
    );
}

#[test]
fn compiled_calls_respect_the_expression_depth_limit() {
    for (depth, expected) in [
        (25, "1"),
        (
            110,
            "\"evaluation nested too deeply: infinite recursion / options(expressions=)?\"",
        ),
    ] {
        let body = format!("{}1{}", "(".repeat(depth), ")".repeat(depth));
        parity(
            &format!(
                r#"
                options(expressions = 100)
                f <- function() {body}
                tryCatch(f(), error = conditionMessage)
                result <- tryCatch(f(), error = conditionMessage)
                "#
            ),
            expected,
        );
    }
}

#[test]
fn subscript_replacement_preserves_function_literals() {
    parity(
        r#"
        f <- function() { x <- 1L; x[1L] <- 2L; x }
        before <- deparse(body(f))
        first <- f()
        second <- f()
        result <- list(first, second, identical(deparse(body(f)), before))
        "#,
        "list(2L, 2L, TRUE)",
    );
}

#[test]
fn arithmetic_dispatch_uses_values_and_original_argument_expressions() {
    parity(
        r#"
        `+.rx_ops` <- function(x, y) list(substitute(y), y)
        f <- function() {
            n <- 0L
            x <- structure(1:2, class = "rx_ops")
            y <- x + { n <- n + 1L; 2L }
            a <- c(3L, 4L)
            b <- a + { a[1L] <- 9L; 1L }
            list(y[[1L]], y[[2L]], n, a, b)
        }
        f()
        result <- f()
        "#,
        "list({\n    n <- n + 1L\n    2L\n}, 2L, 1L, c(9L, 4L), 4:5)",
    );
}

#[test]
fn loops_resume_transfers_and_keep_compact_sequences() {
    parity(
        r#"
        f <- function() {
            total <- 0L
            for (i in 1:100000) {
                if (i == 1L) eval(quote(next))
                if (i == 4L) eval(quote(break))
                total <- total + i
            }
            while (total < 9L) total <- total + 1L
            repeat {
                total <- total + 1L
                break
            }
            for (i in 3:1) total <- total + i
            for (x in pairlist(1L, 2L)) total <- total + x
            total
        }
        result <- f()
        "#,
        "19L",
    );
}

#[test]
fn top_level_for_visits_compact_integer_and_double_sequences() {
    for policy in [GcPolicy::default(), GcPolicy::Torture] {
        let len = if policy == GcPolicy::Torture {
            123
        } else {
            100_000
        };
        for level in [0, 3] {
            let mut ctx = session(policy, level);
            let seq = ctx.session_mut().alloc_compact_intseq(1, len, 1).unwrap();
            let sym = ctx.session_mut().intern("integer_seq").unwrap();
            ctx.assign(sym, seq, GLOBAL_ENV).unwrap();
            let seq = ctx
                .session_mut()
                .alloc_compact_realseq(3_000_000_000.0, 3, -1.0)
                .unwrap();
            let sym = ctx.session_mut().intern("real_seq").unwrap();
            ctx.assign(sym, seq, GLOBAL_ENV).unwrap();
            script(
                &mut ctx,
                "for (i in integer_seq) {}; for (j in real_seq) {}",
            );
            let last = get(&mut ctx, "i");
            assert_eq!(ctx.session().int(last).unwrap().get(0), len as i32);
            let last = get(&mut ctx, "j");
            assert_eq!(ctx.session().real(last).unwrap().get(0), 2_999_999_998.0);
        }
    }
}

#[test]
fn loops_release_their_frames_on_errors_and_returns() {
    parity(
        r#"
        f <- function() {
            for (x in list(1L, 2L)) {
                if (x == 1L) return(z <<- 42L)
            }
        }
        fail <- function() {
            i <- 9L
            err <- tryCatch(for (i in environment()) {}, error = conditionMessage)
            retry <- tryCatch(eval(quote(next)), error = conditionMessage)
            list(i, err, retry)
        }
        f()
        fail()
        result <- list(withVisible(f()), z, fail())
        "#,
        concat!(
            "list(list(value = 42L, visible = FALSE), 42L, list(NULL, \"invalid for() loop sequence\", ",
            "\"no loop for break/next, jumping to top level\"))"
        ),
    );
}

#[test]
fn compiled_transfers_keep_side_effects_and_release_top_level_loops() {
    for policy in [GcPolicy::default(), GcPolicy::Torture] {
        let mut ctx = session(policy, 3);
        script(
            &mut ctx,
            r#"
            expr <- quote(for (i in 1:3) { return(done <- i) })
            retry <- quote(next)
        "#,
        );
        let expr = get(&mut ctx, "expr");
        let code = compile_expr(ctx.session_mut(), expr).unwrap();
        assert!(run_compiled(&mut ctx, &code, GLOBAL_ENV).is_err());
        let done = get(&mut ctx, "done");
        assert_eq!(ctx.session().int(done).unwrap().get(0), 1);
        let retry = get(&mut ctx, "retry");
        assert!(matches!(
            ctx.eval(retry, GLOBAL_ENV),
            Err(roxide_eval::Unwind::TopLevel)
        ));
    }
}

#[test]
fn subscript_dispatch_keeps_indices_lazy_and_preserves_copy_on_write() {
    parity(
        r#"
        `[.rx_lazy` <- function(x, i) substitute(i)
        `[[.rx_lazy` <- function(x, i) 42L
        f <- function() {
            a <- structure(1:3, class = "rx_lazy")
            b <- c(1L, 2L)
            copy <- b
            b[1L] <- 8L
            b[[2L]] <- 9L
            list(a[stop("index")], a[[stop("index")]], copy, b[1L], b[[2L]])
        }
        f()
        result <- f()
        "#,
        "list(stop(\"index\"), 42L, 1:2, 8L, 9L)",
    );
}

struct LazyS4;

impl S4Hooks for LazyS4 {
    fn dispatch(
        &self,
        ctx: &mut Ctx,
        generic: Sym,
        _: &Call,
        args: &Args,
    ) -> EvalResult<Option<SexpId>> {
        assert_eq!(ctx.session().sym_name(generic)?, "[");
        assert_eq!(ctx.session().type_of(args.get(1))?, SexpType::Prom);
        Ok(Some(ctx.session_mut().scalar_int(42)?))
    }
}

#[test]
fn subscript_dispatch_enters_s4_before_evaluating_the_index() {
    for policy in [GcPolicy::default(), GcPolicy::Torture] {
        for level in [0, 3] {
            let mut ctx = session(policy, level);
            ctx.set_s4_hooks(&LazyS4);
            let value = ctx.session_mut().scalar_int(1).unwrap();
            ctx.session_mut().set_s4_flag(value, true).unwrap();
            let sym = ctx.session_mut().intern("x").unwrap();
            ctx.assign(sym, value, GLOBAL_ENV).unwrap();
            script(
                &mut ctx,
                "f <- function() x[stop('forced')]; f(); result <- f()",
            );
            assert_eq!(result(&mut ctx), "42L");
            let fun = get(&mut ctx, "f");
            assert_eq!(is_compiled(&ctx, fun), level == 3);
        }
    }
}

#[test]
fn replacement_reuses_unshared_storage_for_both_subscripts() {
    for policy in [GcPolicy::default(), GcPolicy::Torture] {
        let mut ctx = session(policy, 3);
        script(
            &mut ctx,
            "x <- c(3L, 4L); expr <- quote({x[1L] <- 9L; x[[2L]] <- 8L})",
        );
        let value = get(&mut ctx, "x");
        let expr = get(&mut ctx, "expr");
        let code = compile_expr(ctx.session_mut(), expr).unwrap();
        let rhs = run_compiled(&mut ctx, &code, GLOBAL_ENV).unwrap();
        assert_eq!(ctx.session().int(rhs).unwrap().get(0), 8);
        assert!(!ctx.is_visible());
        assert_eq!(get(&mut ctx, "x"), value);
        assert_eq!(ctx.session().int(value).unwrap().to_vec(), [9, 8]);
    }
}

#[test]
fn caches_respect_rebinding_promises_and_active_bindings() {
    parity(
        r#"
        make <- function() {
            x <- 1L
            add <- function(n) x + n
            add(1L); add(1L)
            x <- 20L
            reread <- add(2L)
            `+` <- function(a, b) 99L
            shadow <- add(stop("lazy"))
            `+` <- 5L
            after <- add(3L)
            local_plus <- function(`+`) 1L + stop("lazy")
            local_plus(function(a, b) 7L)
            promised <- local_plus(function(a, b) 7L)
            forced_plus <- function(`+`) {
                force(`+`)
                1L + 2L
            }
            forced_plus(base::`+`)
            forced <- c(forced_plus(base::`+`), forced_plus(function(a, b) 9L))
            remove("+", envir = environment())
            makeActiveBinding("+", function() function(a, b) 8L, environment())
            active <- add(stop("lazy"))
            c(reread, shadow, after, promised, forced, active)
        }
        make()
        result <- make()
        "#,
        "c(22L, 99L, 23L, 7L, 3L, 9L, 8L)",
    );
}

#[test]
fn guards_check_local_shadows_inside_a_running_loop() {
    parity(
        r#"
        f <- function() {
            x <- c(1L, 2L)
            y <- 0L
            for (i in 1:3) {
                y <- y + x[1L]
                if (i == 1L) {
                    `[` <- function(x, i) 5L
                    `[<-` <- function(x, i, value) c(3L, value)
                    x[stop("unused")] <- 8L
                }
                if (i == 2L) {
                    `if` <- function(...) 99L
                    `for` <- function(...) 77L
                }
            }
            c(y, x, for (i in stop("unused")) stop("unused"))
        }
        result <- f()
        "#,
        "c(11L, 3L, 8L, 77L)",
    );
}

fn short(ctx: &mut Ctx, call: &Call, args: &mut Args) -> EvalResult {
    let (head, _, _) = ctx.session().cons_parts(call.expr)?;
    let and = ctx.session().sym_name_of(head)? == "&&";
    let mut result = RLogical::from_option(Some(and));
    for arg in args.values() {
        let value = ctx.eval(*arg, call.env)?;
        let len = ctx.session().xlength(value)?;
        if len > 1 {
            return Err(ctx.stop(call.expr, "invalid length"));
        }
        let current = if len == 0 {
            RLogical::NA
        } else {
            let (value, _) = ctx.session_mut().coerce(value, SexpType::Lgl)?;
            ctx.session().lgl(value)?.get(0)
        };
        result = if and {
            result.and(current)
        } else {
            result.or(current)
        };
        if result.get() == Some(!and) {
            break;
        }
    }
    Ok(ctx.session_mut().scalar_lgl(result)?)
}

const LOGICAL: Module = Module {
    prims: &[
        PrimDef {
            name: "&&",
            kind: PrimKind::Primitive,
            args: ArgEval::Unevaluated,
            arity: Arity::Exact(2),
            dispatch: Dispatch::None,
            visibility: Visibility::On,
            fun: short,
        },
        PrimDef {
            name: "||",
            kind: PrimKind::Primitive,
            args: ArgEval::Unevaluated,
            arity: Arity::Exact(2),
            dispatch: Dispatch::None,
            visibility: Visibility::On,
            fun: short,
        },
    ],
    prelude: "",
};

#[test]
fn short_circuit_preserves_na_and_lazy_side_effects() {
    for policy in [GcPolicy::default(), GcPolicy::Torture] {
        for level in [0, 3] {
            let mut ctx = session(policy, level);
            ctx.register_module(&LOGICAL).unwrap();
            script(
                &mut ctx,
                r#"
                f <- function() {
                    n <- 0L
                    a <- FALSE && stop("forced")
                    b <- TRUE || stop("forced")
                    c <- FALSE[0L] && {n <- n + 1L; FALSE}
                    d <- NA || {n <- n + 1L; TRUE}
                    e <- TRUE && FALSE[0L]
                    tryCatch(c(TRUE, FALSE) && stop("forced"), error = conditionMessage)
                    list(c(a, b, c, d, e), n)
                }
                f()
                result <- f()
            "#,
            );
            assert_eq!(
                result(&mut ctx),
                "list(c(FALSE, TRUE, FALSE, TRUE, NA), 2L)"
            );
        }
    }
}

#[test]
fn jit_compiles_loops_on_call_one_and_other_bodies_on_call_two() {
    for policy in [GcPolicy::default(), GcPolicy::Torture] {
        for level in 0..=3 {
            let mut ctx = session(policy, level);
            script(
                &mut ctx,
                "f <- function(x) x + 1L; loop <- function() for (i in 3:1) {}",
            );
            let f = get(&mut ctx, "f");
            let loop_fun = get(&mut ctx, "loop");
            assert!(!is_compiled(&ctx, f));
            assert!(!is_compiled(&ctx, loop_fun));
            script(&mut ctx, "f(1L); loop()");
            assert!(!is_compiled(&ctx, f));
            assert_eq!(is_compiled(&ctx, loop_fun), level != 0);
            script(&mut ctx, "f(2L)");
            assert_eq!(is_compiled(&ctx, f), level != 0);
            assert!(!is_compiled(&ctx, SEXP_NIL));

            let mut other = session(policy, 0);
            script(&mut other, "f <- function(x) x + 1L");
            let other_f = get(&mut other, "f");
            assert!(!is_compiled(&other, other_f));
        }
    }
}
