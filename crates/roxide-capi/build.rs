//! Build the native API shims and the integration-test fixture.

fn main() {
    println!("cargo:rerun-if-changed=csrc/shim.cc");
    println!("cargo:rerun-if-changed=csrc/shim_va.c");

    cc::Build::new()
        .std("c11")
        .file("csrc/shim_va.c")
        .compile("roxide_capi_va");
    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .file("csrc/shim.cc")
        .compile("roxide_capi_shim");

    // Only the integration test links this archive.
    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .define("ROXIDE_TRAMPOLINE_FIXTURE", None)
        .file("csrc/shim.cc")
        .cargo_metadata(false)
        .compile("roxide_capi_fixture");
}
