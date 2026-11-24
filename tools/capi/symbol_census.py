#!/usr/bin/env python3
"""Undefined-symbol census for compiled `.so` / `.o` artifacts against `manifest.json`.

Reads ONLY symbol names via `nm -u`; never reads source code.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
DEFAULT_MANIFEST = ROOT / "tools" / "capi" / "manifest.json"


def undefined_symbols(obj_path: pathlib.Path) -> set[str]:
    out = subprocess.check_output(["nm", "-u", str(obj_path)], text=True)
    syms: set[str] = set()
    for line in out.splitlines():
        tok = line.strip().split()[-1] if line.strip() else ""
        if not tok:
            continue
        if sys.platform == "darwin" and tok.startswith("_"):
            tok = tok[1:]
        syms.add(tok)
    return syms


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("objects", nargs="+", type=pathlib.Path)
    parser.add_argument("--manifest", type=pathlib.Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--libr", type=pathlib.Path, default=None, help="Optional built libR to check against")
    args = parser.parse_args()

    manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    by_name = {row["name"]: row for row in manifest["symbols"]}

    libr_exports: set[str] | None = None
    if args.libr is not None:
        from check_exports import list_exports

        libr_exports = list_exports(args.libr)

    all_undef: set[str] = set()
    for obj in args.objects:
        all_undef |= undefined_symbols(obj)

    r_syms = sorted(s for s in all_undef if s in by_name)
    by_api: dict[str, list[str]] = {}
    for sym in r_syms:
        cat = str(by_name[sym]["apitype"])
        by_api.setdefault(cat, []).append(sym)

    print(f"Total undefined symbols: {len(all_undef)}; matched R manifest symbols: {len(r_syms)}")
    for cat in sorted(by_api):
        print(f"  {cat}: {len(by_api[cat])}")

    if libr_exports is not None:
        missing = sorted(s for s in r_syms if s not in libr_exports)
        print(f"Missing from {args.libr}: {len(missing)}")
        for sym in missing:
            print(f"  {sym} ({by_name[sym]['apitype']})")
        return 1 if missing else 0
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
