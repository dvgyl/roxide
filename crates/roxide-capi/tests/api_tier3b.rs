//! Test code-point encoding and C and C++ header compatibility.

use std::io::{self, Write};
use std::process::{Command, Stdio};

use roxide_capi::api::string::Rf_ucstoutf8;

#[test]
fn ucstoutf8_matches_gnu_r_4_6_2() {
    let cases: &[(u32, usize, &[u8])] = &[
        (0, 1, &[0]),
        (1, 1, &[1, 0]),
        (0x7f, 1, &[0x7f, 0]),
        (0x80, 2, &[0xc2, 0x80, 0]),
        (0x7ff, 2, &[0xdf, 0xbf, 0]),
        (0x800, 3, &[0xe0, 0xa0, 0x80, 0]),
        (0xd800, 3, &[0xed, 0xa0, 0x80, 0]),
        (0xdfff, 3, &[0xed, 0xbf, 0xbf, 0]),
        (0xffff, 3, &[0xef, 0xbf, 0xbf, 0]),
        (0x1_0000, 4, &[0xf0, 0x90, 0x80, 0x80, 0]),
        (0x10_ffff, 4, &[0xf4, 0x8f, 0xbf, 0xbf, 0]),
        (0x11_0000, 4, &[0xf4, 0x90, 0x80, 0x80, 0]),
        (0x1f_ffff, 4, &[0xf7, 0xbf, 0xbf, 0xbf, 0]),
        (0x20_0000, 5, &[0xf8, 0x88, 0x80, 0x80, 0x80, 0]),
        (0x3ff_ffff, 5, &[0xfb, 0xbf, 0xbf, 0xbf, 0xbf, 0]),
        (0x400_0000, 6, &[0xfc, 0x84, 0x80, 0x80, 0x80, 0x80, 0]),
        (0x7fff_ffff, 6, &[0xfd, 0xbf, 0xbf, 0xbf, 0xbf, 0xbf, 0]),
        (0x8000_0000, 3, &[0xef, 0xbf, 0xbd, 0]),
        (u32::MAX, 3, &[0xef, 0xbf, 0xbd, 0]),
    ];
    for &(code, expected_len, expected_bytes) in cases {
        let mut buffer = [0xa5u8; 8];
        let mut expected = buffer;
        expected[..expected_bytes.len()].copy_from_slice(expected_bytes);

        // SAFETY: The buffer has space for six bytes and a NUL terminator.
        let len = unsafe { Rf_ucstoutf8(buffer.as_mut_ptr().cast(), code) };

        assert_eq!(len, expected_len, "encoded length for {code:#x}");
        assert_eq!(buffer, expected, "buffer contents for {code:#x}");
    }
}

#[test]
fn public_headers_compile_with_boolean_macros() -> io::Result<()> {
    const SOURCE: &str = r#"
#define TRUE 1
#define FALSE 0
#include <R.h>
#include <Rinternals.h>
#include <R_ext/GraphicsEngine.h>
#include <R_ext/GraphicsDevice.h>
#include <Rinterface.h>

Rboolean boolean_probe(void) { return TRUE; }
DevDesc *device_probe(pDevDesc device) { return device; }
Rboolean interactive_probe(void) { return R_Interactive; }
void version_probe(void) { R_GE_checkVersionOrDie(R_GE_version); }
int length_probe(char *buffer, size_t size, R_xlen_t length) {
    return snprintf(buffer, size, "%" R_PRIdXLEN_T, length);
}

#ifdef R_NO_REMAP
#if defined(ucstoutf8) || defined(onintr)
#error R_NO_REMAP must disable the aliases
#endif
size_t encode_probe(char *buffer) { return Rf_ucstoutf8(buffer, 0x80); }
void interrupt_probe(void) { Rf_onintr(); }
#else
size_t encode_probe(char *buffer) { return ucstoutf8(buffer, 0x80); }
void interrupt_probe(void) { onintr(); }
#endif
"#;

    for (language, standard, no_remap) in [
        ("c", "-std=c11", false),
        ("c", "-std=c11", true),
        ("c++", "-std=c++17", true),
    ] {
        let mut clang = Command::new("clang");
        clang
            .args([standard, "-Wall", "-Wextra", "-Werror", "-pedantic-errors"])
            .args(["-fsyntax-only", "-x", language, "-"])
            .arg("-I")
            .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/include"));
        if no_remap {
            clang.arg("-DR_NO_REMAP");
        }
        let mut child = clang.stdin(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("missing clang stdin"))?;
        stdin.write_all(SOURCE.as_bytes())?;
        drop(stdin);
        let output = child.wait_with_output()?;
        assert!(
            output.status.success(),
            "{standard} with R_NO_REMAP={no_remap}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}
