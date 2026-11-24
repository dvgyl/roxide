//! Build the native API shims and the integration-test fixture.

fn main() {
    println!("cargo:rerun-if-changed=csrc/shim.cc");
    println!("cargo:rerun-if-changed=csrc/shim_va.c");
    println!("cargo:rerun-if-changed=csrc/dotcall.c");
    println!("cargo:rerun-if-changed=csrc/altrep.c");
    println!("cargo:rerun-if-changed=include");

    cc::Build::new()
        .std("c11")
        .flag_if_supported("-Wall")
        .flag_if_supported("-Wextra")
        .file("csrc/shim_va.c")
        .file("csrc/dotcall.c")
        .compile("roxide_capi_va");
    cc::Build::new()
        .std("c11")
        .include("include")
        .flag_if_supported("-w")
        .file("csrc/altrep.c")
        .compile("roxide_capi_altrep");
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

    let altrep_fixture = "../../tests/capi/fixtures/07_altrep.c";
    println!("cargo:rerun-if-changed={altrep_fixture}");
    cc::Build::new()
        .std("c11")
        .include("include")
        .define("ROXIDE_ALTREP_TEST", None)
        .file(altrep_fixture)
        .cargo_metadata(false)
        .compile("roxide_capi_altrep_fixture");
}
