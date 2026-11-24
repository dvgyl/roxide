//! Build the native API shims and the integration-test fixture.

fn main() {
    println!("cargo:rerun-if-changed=csrc/shim.cc");
    println!("cargo:rerun-if-changed=csrc/shim_va.c");
    println!("cargo:rerun-if-changed=csrc/dotcall.c");

    cc::Build::new()
        .std("c11")
        .flag_if_supported("-Wall")
        .flag_if_supported("-Wextra")
        .file("csrc/shim_va.c")
        .file("csrc/dotcall.c")
        .compile("roxide_capi_va");
    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .flag_if_supported("-Wall")
        .flag_if_supported("-Wextra")
        .file("csrc/shim.cc")
        .compile("roxide_capi_shim");

    // Only the integration test links this archive; emit explicit search path
    // so it does not rely implicitly on the production build metadata.
    if let Ok(out_dir) = std::env::var("OUT_DIR") {
        println!("cargo:rustc-link-search=native={out_dir}");
    }
    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .flag_if_supported("-Wall")
        .flag_if_supported("-Wextra")
        .define("ROXIDE_TRAMPOLINE_FIXTURE", None)
        .file("csrc/shim.cc")
        .cargo_metadata(false)
        .compile("roxide_capi_fixture");
}
