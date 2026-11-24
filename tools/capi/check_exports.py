#!/usr/bin/env python3
"""Verify exported symbols and symbol kinds of Roxide's `libR` against `manifest.json` (ADR-0020)."""

from __future__ import annotations

import argparse
import json
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
DEFAULT_MANIFEST = ROOT / "tools" / "capi" / "manifest.json"


def list_exports(lib_path: pathlib.Path) -> dict[str, str]:
    """Return `{symbol_name: 'func' | 'data'}` for all defined global exports in `lib_path`."""
    if sys.platform == "darwin":
        cmd = ["nm", "-gU", str(lib_path)]
    else:
        cmd = ["nm", "-D", "--defined-only", str(lib_path)]
    out = subprocess.check_output(cmd, text=True)
    exports: dict[str, str] = {}
    for line in out.splitlines():
        parts = line.split()
        if len(parts) < 3:
            continue
        sym_type, raw = parts[1], parts[2]
        if sym_type in {"T", "t", "W", "i"}:
            kind = "func"
        elif sym_type in {"D", "S", "B", "R", "V"}:
            kind = "data"
        else:
            continue
        name = raw[1:] if sys.platform == "darwin" and raw.startswith("_") else raw
        exports[name] = kind
    return exports


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("libr", type=pathlib.Path, help="Path to built libR.dylib / libR.so")
    parser.add_argument("--manifest", type=pathlib.Path, default=DEFAULT_MANIFEST)
    parser.add_argument(
        "--apitypes",
        default="api,data",
        help="Comma-separated manifest apitypes required (default: api,data; e.g. api,eapi,data)",
    )
    parser.add_argument(
        "--fail-on-extra",
        action="store_true",
        help="Fail if `libR` exports symbols outside the allowed manifest & roxide_* prefix",
    )
    parser.add_argument("--summary-only", action="store_true")
    args = parser.parse_args()

    manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    wanted_types = {s.strip() for s in args.apitypes.split(",") if s.strip()}
    required: dict[str, str] = {
        row["name"]: str(row["kind"])
        for row in manifest["symbols"]
        if row["exported_in_libr"] and row["apitype"] in wanted_types
    }
    allowed_names = {
        row["name"]
        for row in manifest["symbols"]
        if row["apitype"] in {"api", "eapi", "emb", "for", "data"}
    }
    present = list_exports(args.libr)
    missing = sorted(set(required) - set(present))
    kind_mismatches = sorted(
        f"{name} (expected {req_kind}, got {present[name]})"
        for name, req_kind in required.items()
        if name in present and req_kind in {"func", "data"} and present[name] != req_kind
    )
    extra = sorted(
        name for name in present if name not in allowed_names and not name.startswith("roxide_")
    )
    matched = len(required) - len(missing)

    print(
        f"libR export coverage ({','.join(sorted(wanted_types))}): "
        f"{matched}/{len(required)} present, {len(missing)} missing, "
        f"{len(kind_mismatches)} kind mismatches, {len(extra)} unexpected exports "
        f"({len(present)} total exports)."
    )
    if missing and not args.summary_only:
        print("Missing symbols (first 40):")
        for sym in missing[:40]:
            print(f"  {sym}")
    if kind_mismatches and not args.summary_only:
        print("Kind mismatches (first 40):")
        for item in kind_mismatches[:40]:
            print(f"  {item}")
    if extra and args.fail_on_extra and not args.summary_only:
        print("Unexpected exports (first 40):")
        for sym in extra[:40]:
            print(f"  {sym}")
    failed = bool(missing or kind_mismatches or (args.fail_on_extra and extra))
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
