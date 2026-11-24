#!/usr/bin/env python3
"""Verify exported symbols of Roxide's `libR` against `tools/capi/manifest.json` (ADR-0020)."""

from __future__ import annotations

import argparse
import json
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
DEFAULT_MANIFEST = ROOT / "tools" / "capi" / "manifest.json"


def list_exports(lib_path: pathlib.Path) -> set[str]:
    if sys.platform == "darwin":
        cmd = ["nm", "-gU", str(lib_path)]
    else:
        cmd = ["nm", "-D", "--defined-only", str(lib_path)]
    out = subprocess.check_output(cmd, text=True)
    names: set[str] = set()
    for line in out.splitlines():
        parts = line.split()
        if len(parts) < 3:
            continue
        sym_type, raw = parts[1], parts[2]
        if sym_type not in {"T", "D", "S", "B", "R"}:
            continue
        name = raw[1:] if sys.platform == "darwin" and raw.startswith("_") else raw
        names.add(name)
    return names


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("libr", type=pathlib.Path, help="Path to built libR.dylib / libR.so")
    parser.add_argument("--manifest", type=pathlib.Path, default=DEFAULT_MANIFEST)
    parser.add_argument(
        "--apitypes",
        default="api,data",
        help="Comma-separated manifest apitypes required (default: api,data)",
    )
    parser.add_argument("--summary-only", action="store_true")
    args = parser.parse_args()

    manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    wanted_types = {s.strip() for s in args.apitypes.split(",") if s.strip()}
    required = {
        row["name"]
        for row in manifest["symbols"]
        if row["exported_in_libr"] and row["apitype"] in wanted_types
    }
    present = list_exports(args.libr)
    missing = sorted(required - present)
    matched = len(required) - len(missing)

    print(
        f"libR export coverage ({','.join(sorted(wanted_types))}): "
        f"{matched}/{len(required)} present, {len(missing)} missing "
        f"({len(present)} total exports)."
    )
    if missing and not args.summary_only:
        print("Missing symbols (first 40):")
        for sym in missing[:40]:
            print(f"  {sym}")
    return 1 if missing else 0


if __name__ == "__main__":
    raise SystemExit(main())
