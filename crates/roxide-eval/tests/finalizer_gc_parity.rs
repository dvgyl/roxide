#![forbid(unsafe_code)]

//! Tests for ephemeron GC finalizers (`reg.finalizer` + `gc()`) and re-entrant stream sinks.

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::PureHost;
use roxide_value::{GLOBAL_ENV, ValueApi};

fn make_ctx(policy: GcPolicy) -> Ctx {
    Ctx::new(Box::new(PureHost::new()), policy).expect("create eval ctx")
}

#[test]
fn ephemeron_finalizer_fires_once_on_gc_even_when_closure_references_target() {
    let mut ctx = make_ctx(GcPolicy::default());
    let script = r#"
        ran_first <- FALSE
        ran_second <- FALSE
        seen <- FALSE
        err_msg <- "none"
        make_obj <- function() {
            e <- new.env(parent = emptyenv())
            assign("flag", TRUE, envir = e)
            # Closure captures `e` strongly, just like R6's finalize() captures `self`.
            f <- function(x) {
                tryCatch({
                    if (isTRUE(ran_first)) {
                        ran_second <<- TRUE
                    } else {
                        ran_first <<- TRUE
                    }
                    if (isTRUE(get("flag", envir = e))) {
                        if (isTRUE(get("flag", envir = x))) {
                            seen <<- TRUE
                        }
                    }
                }, error = function(c) {
                    err_msg <<- conditionMessage(c)
                })
            }
            reg.finalizer(e, f, onexit = FALSE)
            e
        }
        obj <- make_obj()
        gc()
        before_rm <- ran_first
        rm(obj)
        mat <- gc()
        after_first_gc <- ran_first
        gc()
        after_second_gc <- ran_second
    "#;
    ctx.run_script("ephemeron.R", script, GLOBAL_ENV, false)
        .expect("run ephemeron script");
    ctx.run_exit_finalizers();

    let get_lgl = |ctx: &mut Ctx, name: &str| {
        let sym = ctx.session_mut().intern(name).expect("intern");
        let val = ctx.session().env_get(GLOBAL_ENV, sym).expect(name).value;
        ctx.session().lgl(val).expect("lgl").get(0).get()
    };
    assert_eq!(get_lgl(&mut ctx, "before_rm"), Some(false));
    assert_eq!(get_lgl(&mut ctx, "after_first_gc"), Some(true));
    assert_eq!(get_lgl(&mut ctx, "after_second_gc"), Some(false));
    assert_eq!(get_lgl(&mut ctx, "ran_second"), Some(false));
    let err_sym = ctx.session_mut().intern("err_msg").expect("intern");
    let err_val = ctx
        .session()
        .env_get(GLOBAL_ENV, err_sym)
        .expect("err")
        .value;
    let err_str = std::str::from_utf8(
        ctx.session()
            .char_bytes(ctx.session().str_slice(err_val).expect("str")[0])
            .expect("bytes"),
    )
    .expect("utf8");
    assert_eq!(err_str, "none", "finalizer error: {err_str}");
    assert_eq!(get_lgl(&mut ctx, "seen"), Some(true));

    let mat_sym = ctx.session_mut().intern("mat").expect("intern");
    let mat = ctx
        .session()
        .env_get(GLOBAL_ENV, mat_sym)
        .expect("mat")
        .value;
    let dim_sym = ctx.session_mut().intern("dim").expect("intern");
    let dim = ctx.session().attr(mat, dim_sym);
    assert_eq!(ctx.session().int(dim).expect("dim").to_vec(), vec![2, 7]);
}

#[test]
fn onexit_finalizer_fires_at_exit_only_if_still_alive() {
    let mut ctx = make_ctx(GcPolicy::Torture);
    let script = r#"
        alive_ran <- FALSE
        dead_ran <- FALSE
        dead_twice <- FALSE
        alive <- new.env(parent = emptyenv())
        reg.finalizer(alive, function(e) { alive_ran <<- TRUE }, onexit = TRUE)
        dead <- new.env(parent = emptyenv())
        reg.finalizer(dead, function(e) {
            if (isTRUE(dead_ran)) dead_twice <<- TRUE
            dead_ran <<- TRUE
        }, onexit = TRUE)
        rm(dead)
        gc()
    "#;
    ctx.run_script("onexit.R", script, GLOBAL_ENV, false)
        .expect("run onexit script");

    let get_lgl = |ctx: &mut Ctx, name: &str| {
        let sym = ctx.session_mut().intern(name).expect("intern");
        let val = ctx.session().env_get(GLOBAL_ENV, sym).expect(name).value;
        ctx.session().lgl(val).expect("lgl").get(0).get()
    };
    assert_eq!(get_lgl(&mut ctx, "dead_ran"), Some(true));
    assert_eq!(get_lgl(&mut ctx, "alive_ran"), Some(false));

    ctx.run_exit_finalizers();
    assert_eq!(get_lgl(&mut ctx, "alive_ran"), Some(true));
    assert_eq!(get_lgl(&mut ctx, "dead_twice"), Some(false));
}

#[test]
fn reentrant_stream_sink_pop_during_flush_does_not_panic() {
    let mut ctx = make_ctx(GcPolicy::default());
    ctx.push_stream_sink(true, false, false, |ctx, _bytes| {
        let _ = ctx.pop_stream_sink(false)?;
        Ok(())
    });
    ctx.host().write_out(b"hello\n").expect("write out");
    ctx.flush_stream_sinks()
        .expect("re-entrant pop inside sink callback must not panic");
    assert!(!ctx.pop_stream_sink(false).expect("pop check"));
}
