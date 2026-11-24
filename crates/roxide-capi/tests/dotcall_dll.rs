//! Compiled C fixtures for default and torture garbage collection.

use std::ffi::{c_char, c_void};
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use roxide_capi::abi::Sexp;
use roxide_capi::api::{coerce, eval, extptr, memory, pairlist, sym, vector, weakref};
use roxide_capi::{globals, hooks, jump, register};
use roxide_eval::{Args, Call, Ctx, DotCOpts, EvalResult, NamespaceLoader, NativeCallKind};
use roxide_gc::{GcPolicy, SexpId};
use roxide_host::NativeHost;
use roxide_value::{
    CharId, GLOBAL_ENV, NA_INTEGER, PrimId, RComplex, RLogical, SEXP_NIL, SYM_NAMES, ValueApi,
};

const FIXTURE: &str = r#"
#include <R.h>
#include <Rinternals.h>
#include <R_ext/Rdynload.h>

static int *events;
static SEXP preserved;

__attribute__((constructor)) static void initialize_image(void) {
    preserved = Rf_ScalarInteger(97);
    R_PreserveObject(preserved);
}
__attribute__((destructor)) static void release_image(void) {
    R_ReleaseObject(preserved);
    if (events) events[2]++;
}
static SEXP preserved_value(void) { return preserved; }
static SEXP zero(void) { return Rf_ScalarInteger(42); }
static SEXP sum2(SEXP a, SEXP b) {
    SEXP result = PROTECT(Rf_ScalarInteger(INTEGER(a)[0] + INTEGER(b)[0]));
    UNPROTECT(1);
    return result;
}
static SEXP sum10(SEXP a, SEXP b, SEXP c, SEXP d, SEXP e, SEXP f,
                  SEXP g, SEXP h, SEXP i, SEXP j) {
    return Rf_ScalarInteger(INTEGER(a)[0] + INTEGER(b)[0] + INTEGER(c)[0] +
        INTEGER(d)[0] + INTEGER(e)[0] + INTEGER(f)[0] +
        INTEGER(g)[0] + INTEGER(h)[0] + INTEGER(i)[0] + INTEGER(j)[0]);
}
static SEXP null_pointer(void) { return NULL; }
static SEXP fail(void) {
    PROTECT(Rf_ScalarInteger(1));
    Rf_error("fixture error");
    return NULL;
}
static SEXP unbalanced(void) { return PROTECT(Rf_ScalarInteger(4)); }
static SEXP external(SEXP args) {
    if (TAG(CDR(args)) != Rf_install("argument") || CDR(CDR(args)) != R_NilValue)
        Rf_error("wrong .External tags");
    return CAR(args);
}
static SEXP external2(SEXP call, SEXP op, SEXP args, SEXP rho) {
    if (call != R_NilValue || op != R_NilValue || rho != R_GlobalEnv)
        Rf_error("wrong .External2 call context");
    return external(args);
}
static void mutate(int *value, double *other) { *value += 3; *other += 0.5; }
static void ignore(void *value) { (void)value; }
static void text(char **value) { value[0][0] = 'Y'; }
static void f_text(char *value) { value[0] = 'Y'; }
static void over(unsigned char *value) { value[1] = 0; }
static void under(unsigned char *value) { value[-1] = 0; }
void dynamic_value_(int *value) { *value += 10; }

