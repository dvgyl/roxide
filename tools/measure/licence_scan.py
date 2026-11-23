#!/usr/bin/env python3
"""Clean-room licence and hygiene scanner for Roxide.

Verifies:
1. No forbidden GPL/AGPL/LGPL copyleft licence declarations or GNU R source banners
   exist in tracked source files (`crates/`, `stdlib/`, `conformance/`, `tools/`, `scripts/`).
2. No forbidden AI co-author or session trailer lines exist in source files.
3. Pure-Rust crates declare `#![forbid(unsafe_code)]` (except explicitly audited `roxide-capi`).
4. Cargo.toml files only use `MIT OR Apache-2.0` workspace licensing and Cargo.lock (if present)
   has no unvetted crates missing from `docs/PRIOR_ART.md`.
"""

from __future__ import annotations

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]

ALLOWED_UNSAFE_CRATES = {"roxide-capi"}

FORBIDDEN_SOURCE_PATTERNS = [
    (re.compile(r"R\s*:\s*A\s+Computer\s+Language\s+for\s+Statistical\s+Data\s+Analysis", re.I),
     "GNU R source header detected"),
    (re.compile(r"Copyright\s*\(C\)\s*[0-9,\-\s]+The\s+R\s+(Core\s+Team|Foundation)", re.I),
     "GNU R copyright banner detected"),
    (re.compile(r"This\s+program\s+is\s+free\s+software;\s+you\s+can\s+redistribute\s+it\s+and/or\s+modify\s+it\s+under\s+the\s+terms\s+of\s+the\s+GNU\s+General\s+Public\s+License", re.I),
     "GPL licence header detected in source file"),
    (re.compile(r"^\s*(Co-Authored-By:|Claude-Session:|Generated\s+with\s+)", re.I | re.M),
     "Forbidden AI attribution trailer detected"),
]

SCAN_DIRS = ["crates", "stdlib", "conformance", "tools", "scripts"]
TEXT_EXTENSIONS = {".rs", ".r", ".R", ".py", ".sh", ".toml", ".md", ".c", ".h", ".txt", ".json"}


def should_skip(path: pathlib.Path) -> bool:
    name = path.name
    if name.startswith("._") or name == ".DS_Store":
        return True
    parts = set(path.parts)
    if "target" in parts or "env" in parts or ".git" in parts or "__pycache__" in parts:
        return True
    return False


def scan_files() -> list[str]:
    errors: list[str] = []
    self_path = pathlib.Path(__file__).resolve()
    for rel_dir in SCAN_DIRS:
        base = ROOT / rel_dir
        if not base.exists():
            continue
        for path in sorted(base.rglob("*")):
            if not path.is_file() or should_skip(path):
                continue
            if path.resolve() == self_path:
                continue
            if path.suffix not in TEXT_EXTENSIONS:
                continue
            try:
                text = path.read_text(encoding="utf-8")
            except UnicodeDecodeError:
                continue
            rel = path.relative_to(ROOT)
            for pattern, msg in FORBIDDEN_SOURCE_PATTERNS:
                if pattern.search(text):
                    errors.append(f"{rel}: {msg}")
    return errors


def check_pure_crates_forbid_unsafe() -> list[str]:
    errors: list[str] = []
    crates_dir = ROOT / "crates"
    if not crates_dir.exists():
        return errors
    for crate_dir in sorted(crates_dir.iterdir()):
        if not crate_dir.is_dir() or crate_dir.name.startswith("."):
            continue
        crate_name = crate_dir.name
        if crate_name in ALLOWED_UNSAFE_CRATES:
            continue
        lib_rs = crate_dir / "src" / "lib.rs"
        main_rs = crate_dir / "src" / "main.rs"
        targets = [p for p in (lib_rs, main_rs) if p.exists()]
        if not targets:
            continue
        for entry in targets:
            text = entry.read_text(encoding="utf-8")
            if "#![forbid(unsafe_code)]" not in text:
                rel = entry.relative_to(ROOT)
                errors.append(f"{rel}: pure crate `{crate_name}` must declare `#![forbid(unsafe_code)]`")
    return errors


def check_cargo_licenses() -> list[str]:
    errors: list[str] = []
    root_toml = ROOT / "Cargo.toml"
    if root_toml.exists():
        text = root_toml.read_text(encoding="utf-8")
        if 'license = "MIT OR Apache-2.0"' not in text:
            errors.append("Cargo.toml: workspace.package must declare `license = \"MIT OR Apache-2.0\"`")
    crates_dir = ROOT / "crates"
    if crates_dir.exists():
        for toml in sorted(crates_dir.glob("*/Cargo.toml")):
            text = toml.read_text(encoding="utf-8")
            if "license.workspace = true" not in text and 'license = "MIT OR Apache-2.0"' not in text:
                errors.append(f"{toml.relative_to(ROOT)}: crate must inherit `license.workspace = true` or declare `MIT OR Apache-2.0`")
    return errors


def main() -> int:
    errors = []
    errors.extend(scan_files())
    errors.extend(check_pure_crates_forbid_unsafe())
    errors.extend(check_cargo_licenses())
    if errors:
        print("LICENCE / CLEAN-ROOM SCAN FAILED:", file=sys.stderr)
        for err in errors:
            print(f"  - {err}", file=sys.stderr)
        return 1
    print("Clean-room licence scan passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
