#!/usr/bin/env python3
"""Numeric-only ABI layout & constant comparator for clean-room C API headers (ADR-0017).

Compiles an identical clean-room C probe against Roxide headers and oracle headers,
runs both binaries, and compares ONLY their printed numeric values (`sizeof`,
`alignof`, `offsetof`, enum values, numeric macros). Never reads oracle header text.
"""

from __future__ import annotations

import argparse
import os
import pathlib
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
DEFAULT_PROBE = ROOT / "tools" / "capi" / "abi_probe_src.c"

DEFAULT_PROBE_CODE = r"""
/* SPDX-License-Identifier: MIT OR Apache-2.0 */
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <R.h>
#include <Rinternals.h>
#include <R_ext/Rdynload.h>

int main(void) {
    printf("sizeof.SEXP=%zu\n", sizeof(SEXP));
    printf("sizeof.Rboolean=%zu\n", sizeof(Rboolean));
    printf("sizeof.Rbyte=%zu\n", sizeof(Rbyte));
    printf("sizeof.Rcomplex=%zu\n", sizeof(Rcomplex));
    printf("offset.Rcomplex.r=%zu\n", offsetof(Rcomplex, r));
    printf("offset.Rcomplex.i=%zu\n", offsetof(Rcomplex, i));
    printf("sizeof.R_len_t=%zu\n", sizeof(R_len_t));
    printf("sizeof.R_xlen_t=%zu\n", sizeof(R_xlen_t));
    printf("sizeof.R_CallMethodDef=%zu\n", sizeof(R_CallMethodDef));
    printf("sizeof.R_CMethodDef=%zu\n", sizeof(R_CMethodDef));
    printf("sizeof.R_ExternalMethodDef=%zu\n", sizeof(R_ExternalMethodDef));
    printf("enum.FALSE=%d\n", (int)FALSE);
    printf("enum.TRUE=%d\n", (int)TRUE);
    printf("sexp.NILSXP=%d\n", (int)NILSXP);
    printf("sexp.SYMSXP=%d\n", (int)SYMSXP);
    printf("sexp.LISTSXP=%d\n", (int)LISTSXP);
    printf("sexp.CLOSXP=%d\n", (int)CLOSXP);
    printf("sexp.ENVSXP=%d\n", (int)ENVSXP);
    printf("sexp.PROMSXP=%d\n", (int)PROMSXP);
    printf("sexp.LANGSXP=%d\n", (int)LANGSXP);
    printf("sexp.SPECIALSXP=%d\n", (int)SPECIALSXP);
    printf("sexp.BUILTINSXP=%d\n", (int)BUILTINSXP);
    printf("sexp.CHARSXP=%d\n", (int)CHARSXP);
    printf("sexp.LGLSXP=%d\n", (int)LGLSXP);
    printf("sexp.INTSXP=%d\n", (int)INTSXP);
    printf("sexp.REALSXP=%d\n", (int)REALSXP);
    printf("sexp.CPLXSXP=%d\n", (int)CPLXSXP);
    printf("sexp.STRSXP=%d\n", (int)STRSXP);
    printf("sexp.DOTSXP=%d\n", (int)DOTSXP);
    printf("sexp.ANYSXP=%d\n", (int)ANYSXP);
    printf("sexp.VECSXP=%d\n", (int)VECSXP);
    printf("sexp.EXPRSXP=%d\n", (int)EXPRSXP);
    printf("sexp.BCODESXP=%d\n", (int)BCODESXP);
    printf("sexp.EXTPTRSXP=%d\n", (int)EXTPTRSXP);
    printf("sexp.WEAKREFSXP=%d\n", (int)WEAKREFSXP);
    printf("sexp.RAWSXP=%d\n", (int)RAWSXP);
    printf("sexp.S4SXP=%d\n", (int)S4SXP);
    return 0;
}
"""


def compile_and_run(cc: str, include_dir: pathlib.Path, probe_src: str, td: pathlib.Path, tag: str) -> dict[str, str]:
    c_path = td / f"probe_{tag}.c"
    bin_path = td / f"probe_{tag}"
    c_path.write_text(probe_src, encoding="utf-8")
    subprocess.run(
        [cc, "-std=c11", "-Wall", "-Wextra", f"-I{include_dir}", str(c_path), "-o", str(bin_path)],
        check=True,
    )
    out = subprocess.check_output([str(bin_path)], text=True)
    values: dict[str, str] = {}
    for line in out.splitlines():
        if "=" in line:
            k, v = line.split("=", 1)
            values[k.strip()] = v.strip()
    return values


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--roxide-include", type=pathlib.Path, default=ROOT / "crates" / "roxide-capi" / "include")
    parser.add_argument("--oracle-include", type=pathlib.Path, default=None)
    parser.add_argument("--probe", type=pathlib.Path, default=DEFAULT_PROBE)
    parser.add_argument("--cc", default=os.environ.get("CC", "clang"))
    args = parser.parse_args()

    oracle_inc = args.oracle_include
    if oracle_inc is None:
        r_home = os.environ.get("ROXIDE_ORACLE_R_HOME", "")
        if not r_home:
            print("ROXIDE_ORACLE_R_HOME not set; source env/env.sh first", file=sys.stderr)
            return 1
        oracle_inc = pathlib.Path(r_home) / "include"

    if not args.roxide_include.exists() or not (args.roxide_include / "R.h").exists():
        print(f"Roxide headers not yet present at {args.roxide_include}", file=sys.stderr)
        return 2

    probe_src = args.probe.read_text(encoding="utf-8") if args.probe.exists() else DEFAULT_PROBE_CODE

    with tempfile.TemporaryDirectory() as raw_td:
        td = pathlib.Path(raw_td)
        oracle_vals = compile_and_run(args.cc, oracle_inc, probe_src, td, "oracle")
        roxide_vals = compile_and_run(args.cc, args.roxide_include, probe_src, td, "roxide")

    diffs: list[str] = []
    for key in sorted(set(oracle_vals) | set(roxide_vals)):
        ov = oracle_vals.get(key)
        rv = roxide_vals.get(key)
        if ov != rv:
            diffs.append(f"  {key}: oracle={ov!r} roxide={rv!r}")

    if diffs:
        print("ABI PROBE MISMATCHES:", file=sys.stderr)
        print("\n".join(diffs), file=sys.stderr)
        return 1
    print(f"ABI probe passed: {len(oracle_vals)} numeric constants/layouts match GNU R 4.6.2.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