static SEXP callable(void) { return Rf_ScalarInteger(91); }
static SEXP use_callable(void) {
    SEXP (*fun)(void) = (SEXP (*)(void)) R_GetCCallable("wave", "callable");
    return fun();
}
static SEXP install_callable(void) {
    R_RegisterCCallable("wave", "lazy", (DL_FUNC) callable);
    return R_NilValue;
}
static void finalize(SEXP target) {
    (void)target;
    events[0] += events[1] == 1 ? 1 : -100;
}
static SEXP finalizer_address(void) {
    return R_MakeExternalPtr((void *)finalize, R_NilValue, R_NilValue);
}
static SEXP counters(SEXP address) { events = R_ExternalPtrAddr(address); return R_NilValue; }
static SEXP make_ext(void) {
    SEXP result = PROTECT(R_MakeExternalPtr(NULL, R_NilValue, R_NilValue));
    R_RegisterCFinalizerEx(result, finalize, TRUE);
    UNPROTECT(1);
    return result;
}
static SEXP add_non_exit(SEXP target) {
    R_RegisterCFinalizerEx(target, finalize, FALSE);
    return R_NilValue;
}
static SEXP make_weak(SEXP key) {
    return R_MakeWeakRefC(key, R_NilValue, finalize, TRUE);
}
static SEXP enable_dynamic(void) {
    DllInfo *dll = R_getDllInfo("wave.test");
    if (!dll || !R_getEmbeddingDllInfo() || R_useDynamicSymbols(dll, TRUE))
        Rf_error("wrong DLL registration state");
    if (!R_forceSymbols(dll, FALSE) || !R_FindSymbol("zero", "wave.test", NULL))
        Rf_error("wrong symbol lookup state");
    return R_NilValue;
}
static const R_CallMethodDef call[] = {
    {"zero", (DL_FUNC)zero, 0}, {"sum2", (DL_FUNC)sum2, 2}, {"sum10", (DL_FUNC)sum10, 10},
    {"null_pointer", (DL_FUNC)null_pointer, 0}, {"fail", (DL_FUNC)fail, 0},
    {"unbalanced", (DL_FUNC)unbalanced, 0}, {"use_callable", (DL_FUNC)use_callable, 0},
    {"install_callable", (DL_FUNC)install_callable, 0},
    {"counters", (DL_FUNC)counters, 1}, {"make_ext", (DL_FUNC)make_ext, 0},
    {"make_weak", (DL_FUNC)make_weak, 1}, {"enable_dynamic", (DL_FUNC)enable_dynamic, 0},
    {"finalizer_address", (DL_FUNC)finalizer_address, 0},
    {"preserved_value", (DL_FUNC)preserved_value, 0},
    {"add_non_exit", (DL_FUNC)add_non_exit, 1},
    {NULL, NULL, 0}
};
static const R_ExternalMethodDef ext[] = {
    {"external", (DL_FUNC)external, 1}, {"external2", (DL_FUNC)external2, 1}, {NULL, NULL, 0}
};
static const R_CMethodDef c[] = {
    {"mutate", (DL_FUNC)mutate, 2, NULL}, {"ignore", (DL_FUNC)ignore, 1, NULL},
    {"text", (DL_FUNC)text, 1, NULL}, {"over", (DL_FUNC)over, 1, NULL},
    {"under", (DL_FUNC)under, 1, NULL}, {NULL, NULL, 0, NULL}
};
static const R_FortranMethodDef fortran[] = {
    {"f_mutate", (DL_FUNC)mutate, 2, NULL}, {"f_over", (DL_FUNC)over, 1, NULL},
    {"f_text", (DL_FUNC)f_text, 1, NULL}, {NULL, NULL, 0, NULL}
};

void R_init_wave_test(DllInfo *info) {
    R_registerRoutines(info, c, call, fortran, ext);
    R_useDynamicSymbols(info, FALSE);
    R_forceSymbols(info, TRUE);
    R_RegisterCCallable("wave", "callable", (DL_FUNC) zero);
    R_RegisterCCallable("wave", "callable", (DL_FUNC) callable);
    R_RegisterCCallable("wave", "borrowed", (DL_FUNC) R_GetCurrentEnv);
}
void R_unload_wave_test(DllInfo *info) {
    if (info != R_getDllInfo("wave.test")) Rf_error("wrong unload handle");
    if (events) events[1]++;
}
"#;

unsafe extern "C" {
    fn Rf_error(format: *const c_char, ...) -> !;
}

struct Temp(PathBuf);

