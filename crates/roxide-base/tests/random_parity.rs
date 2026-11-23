//! Check base random functions through the evaluator with collection at every safepoint.

use roxide_eval::Ctx;
use roxide_gc::GcPolicy;
use roxide_host::PureHost;
use roxide_value::{GLOBAL_ENV, ValueApi};

fn run(script: &str) -> Ctx {
    let mut ctx = Ctx::new(Box::new(PureHost::new()), GcPolicy::Torture).unwrap();
    roxide_base::register_all(&mut ctx).unwrap();
    ctx.run_script("random_parity.R", script, GLOBAL_ENV, false)
        .unwrap();
    ctx
}

fn integers(ctx: &mut Ctx, name: &str) -> Vec<i32> {
    let symbol = ctx.session_mut().intern(name).unwrap();
    let value = ctx.session().env_get(GLOBAL_ENV, symbol).unwrap().value;
    ctx.session().int(value).unwrap().to_vec()
}

#[test]
fn sample_and_seed_restore_match_oracle() {
    let mut ctx = run(r#"
        set.seed(42L)
        old <- .Random.seed
        first <- sample.int(100L, 10L)
        after <- .Random.seed
        .Random.seed <- old
        draw <- function() {
            .Random.seed <- "local"
            sample.int(100L, 10L)
        }
        second <- draw()
    "#);
    assert_eq!(
        integers(&mut ctx, "first"),
        [49, 65, 25, 74, 18, 100, 47, 24, 71, 89]
    );
    assert_eq!(integers(&mut ctx, "first"), integers(&mut ctx, "second"));
    assert_eq!(
        integers(&mut ctx, "old")[..4],
        [10403, 624, 507561766, 1260545903]
    );
    assert_eq!(
        integers(&mut ctx, "after"),
        integers(&mut ctx, ".Random.seed")
    );
}

#[test]
fn query_has_no_seed_side_effect_and_pure_host_reseed_is_deterministic() {
    let mut ctx = run("kind <- RNGkind()");
    let symbol = ctx.session_mut().intern(".Random.seed").unwrap();
    assert!(ctx.session().env_get(GLOBAL_ENV, symbol).is_none());

    let mut first = run("set.seed(NULL); sample.int(100L, 5L)");
    let mut second = run("sample.int(100L, 5L)");
    assert_eq!(
        integers(&mut first, ".Random.seed"),
        integers(&mut second, ".Random.seed")
    );
}

#[test]
fn active_seed_and_seed_vector_use_the_global_stream() {
    let mut ctx = Ctx::new(Box::new(roxide_host::NativeHost::new()), GcPolicy::Torture).unwrap();
    roxide_base::register_all(&mut ctx).unwrap();
    ctx.run_script(
        "Active seed",
        r#"
set.seed(1L)
expected <- sample.int(100, 10)
set.seed(c(1, 2))
stopifnot(identical(sample.int(100, 10), expected))
set.seed(1L)
saved <- .Random.seed
rm(.Random.seed)
writes <- 0L
reads <- 0L
makeActiveBinding(".Random.seed", function(x) {
    if (missing(x)) {
        reads <<- reads + 1L
        return(saved)
    }
    saved <<- x
    writes <<- writes + 1L
}, .GlobalEnv)
actual <- sample.int(100, 10)
stopifnot(identical(actual, expected), writes == 1L, reads == 2L)
.Random.seed <- c(10401L, -1872166913L, 1179647999L)
draw <- sample.int(3, 1)
stopifnot(draw >= 1L, draw <= 3L)
"#,
        GLOBAL_ENV,
        false,
    )
    .unwrap();
}

#[test]
fn sampling_checks_compact_lengths_and_hash_storage() {
    let limits = roxide_host::Limits {
        max_vector_bytes: 128,
        ..roxide_host::Limits::default()
    };
    let mut ctx = Ctx::new(
        Box::new(roxide_host::NativeHost::with_limits(limits)),
        GcPolicy::Torture,
    )
    .unwrap();
    roxide_base::register_all(&mut ctx).unwrap();
    ctx.run_script(
        "Sample limits",
        r#"
err <- function(expr) tryCatch(expr, error = function(e) conditionMessage(e))
stopifnot(identical(err(sample.int(1, seq_len(32))), "invalid 'size' argument"))
stopifnot(identical(err(sample.int(100, 5, useHash=TRUE)),
                  "sampling allocation exceeds the vector size limit"))
stopifnot(!exists(".Random.seed", envir=.GlobalEnv, inherits=FALSE))
"#,
        GLOBAL_ENV,
        false,
    )
    .unwrap();
}
