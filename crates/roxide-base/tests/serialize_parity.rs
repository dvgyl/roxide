#![forbid(unsafe_code)]

//! Check XDR streams and workspace files against GNU R 4.6.2.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use roxide_eval::{
    ArgEval, Args, Arity, Call, Ctx, CtxExt, Dispatch, EvalResult, Module, PrimDef, PrimKind,
    Unwind, Visibility,
};
use roxide_gc::{GcPolicy, Trace, Tracer};
use roxide_host::{Host, Limits, NativeHost, PureHost};
use roxide_value::{DeparseOpts, GLOBAL_ENV, ValueApi, dput};

const CASES: &str = include_str!("../../../conformance/layer1/07_rds_rdata_roundtrip.rcase");

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static INDEX: AtomicUsize = AtomicUsize::new(0);
        let path = env::temp_dir().join(format!(
            "roxide-xdr-{}-{}",
            std::process::id(),
            INDEX.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("Create test directory");
        Self(path)
    }

    fn path(&self, name: &str) -> String {
        format!("{:?}", self.0.join(name).to_str().expect("UTF-8 test path"))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct Output(String);

impl Trace for Output {
    fn trace(&self, _: &mut Tracer<'_>) {}
}

impl CtxExt for Output {}

fn capture(ctx: &mut Ctx, _: &Call, args: &mut Args) -> EvalResult {
    let value = args.get(0);
    for line in dput(ctx.session(), value, DeparseOpts::default())? {
        let out = &mut ctx.ext::<Output>().0;
        out.push_str(&line);
        out.push('\n');
    }
    Ok(value)
}

const CAPTURE: Module = Module {
    prims: &[PrimDef {
        name: "dput",
        fun: capture,
        kind: PrimKind::Primitive,
        args: ArgEval::Eval,
        arity: Arity::Exact(1),
        dispatch: Dispatch::None,
        visibility: Visibility::Off,
    }],
    prelude: "",
};

fn context(host: Box<dyn Host>) -> Ctx {
    let mut ctx = Ctx::new(host, GcPolicy::Torture).expect("Create R session");
    roxide_base::register_all(&mut ctx).expect("Register base modules");
    ctx
}

fn run(ctx: &mut Ctx, name: &str, script: &str) {
    ctx.run_script(name, script, GLOBAL_ENV, false)
        .unwrap_or_else(|error| panic!("{name}: {error:?}"));
}

fn oracle(executable: &Path, code: &str) -> String {
    let output = Command::new(executable)
        .args(["--vanilla", "-e", code])
        .env("R_ENABLE_JIT", "0")
        .output()
        .expect("Run GNU R 4.6.2");
    assert!(
        output.status.success(),
        "GNU R: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("Read GNU R output")
}

#[test]
fn layer_one_serialization_cases_match_gnu_r() {
    let mut total = 0;
    for case in CASES.split("=== case: ").skip(1) {
        let (header, script) = case.split_once('\n').expect("Read case header");
        let name = header.strip_suffix(" ===").expect("Read case name");
        let expected = match name {
            "serialize_null_in_both_versions" => "\"raw\"\nTRUE\n\"raw\"\nTRUE\n",
            "serialize_logical_and_integer_vectors"
            | "serialize_pairlists_and_calls"
            | "save_rds_and_read_rds_with_each_compression" => "TRUE\nTRUE\nTRUE\nTRUE\n",
            "serialize_double_special_values_and_negative_zero" => {
                "TRUE\nc(-Inf, Inf)\nTRUE\nc(-Inf, Inf)\n"
            }
            "serialize_utf8_and_missing_strings" => concat!(
                "TRUE\nc(\"UTF-8\", \"UTF-8\", \"unknown\", \"unknown\")\n",
                "TRUE\nc(\"UTF-8\", \"UTF-8\", \"unknown\", \"unknown\")\n",
            ),
            "serialize_compact_integer_sequence" => {
                "TRUE\nc(100L, 1L, 42L)\nTRUE\nc(100L, 1L, 42L)\n"
            }
            "serialize_closures_with_shared_lexical_environment" => concat!(
                "TRUE\nTRUE\nTRUE\nc(6L, 8L, 8L, 6L)\n",
                "TRUE\nTRUE\nTRUE\nc(6L, 8L, 8L, 6L)\n",
            ),
            "save_and_load_objects_in_fresh_environment" => {
                "c(\"notes\", \"counts\")\nc(\"counts\", \"notes\")\nTRUE\nTRUE\n"
            }
            _ => "TRUE\nTRUE\n",
        };
        let mut ctx = context(Box::new(PureHost::new()));
        ctx.register_module(&CAPTURE)
            .expect("Register output capture");
        // Use a fresh virtual file system for each case.
        let script = script
            .replace("tempfile()", "\"/roundtrip\"")
            .replace("    on.exit(unlink(path))\n", "");
        run(&mut ctx, name, &script);
        assert_eq!(ctx.ext::<Output>().0, expected, "{name}");
        total += 1;
    }
    assert_eq!(total, 15);
}

#[test]
fn cross_engine_rds_and_workspace_roundtrips() {
    let Some(executable) = env::var_os("ROXIDE_ORACLE_RSCRIPT") else {
        return;
    };
    let executable = Path::new(&executable);
    let dir = TempDir::new();
    let mut prepare = String::from(
        r#"
stopifnot(as.character(getRversion()) == "4.6.2")
e <- new.env(parent = baseenv())
e$total <- 6L
e$self <- e
e$step <- 2L
delayedAssign("lazy", total + 10L, eval.env=e, assign.env=e)
delayedAssign("forced", total + 20L, eval.env=e, assign.env=e)
invisible(e$forced)
lockBinding("step", e)
lockEnvironment(e)
fun <- evalq(function() {
    step_value <- function() step
    total <<- total + step_value()
    total
}, e)
get_total <- evalq(function() total, e)
obj <- list(
    values = c(NA_real_, NaN, -0.0, Inf, -Inf),
    strings = c("caf\u00e9", "\u754c", "", NA_character_,
                iconv("caf\u00e9", "UTF-8", "latin1")),
    complex = complex(real = c(1, NA_real_, 0), imaginary = c(-2, 4, Inf)),
    raw = as.raw(c(0L, 1L, 128L, 255L)),
    compact = 1:100,
    reverse = 2147483651:2147483648,
    data = data.frame(x = c(9L, NA_integer_), y = c("ridge", "shore")),
    factor = ordered(c("medium", NA_character_, "low"), levels = c("low", "medium", "high")),
    object = structure(c(first = 12.5, second = 14), class = c("sample_mass", "numeric"), unit = "g"),
    pair = pairlist(site = "shore", action = quote(combine(site, , count = 3L))),
    expr = expression(x + 1L, y),
    methods = list(add = compiler::cmpfun(fun), get_total = compiler::cmpfun(get_total)),
    primitives = list(.Primitive("+"), .Primitive("if")),
    extra = list(.GlobalEnv, emptyenv(), baseenv(), .BaseNamespaceEnv)
)
alpha <- obj
beta <- obj$methods
"#,
    );
    let mut roundtrip = String::from(
        r#"
verify <- function(obj) {
    scope <- environment(obj$methods$add)
    stopifnot(identical(scope, environment(obj$methods$get_total)))
    stopifnot(identical(scope, scope$self))
    stopifnot(identical(obj$methods$get_total(), 6L))
}
"#,
    );
    let mut verify = String::from(
        r#"
verify <- function(before, after) {
    old <- environment(before$methods$add)
    new <- environment(after$methods$add)
    stopifnot(identical(new, environment(after$methods$get_total)), identical(new, new$self))
    stopifnot(identical(parent.env(old), parent.env(new)), environmentIsLocked(new))
    stopifnot(bindingIsLocked("step", new), identical(ls(old), ls(new)))
    stopifnot(identical(old$total, new$total), identical(old$step, new$step))
    environment(before$methods$add) <- new
    environment(before$methods$get_total) <- new
    stopifnot(identical(before, after, num.eq = FALSE, single.NA = FALSE))
    stopifnot(identical(after$methods$get_total(), 6L), identical(after$methods$add(), 8L))
    stopifnot(identical(new$lazy, 18L), identical(new$forced, 26L))
}
"#,
    );
    for version in [2, 3] {
        for codec in ["FALSE", "\"gzip\"", "\"bzip2\"", "\"xz\"", "\"zstd\""] {
            let label = format!("{version}-{}", codec.trim_matches('"'));
            let rds = dir.path(&format!("gnu-{label}.rds"));
            let rdata = dir.path(&format!("gnu-{label}.RData"));
            let out_rds = dir.path(&format!("rox-{label}.rds"));
            let out_rdata = dir.path(&format!("rox-{label}.RData"));
            prepare.push_str(&format!(
                "saveRDS(obj, {rds}, version={version}, compress={codec})\n\
                 save(alpha, beta, file={rdata}, version={version}, compress={codec})\n"
            ));
            roundtrip.push_str(&format!(
                "obj <- readRDS({rds})\nverify(obj)\nsaveRDS(obj, {out_rds}, version={version}, compress={codec})\n\
                 e <- new.env(parent=emptyenv())\nresult <- withVisible(load({rdata}, envir=e))\n\
                 stopifnot(identical(result$value, c(\"alpha\", \"beta\")), identical(result$visible, FALSE))\n\
                 verify(e$alpha)\nstopifnot(identical(environment(e$alpha$methods$add), environment(e$beta$add)))\n\
                 save(alpha, list=\"beta\", file={out_rdata}, envir=e, version={version}, compress={codec})\n"
            ));
            verify.push_str(&format!(
                "verify(readRDS({rds}), readRDS({out_rds}))\n\
                 old <- new.env()\nnew <- new.env()\n\
                 stopifnot(identical(load({rdata}, envir=old), load({out_rdata}, envir=new)))\n\
                 stopifnot(identical(environment(new$alpha$methods$add), environment(new$beta$add)))\n\
                 verify(old$alpha, new$alpha)\n"
            ));
        }
    }
    oracle(executable, &prepare);
    run(
        &mut context(Box::new(NativeHost::new())),
        "cross-engine roundtrip",
        &roundtrip,
    );
    oracle(executable, &verify);
}

#[test]
fn xdr_bytes_match_gnu_r() {
    let Some(executable) = env::var_os("ROXIDE_ORACLE_RSCRIPT") else {
        return;
    };
    let executable = Path::new(&executable);
    let dir = TempDir::new();
    let cases = r#"
scope <- new.env(parent = baseenv(), hash = FALSE)
scope$x <- 4L
scope$y <- scope
objects <- list(
    NULL, c(TRUE, NA, FALSE), c(1L, NA_integer_), c(NA_real_, NaN, -0.0, Inf),
    complex(real = c(1, NA_real_), imaginary = c(-2, 4)),
    c("caf\u00e9", "", NA_character_), as.raw(c(0L, 1L, 255L)), 1:100,
    structure(1.25, class = "test", unit = "m"),
    pairlist(x = quote(f(x,, x = 1L)), y = scope), list(scope, scope),
    evalq(function(a = 1L) x + a, scope)
)
"#;
    let mut gnu = cases.to_owned();
    let mut rox = cases.to_owned();
    let mut files = Vec::new();
    for v in [2, 3] {
        for i in 1..=12 {
            let r = format!("{v}-{i}-gnu.rds");
            let x = format!("{v}-{i}-rox.rds");
            gnu.push_str(&format!(
                "saveRDS(objects[[{i}]], {}, version={v}, compress=FALSE)\n",
                dir.path(&r)
            ));
            rox.push_str(&format!(
                "saveRDS(objects[[{i}]], {}, version={v}, compress=FALSE)\n",
                dir.path(&x)
            ));
            files.push((r, x));
        }
    }
    oracle(executable, &gnu);
    run(&mut context(Box::new(NativeHost::new())), "XDR bytes", &rox);
    for (r, x) in files {
        assert_eq!(
            fs::read(dir.0.join(&r)).unwrap(),
            fs::read(dir.0.join(x)).unwrap(),
            "{r}"
        );
    }
}

#[test]
fn rejects_corrupt_streams_and_resource_limit_violations() {
    let dir = TempDir::new();
    let path = dir.path("bounded.rds");
    let good = format!("saveRDS(rep(1L, 2048L), {path})");
    run(
        &mut context(Box::new(NativeHost::new())),
        "gzip fixture",
        &good,
    );
    let limits = Limits {
        max_decompress_bytes: 256,
        max_vector_bytes: 128,
        ..Limits::default()
    };
    let mut ctx = context(Box::new(NativeHost::with_limits(limits)));
    let result = ctx.run_script("gzip limit", &format!("readRDS({path})"), GLOBAL_ENV, false);
    assert!(matches!(result, Err(Unwind::TopLevel)), "{result:?}");

    let cases = vec![
        vec![],
        b"X\n\0".to_vec(),
        xdr_body(&[0x1ff]),
        xdr_body(&[255, 0]),
        xdr_body(&[13, 33]),
        xdr_body(&[14, u32::MAX, u32::MAX, u32::MAX]),
        xdr_body(&[4, 0, 0x1ff, 254, 254, 254]),
    ];
    for bytes in cases {
        rejects_raw(&mut ctx, &bytes);
    }

    let mut nested = [19, 1].repeat(180);
    nested.push(254);
    rejects_raw(&mut context(Box::new(PureHost::new())), &xdr_body(&nested));
}

fn rejects_raw(ctx: &mut Ctx, bytes: &[u8]) {
    let input = ctx.session_mut().intern("input").unwrap();
    let raw = ctx.session_mut().alloc_raw_from(bytes).unwrap();
    ctx.assign(input, raw, GLOBAL_ENV).unwrap();
    let result = ctx.run_script("invalid XDR", "unserialize(input)", GLOBAL_ENV, false);
    assert!(matches!(result, Err(Unwind::TopLevel)), "{result:?}");
}

fn xdr_body(words: &[u32]) -> Vec<u8> {
    let mut bytes = b"X\n\0\0\0\x02\0\x04\x06\x02\0\x02\x03\0".to_vec();
    bytes.extend(words.iter().flat_map(|n| n.to_be_bytes()));
    bytes
}