impl Temp {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "roxide-native-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn compile(temp: &Temp) -> PathBuf {
    let source = temp.0.join("fixture.c");
    let library = temp.0.join("wave.test.so");
    let fixture = if cfg!(target_os = "macos") {
        let exports = ffi_addresses();
        std::hint::black_box(exports);
        FIXTURE.to_owned()
    } else {
        // Cargo's Linux test executable does not export its symbols to dlopen.
        // Use the same C ABI at the executable's resolved addresses.
        let (headers, code) = FIXTURE.split_once("static int *events;").unwrap();
        let mut prelude = headers.to_owned();
        for (name, address) in ffi_addresses() {
            let alias = if matches!(name, "R_NilValue" | "R_GlobalEnv") {
                format!("(*(__typeof__({name}) *){address:#x}ULL)")
            } else {
                format!("((__typeof__(&{name})){address:#x}ULL)")
            };
            prelude.push_str(&format!("#define {name} {alias}\n"));
        }
        prelude + "static int *events;" + code
    };
    fs::write(&source, fixture).unwrap();
    let mut clang = Command::new("clang");
    clang.args(["-shared", "-fPIC"]);
    if cfg!(target_os = "macos") {
        clang.args(["-undefined", "dynamic_lookup"]);
        let stub = temp.0.join("link-stub.dylib");
        let alias = temp.0.join("libR.dylib");
        let output = Command::new("clang")
            .args(["-dynamiclib", "-x", "c", "/dev/null", "-install_name"])
            .arg(&alias)
            .arg("-o")
            .arg(&stub)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        clang
            .args(["-Xlinker", "-weak_library", "-Xlinker"])
            .arg(&stub);
        std::os::unix::fs::symlink(std::env::current_exe().unwrap(), alias).unwrap();
    }
    let output = clang
        .arg("-I")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("include"))
        .arg(&source)
        .arg("-o")
        .arg(&library)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    library
}

fn ffi_addresses() -> Vec<(&'static str, usize)> {
    macro_rules! address {
        ($($module:ident::$name:ident),+ $(,)?) => { vec![$((stringify!($name), $module::$name as *const () as usize)),+] };
    }
    let mut symbols = address![
        coerce::Rf_ScalarInteger,
        eval::R_GetCurrentEnv,
        memory::R_PreserveObject,
        memory::R_ReleaseObject,
        vector::INTEGER,
        jump::Rf_protect,
        jump::Rf_unprotect,
        pairlist::TAG,
        pairlist::CDR,
        pairlist::CAR,
        sym::Rf_install,
        register::R_registerRoutines,
        register::R_useDynamicSymbols,
        register::R_forceSymbols,
        register::R_getDllInfo,
        register::R_getEmbeddingDllInfo,
        register::R_FindSymbol,
        register::R_GetCCallable,
        register::R_RegisterCCallable,
        extptr::R_MakeExternalPtr,
        extptr::R_ExternalPtrAddr,
        extptr::R_RegisterCFinalizerEx,
        weakref::R_MakeWeakRefC,
    ];
    symbols.extend([
        ("R_NilValue", (&raw const globals::R_NilValue).addr()),
        ("R_GlobalEnv", (&raw const globals::R_GlobalEnv).addr()),
        ("Rf_error", Rf_error as *const () as usize),
    ]);
    symbols
}

fn hooks(ctx: &Ctx) -> &'static dyn roxide_eval::NativeHooks {
    ctx.native_hooks().unwrap()
}

fn info(ctx: &mut Ctx, name: &str) -> SexpId {
    let result = hooks(ctx)
        .get_native_symbol_info(ctx, name, Some("wave.test"), true, SEXP_NIL)
        .unwrap();
    ctx.protect(result);
    result
}

fn invoke(
    ctx: &mut Ctx,
    kind: NativeCallKind,
    symbol: SexpId,
    args: &Args,
    opts: Option<&DotCOpts>,
) -> EvalResult {
    let call = Call {
        expr: SEXP_NIL,
        user_call: SEXP_NIL,
        env: GLOBAL_ENV,
        prim: PrimId(0),
    };
    hooks(ctx).call_native(
        ctx,
        kind,
        symbol,
        args,
        Some("wave.test"),
        opts,
        SEXP_NIL,
        GLOBAL_ENV,
        &call,
    )
}

fn call(ctx: &mut Ctx, name: &str, values: &[SexpId]) -> EvalResult {
    ctx.scoped(|ctx| {
        let symbol =
            hooks(ctx).get_native_symbol_info(ctx, name, Some("wave.test"), true, SEXP_NIL)?;
        let args = Args::from_pairs(values.iter().map(|&v| (None, v)).collect());
        invoke(ctx, NativeCallKind::Call, symbol, &args, None)
    })
}

fn message(ctx: &mut Ctx, action: impl FnOnce(&mut Ctx) -> EvalResult) -> String {
    let captured = ctx
        .with_native_error_handler(action, |ctx, condition| {
            Ok(ctx.session().list_get(condition, 0)?)
        })
        .unwrap();
    let ch = ctx
        .session_mut()
        .str_get(captured, 0)
        .expect("expected an R error");
    String::from_utf8(ctx.session().char_bytes(ch).unwrap().to_vec()).unwrap()
}

