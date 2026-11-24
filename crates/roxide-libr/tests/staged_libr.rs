//! Test the C launcher entry point with the native hooks.

use std::ffi::CString;
use std::os::raw::c_int;

#[test]
fn launcher_installs_native_hooks() {
    let script = r#"
        stopifnot(inherits(getLoadedDLLs(), "DLLInfoList"), !is.loaded("roxide_test_missing"))
        message <- tryCatch(dyn.load(""), error=conditionMessage)
        stopifnot(grepl("unable to load shared object", message, fixed=TRUE))
        quit(status=42L)
    "#;
    let args = ["Rscript", "--vanilla", "-e", script]
        .map(|arg| CString::new(arg).expect("valid C argument"));
    let argv = args.each_ref().map(|arg| arg.as_ptr());
    let argc = c_int::try_from(argv.len()).expect("argument count");
    // SAFETY: The pointer array and all strings live until the launcher returns.
    let status = unsafe { roxide_libr::roxide_main(argc, argv.as_ptr()) };
    assert_eq!(status, 42);
}
