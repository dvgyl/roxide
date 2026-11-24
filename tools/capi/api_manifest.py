#!/usr/bin/env python3
"""Clean-room C API symbol manifest generator for Roxide (ADR-0017).

Combines strictly black-box symbol metadata from:
  1. Official GNU R 4.6.2 `tools:::funAPI()` (symbol names, locations, API tier)
  2. `nm` exported symbol table of oracle `libR` (exported symbol names & func/data kind)

Never reads any GNU R header or source file.
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
DEFAULT_OUT = ROOT / "tools" / "capi" / "manifest.json"

# Documented R 4.6.2 public C-API data globals (from Writing R Extensions & Rinternals.h).
# Strictly excludes non-API/internal data globals such as R_FunTab or R_PPStack.
PUBLIC_DATA_GLOBALS = {
    "R_NilValue",
    "R_UnboundValue",
    "R_MissingArg",
    "R_NaString",
    "R_BlankString",
    "R_BlankScalarString",
    "R_EmptyEnv",
    "R_BaseEnv",
    "R_GlobalEnv",
    "R_BaseNamespace",
    "R_NamespaceRegistry",
    "R_MethodsNamespace",
    "R_TrueValue",
    "R_FalseValue",
    "R_LogicalNAValue",
    "R_NaInt",
    "R_NaReal",
    "R_NaN",
    "R_PosInf",
    "R_NegInf",
    "R_AsCharacterSymbol",
    "R_AtsignSymbol",
    "R_BaseSymbol",
    "R_baseSymbol",
    "R_BraceSymbol",
    "R_Bracket2Symbol",
    "R_BracketSymbol",
    "R_ClassSymbol",
    "R_DeviceSymbol",
    "R_DimNamesSymbol",
    "R_DimSymbol",
    "R_DollarSymbol",
    "R_DotsSymbol",
    "R_DoubleColonSymbol",
    "R_DropSymbol",
    "R_EvalSymbol",
    "R_FunctionSymbol",
    "R_LastvalueSymbol",
    "R_LevelsSymbol",
    "R_ModeSymbol",
    "R_NaRmSymbol",
    "R_NameSymbol",
    "R_NamesSymbol",
    "R_NamespaceEnvSymbol",
    "R_PackageSymbol",
    "R_PreviousSymbol",
    "R_QuoteSymbol",
    "R_RowNamesSymbol",
    "R_SeedsSymbol",
    "R_SortListSymbol",
    "R_SourceSymbol",
    "R_SpecSymbol",
    "R_SrcfileSymbol",
    "R_SrcrefSymbol",
    "R_TripleColonSymbol",
    "R_TspSymbol",
    "R_dot_Generic",
    "R_dot_Method",
    "R_dot_defined",
    "R_dot_packageName",
    "R_dot_target",
}


def query_funapi(rscript: str) -> dict[str, dict[str, str]]:
    """Query `tools:::funAPI()` from the live GNU R oracle."""
    r_code = (
        'df <- tools:::funAPI(); '
        'for (i in seq_len(nrow(df))) '
        'cat(df$name[i], "\\t", df$loc[i], "\\t", df$apitype[i], "\\n", sep = "")'
    )
    out = subprocess.check_output([rscript, "--vanilla", "-e", r_code], text=True)
    records: dict[str, dict[str, str]] = {}
    for line in out.splitlines():
        if not line.strip():
            continue
        parts = line.split("\t")
        if len(parts) != 3:
            continue
        name, loc, apitype = parts
        records[name] = {"loc": loc, "apitype": apitype}
    return records


def query_nm_exports(libr_path: pathlib.Path) -> dict[str, str]:
    """Read defined global symbol names and func/data kinds from `libR` via `nm`."""
    if sys.platform == "darwin":
        cmd = ["nm", "-gU", str(libr_path)]
    else:
        cmd = ["nm", "-D", "--defined-only", str(libr_path)]
    out = subprocess.check_output(cmd, text=True)
    exports: dict[str, str] = {}
    for line in out.splitlines():
        parts = line.split()
        if len(parts) < 3:
            continue
        sym_type, raw_name = parts[1], parts[2]
        name = raw_name[1:] if sys.platform == "darwin" and raw_name.startswith("_") else raw_name
        if sym_type in {"T", "t", "W", "i"}:
            kind = "func"
        elif sym_type in {"D", "S", "B", "R", "V"}:
            kind = "data"
        else:
            continue
        exports[name] = kind
    return exports


def build_manifest(rscript: str, libr_path: pathlib.Path) -> dict:
    funapi = query_funapi(rscript)
    exports = query_nm_exports(libr_path)

    all_names = sorted(set(funapi) | PUBLIC_DATA_GLOBALS | set(exports))
    symbols: list[dict[str, object]] = []
    for name in all_names:
        meta = funapi.get(name)
        in_libr = name in exports
        kind = exports.get(name, "macro_or_external")
        if meta is not None:
            apitype = meta["apitype"]
            loc = meta["loc"]
        elif name in PUBLIC_DATA_GLOBALS and kind == "data":
            apitype = "data"
            loc = "Rinternals.h"
        else:
            apitype = "nonapi"
            loc = ""
        symbols.append(
            {
                "name": name,
                "apitype": apitype,
                "loc": loc,
                "kind": kind,
                "exported_in_libr": in_libr,
            }
        )

    counts: dict[str, int] = {}
    for row in symbols:
        key = str(row["apitype"])
        counts[key] = counts.get(key, 0) + 1

    return {
        "oracle_target": "4.6.2",
        "counts": counts,
        "total_libr_exports": len(exports),
        "symbols": symbols,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=pathlib.Path, default=DEFAULT_OUT)
    parser.add_argument(
        "--rscript",
        default=os.environ.get("ROXIDE_ORACLE_RSCRIPT", ""),
        help="Path to GNU R 4.6.2 Rscript oracle",
    )
    parser.add_argument(
        "--libr",
        type=pathlib.Path,
        default=None,
        help="Path to oracle libR.dylib / libR.so",
    )
    args = parser.parse_args()

    if not args.rscript:
        print("ROXIDE_ORACLE_RSCRIPT not set; source env/env.sh first", file=sys.stderr)
        return 1

    libr = args.libr
    if libr is None:
        r_home = os.environ.get("ROXIDE_ORACLE_R_HOME", "")
        if not r_home:
            print("ROXIDE_ORACLE_R_HOME not set; source env/env.sh first", file=sys.stderr)
            return 1
        ext = "dylib" if sys.platform == "darwin" else "so"
        libr = pathlib.Path(r_home) / "lib" / f"libR.{ext}"

    manifest = build_manifest(args.rscript, libr)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(
        f"Wrote {args.out.relative_to(ROOT) if args.out.is_relative_to(ROOT) else args.out}: "
        f"{len(manifest['symbols'])} entries ({manifest['counts']}), "
        f"{manifest['total_libr_exports']} oracle libR exports."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