struct LazyLoader;
impl NamespaceLoader for LazyLoader {
    fn load(&self, ctx: &mut Ctx, name: &str, _: SexpId) -> EvalResult {
        match name {
            "wave" => call(ctx, "install_callable", &[]),
            "interrupted" => Err(roxide_eval::Unwind::TopLevel),
            _ => Err(ctx.stop(SEXP_NIL, "package unavailable")),
        }
    }
}

unsafe extern "C" fn get_callable(data: *mut c_void) {
    // SAFETY: lookup_callable passes a live descriptor and an active trampoline.
    unsafe {
        let ((package, name), result) =
            &mut *data.cast::<((&'static std::ffi::CStr, &'static std::ffi::CStr), Sexp)>();
        if let Some(fun) = register::R_GetCCallable(package.as_ptr(), name.as_ptr()) {
            *result = Sexp(fun() as usize);
        }
    }
}

fn lookup_package_callable(
    ctx: &mut Ctx,
    package: &'static std::ffi::CStr,
    name: &'static std::ffi::CStr,
) -> EvalResult {
    let mut result = ((package, name), Sexp::NULL);
    // SAFETY: get_callable uses the descriptor only while the trampoline is active.
    unsafe { jump::enter_trampoline(ctx, get_callable, std::ptr::from_mut(&mut result).cast())? };
    Ok(result.1.to_id().unwrap())
}

fn lookup_callable(ctx: &mut Ctx, name: &'static std::ffi::CStr) -> EvalResult {
    lookup_package_callable(ctx, c"wave", name)
}

fn int(ctx: &Ctx, value: SexpId) -> i32 {
    ctx.session().int(value).unwrap().iter().next().unwrap()
}

fn keep(ctx: &mut Ctx, make: impl FnOnce(&mut Ctx) -> SexpId) -> SexpId {
    let value = make(ctx);
    ctx.protect(value);
    value
}

fn sexp_calls(ctx: &mut Ctx) {
    let zero = call(ctx, "zero", &[]).unwrap();
    assert_eq!(int(ctx, zero), 42);
    let one = keep(ctx, |ctx| ctx.session_mut().scalar_int(1).unwrap());
    let two = keep(ctx, |ctx| ctx.session_mut().scalar_int(2).unwrap());
    let result = call(ctx, "sum2", &[one, two]).unwrap();
    assert_eq!(int(ctx, result), 3);
    let result = call(ctx, "sum10", &[two; 10]).unwrap();
    assert_eq!(int(ctx, result), 20);

    let sum2 = info(ctx, "sum2");
    let address = ctx.session().list_get(sum2, 1).unwrap();
    let args = Args::from_pairs(vec![(None, one), (None, two)]);
    let result = invoke(ctx, NativeCallKind::Call, address, &args, None).unwrap();
    assert_eq!(int(ctx, result), 3);
    assert_eq!(
        message(ctx, |ctx| invoke(
            ctx,
            NativeCallKind::C,
            address,
            &args,
            None
        )),
        "NULL value passed as symbol address"
    );
    assert_eq!(
        message(ctx, |ctx| invoke(
            ctx,
            NativeCallKind::Call,
            address,
            &Args::default(),
            None
        )),
        "Incorrect number of arguments (0), expecting 2 for 'sum2'"
    );
    let many = Args::from_pairs(vec![(None, one); 66]);
    assert_eq!(
        message(ctx, |ctx| invoke(
            ctx,
            NativeCallKind::Call,
            address,
            &many,
            None
        )),
        "too many arguments in foreign function call"
    );

    let missing = keep(ctx, |ctx| ctx.session_mut().scalar_str("absent").unwrap());
    for (kind, label) in [
        (NativeCallKind::Call, ".Call"),
        (NativeCallKind::External, ".External"),
        (NativeCallKind::External2, ".External"),
        (NativeCallKind::C, ".C"),
        (NativeCallKind::Fortran, ".Fortran"),
    ] {
        assert_eq!(
            message(ctx, |ctx| invoke(
                ctx,
                kind,
                missing,
                &Args::default(),
                None
            )),
            format!(r#""absent" not available for {label}() for package "wave.test""#)
        );
    }

    let null = info(ctx, "null_pointer");
    ctx.opts_mut().warn = 2;
    assert!(
        message(ctx, |ctx| invoke(
            ctx,
            NativeCallKind::Call,
            null,
            &Args::default(),
            None
        ))
        .contains("converting NULL pointer to R NULL")
    );
    ctx.opts_mut().warn = 0;
    assert_eq!(
        invoke(ctx, NativeCallKind::Call, null, &Args::default(), None).unwrap(),
        SEXP_NIL
    );

    for name in ["fail", "unbalanced"] {
        let symbol = info(ctx, name);
        let before = ctx.session().heap.protect_depth();
        if name == "fail" {
            assert_eq!(
                message(ctx, |ctx| invoke(
                    ctx,
                    NativeCallKind::Call,
                    symbol,
                    &Args::default(),
                    None
                )),
                "fixture error"
            );
        } else {
            let result = invoke(ctx, NativeCallKind::Call, symbol, &Args::default(), None).unwrap();
            assert_eq!(int(ctx, result), 4);
        }
        assert_eq!(ctx.session().heap.protect_depth(), before);
    }

    let tag = ctx.session_mut().intern("argument").unwrap();
    for (kind, name) in [
        (NativeCallKind::External, "external"),
        (NativeCallKind::External2, "external2"),
    ] {
        let symbol = info(ctx, name);
        assert_eq!(
            invoke(
                ctx,
                kind,
                symbol,
                &Args::from_pairs(vec![(Some(tag), one)]),
                None
            )
            .unwrap(),
            symbol
        );
    }
}

fn copy_calls(ctx: &mut Ctx) {
    let number = keep(ctx, |ctx| ctx.session_mut().scalar_int(10).unwrap());
    let real = keep(ctx, |ctx| ctx.session_mut().scalar_real(3.0).unwrap());
    let raw = keep(ctx, |ctx| ctx.session_mut().alloc_raw_from(&[17]).unwrap());
    let na_int = keep(ctx, |ctx| ctx.session_mut().scalar_int(NA_INTEGER).unwrap());
    let na_lgl = keep(ctx, |ctx| {
        ctx.session_mut().scalar_lgl(RLogical::NA).unwrap()
    });
    let inf_real = keep(ctx, |ctx| {
        ctx.session_mut().scalar_real(f64::INFINITY).unwrap()
    });
    let na_cplx = keep(ctx, |ctx| {
        ctx.session_mut().alloc_cplx_from(&[RComplex::NA]).unwrap()
    });
    let na_chr = keep(ctx, |ctx| {
        ctx.session_mut().alloc_str_from(&[CharId::NA]).unwrap()
    });
    let one_arg = |value| Args::from_pairs(vec![(None, value)]);

    for (kind, name) in [
        (NativeCallKind::C, "mutate"),
        (NativeCallKind::Fortran, "f_mutate"),
    ] {
        let routine = info(ctx, name);
        let tag = ctx.session_mut().intern("value").unwrap();
        let args = Args::from_pairs(vec![(Some(tag), number), (None, real)]);
        let result = invoke(ctx, kind, routine, &args, None).unwrap();
        assert_eq!(int(ctx, ctx.session().list_get(result, 0).unwrap()), 13);
        let value = ctx.session().list_get(result, 1).unwrap();
        assert_eq!(ctx.session().real(value).unwrap().iter().next(), Some(3.5));
        assert_eq!(int(ctx, number), 10);
        assert_eq!(ctx.session().real(real).unwrap().iter().next(), Some(3.0));
        let names = ctx.session().attr(result, SYM_NAMES);
        let first = ctx.session_mut().str_get(names, 0).unwrap();
        let second = ctx.session_mut().str_get(names, 1).unwrap();
        assert_eq!(ctx.session().char_bytes(first).unwrap(), b"value");
        assert_eq!(ctx.session().char_bytes(second).unwrap(), b"");

        let args = Args::from_pairs(vec![(None, number), (None, inf_real)]);
        assert_eq!(
            message(ctx, |ctx| invoke(ctx, kind, routine, &args, None)),
            "NA/NaN/Inf in foreign function call (arg 2)"
        );
    }

    let ignore = info(ctx, "ignore");
    for (value, expected) in [
        (na_int, "NAs in foreign function call (arg 1)"),
        (na_lgl, "NAs in foreign function call (arg 1)"),
        (inf_real, "NA/NaN/Inf in foreign function call (arg 1)"),
        (
            na_cplx,
            "complex NA/NaN/Inf in foreign function call (arg 1)",
        ),
    ] {
        assert_eq!(
            message(ctx, |ctx| invoke(
                ctx,
                NativeCallKind::C,
                ignore,
                &one_arg(value),
                None
            )),
            expected
        );
        let naok = DotCOpts {
            naok: true,
            ..Default::default()
        };
        assert!(invoke(ctx, NativeCallKind::C, ignore, &one_arg(value), Some(&naok)).is_ok());
    }
    let result = invoke(ctx, NativeCallKind::C, ignore, &one_arg(na_chr), None).unwrap();
    let text = ctx.session().list_get(result, 0).unwrap();
    assert_eq!(ctx.session_mut().str_get(text, 0).unwrap(), CharId::NA);
    let text = keep(ctx, |ctx| ctx.session_mut().scalar_str("yes").unwrap());
    let routine = info(ctx, "text");
    let result = invoke(ctx, NativeCallKind::C, routine, &one_arg(text), None).unwrap();
    let text = ctx.session().list_get(result, 0).unwrap();
    let text = ctx.session_mut().str_get(text, 0).unwrap();
    assert_eq!(ctx.session().char_bytes(text).unwrap(), b"Yes");

    let strings = keep(ctx, |ctx| {
        let session = ctx.session_mut();
        let yes = session
            .mk_char(b"yes", roxide_value::Encoding::Utf8)
            .unwrap();
        let no = session
            .mk_char(b"no", roxide_value::Encoding::Utf8)
            .unwrap();
        session.alloc_str_from(&[yes, no]).unwrap()
    });
    let f_text = info(ctx, "f_text");
    let args = one_arg(strings);
    let single = keep(ctx, |ctx| ctx.session_mut().scalar_str("yes").unwrap());
    ctx.opts_mut().warn = 2;
    assert_eq!(
        message(ctx, |ctx| invoke(
            ctx,
            NativeCallKind::Fortran,
            f_text,
            &one_arg(single),
            None
        )),
        "(converted from warning) passing a char vector to .Fortran is not portable"
    );
    assert_eq!(
        message(ctx, |ctx| invoke(
            ctx,
            NativeCallKind::Fortran,
            f_text,
            &args,
            None
        )),
        "(converted from warning) only the first string in a char vector used in .Fortran"
    );
    ctx.opts_mut().warn = 0;
    let result = invoke(ctx, NativeCallKind::Fortran, f_text, &args, None).unwrap();
    let text = ctx.session().list_get(result, 0).unwrap();
    assert_eq!(ctx.session().xlength(text).unwrap(), 1);
    let ch = ctx.session_mut().str_get(text, 0).unwrap();
    assert_eq!(ctx.session().char_bytes(ch).unwrap(), b"Yes");

    for (kind, name, expected) in [
        (
            NativeCallKind::C,
            "over",
            "array over-run in .C(\"over\") argument 1",
        ),
        (
            NativeCallKind::C,
            "under",
            "array under-run in .C(\"under\") argument 1",
        ),
        (
            NativeCallKind::Fortran,
            "f_over",
            "array over-run in .Fortran(\"f_over\") argument 1",
        ),
    ] {
        let symbol = info(ctx, name);
        assert_eq!(
            message(ctx, |ctx| invoke(ctx, kind, symbol, &one_arg(raw), None)),
            expected
        );
    }
}

fn rejected_images(ctx: &mut Ctx, temp: &Temp) {
    let path = temp.0.join("foreign.so");
    let name = b"/Library/Frameworks/R.framework/Versions/4.6/Resources/lib/libR.dylib";
    let size = (24 + name.len() + 1).next_multiple_of(8);
    let mut bytes = vec![0; 32 + size];
    bytes[..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
    for (i, value) in [
        (12, 6u32),
        (16, 1),
        (20, size as u32),
        (32, 0xc),
        (36, size as u32),
        (40, 24),
    ] {
        bytes[i..i + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[56..56 + name.len()].copy_from_slice(name);
    for (image, expected) in [
        (&bytes[..], "foreign R dependency"),
        (&bytes[..50], "invalid or unsupported shared-library format"),
    ] {
        fs::write(&path, image).unwrap();
        let failure = message(ctx, |ctx| {
            hooks(ctx).dyn_load(ctx, &path, true, true, SEXP_NIL)
        });
        assert!(failure.starts_with(&format!(
            "unable to load shared object '{}':\n  {expected}",
            path.display()
        )));
    }
}

fn exercise(policy: GcPolicy) {
    static NATIVE_SESSION: Mutex<()> = Mutex::new(());
    let _guard = NATIVE_SESSION.lock().unwrap_or_else(|e| e.into_inner());
    let temp = Temp::new();
    let library = compile(&temp);
    // Keep the debug payload sparse. It must not affect dependency validation.
    fs::OpenOptions::new()
        .write(true)
        .open(&library)
        .unwrap()
        .set_len(257 * 1024 * 1024)
        .unwrap();
    let mut events = [0i32; 3];
    let mut ctx = Ctx::new(Box::new(NativeHost::new()), policy).unwrap();
    hooks::install(&mut ctx);
    let native = hooks(&ctx);
    rejected_images(&mut ctx, &temp);
    if cfg!(target_os = "macos") {
        ctx.host().set_env_var("ROXIDE_LIBR_PATH", None).unwrap();
        assert!(
            message(&mut ctx, |ctx| native
                .dyn_load(ctx, &library, true, true, SEXP_NIL))
            .contains("foreign R dependency")
        );
        let alias = temp.0.join("libR.dylib");
        ctx.host()
            .set_env_var("ROXIDE_LIBR_PATH", Some(alias.as_os_str().as_bytes()))
            .unwrap();
    }

    ctx.scoped(|ctx| {
        let dll = native.dyn_load(ctx, &library, true, true, SEXP_NIL)?;
        let dlls = native.get_loaded_dlls(ctx)?;
        assert_eq!(ctx.session().list_get(dlls, 0)?, dll);
        let names = ctx.session().attr(dlls, SYM_NAMES);
        let name = ctx.session_mut().str_get(names, 0)?;
        assert_eq!(ctx.session().char_bytes(name)?, b"wave.test");
        let name = ctx.session().list_get(dll, 0)?;
        let name = ctx.session_mut().str_get(name, 0)?;
        assert_eq!(ctx.session().char_bytes(name)?, b"wave.test");
        ctx.collect_gc();
        let preserved = call(ctx, "preserved_value", &[])?;
        assert_eq!(int(ctx, preserved), 97);
        let tables = native.get_dll_registered_routines(ctx, dll, true, SEXP_NIL)?;
        let calls = ctx.session().list_get(tables, 1)?;
        assert_eq!(ctx.session().xlength(calls)?, 15);
        let dynamic = ctx.session().list_get(dll, 2)?;
        assert_eq!(
            ctx.session().lgl(dynamic)?.iter().next(),
            Some(RLogical::FALSE)
        );
        assert!(!native.is_loaded(
            ctx,
            "dynamic_value",
            Some("wave.test"),
            Some(NativeCallKind::Fortran)
        )?);
        let zero_name = keep(ctx, |ctx| ctx.session_mut().scalar_str("zero").unwrap());
        assert_eq!(
            message(ctx, |ctx| invoke(
                ctx,
                NativeCallKind::Call,
                zero_name,
                &Args::default(),
                None
            )),
            r#""zero" not available for .Call() for package "wave.test""#
        );

        sexp_calls(ctx);
        copy_calls(ctx);

        call(ctx, "enable_dynamic", &[])?;
        assert!(native.is_loaded(
            ctx,
            "DYNAMIC_VALUE",
            Some("wave.test"),
            Some(NativeCallKind::Fortran)
        )?);
        let mangled = keep(ctx, |ctx| {
            ctx.session_mut().scalar_str("DYNAMIC_VALUE").unwrap()
        });
        let number = keep(ctx, |ctx| ctx.session_mut().scalar_int(1).unwrap());
        let args = Args::from_pairs(vec![(None, number)]);
        let result = invoke(ctx, NativeCallKind::Fortran, mangled, &args, None)?;
        assert_eq!(int(ctx, ctx.session().list_get(result, 0)?), 11);
        let result = invoke(ctx, NativeCallKind::Call, zero_name, &Args::default(), None)?;
        assert_eq!(int(ctx, result), 42);

        let value = call(ctx, "use_callable", &[])?;
        assert_eq!(int(ctx, value), 91);
        assert_eq!(lookup_callable(ctx, c"borrowed")?, GLOBAL_ENV);
        ctx.set_namespace_loader(&LazyLoader);
        let value = lookup_callable(ctx, c"lazy")?;
        assert_eq!(int(ctx, value), 91);
        assert!(matches!(
            lookup_package_callable(ctx, c"interrupted", c"lazy"),
            Err(roxide_eval::Unwind::TopLevel)
        ));
        assert_eq!(
            message(ctx, |ctx| lookup_callable(ctx, c"missing")),
            "function 'missing' not provided by package 'wave'"
        );

        let ptr = keep(ctx, |ctx| {
            ctx.session_mut()
                .alloc_extptr(events.as_mut_ptr().expose_provenance(), SEXP_NIL, SEXP_NIL)
                .unwrap()
        });
        call(ctx, "counters", &[ptr])?;
        let callback = call(ctx, "finalizer_address", &[])?;
        let callback = ctx.session().extptr_addr(callback)?;
        let live = call(ctx, "make_ext", &[])?;
        ctx.protect(live);
        let weak = call(ctx, "make_weak", &[live])?;
        ctx.protect(weak);

        let non_exit = keep(ctx, |ctx| {
            ctx.session_mut()
                .alloc_extptr(0, SEXP_NIL, SEXP_NIL)
                .unwrap()
        });
        call(ctx, "add_non_exit", &[non_exit])?;
        call(ctx, "make_ext", &[])?;
        ctx.collect_gc();
        assert_eq!(events, [0, 0, 0]);

        let old = info(ctx, "zero");
        let address = ctx.session().list_get(old, 1)?;
        let dll_handle = ctx.session().list_get(dll, 4)?;
        let id = ctx.session().extptr_addr(dll_handle)?;
        let reloaded = native.dyn_load(ctx, &library, true, false, SEXP_NIL)?;
        // Two live exit callbacks and one queued callback all ran before dlclose.
        assert_eq!(events, [3, 1, 1]);
        assert_eq!(ctx.session().weakref_key(weak)?, SEXP_NIL);
        let dlls = native.get_loaded_dlls(ctx)?;
        let names = ctx.session().attr(dlls, SYM_NAMES);
        let mut matching = 0;
        for i in 0..ctx.session().xlength(names)? {
            let name = ctx.session_mut().str_get(names, i)?;
            matching += usize::from(ctx.session().char_bytes(name)? == b"wave.test");
        }
        assert_eq!(matching, 1);
        assert_eq!(ctx.session().extptr_addr(address)?, 0);
        assert_eq!(ctx.session().extptr_addr(dll_handle)?, 0);
        assert!(
            message(ctx, |ctx| invoke(
                ctx,
                NativeCallKind::Call,
                old,
                &Args::default(),
                None
            ))
            .contains("NULL value passed as symbol address")
        );
        let handle = ctx.session().list_get(reloaded, 4)?;
        assert!(ctx.session().extptr_addr(handle)? > id);
        call(ctx, "counters", &[ptr])?;
        native.run_c_finalizer(ctx, callback, live)?;
        assert_eq!(events, [3, 1, 1]);
        // Register the same callback on the surviving object in the new DLL.
        call(ctx, "add_non_exit", &[non_exit])?;
        native.dyn_unload(ctx, &library, SEXP_NIL)?;
        for (symbol, expected) in [
            (
                c"callable",
                "function 'callable' not provided by package 'wave'",
            ),
            (
                c"borrowed",
                "function 'borrowed' not provided by package 'wave'",
            ),
        ] {
            assert_eq!(message(ctx, |ctx| lookup_callable(ctx, symbol)), expected);
        }
        Ok(())
    })
    .unwrap();

    ctx.collect_gc();
    ctx.run_pending_finalizers();
    assert_eq!(events, [3, 2, 2]);
    assert_eq!(ctx.session().heap.protect_depth(), 0);
}

#[test]
fn native_dispatch_default_gc() {
    exercise(GcPolicy::default());
}

#[test]
fn native_dispatch_torture_gc() {
    exercise(GcPolicy::Torture);
}
