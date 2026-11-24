#!/usr/bin/env python3
"""Fetch and verify the pinned permissive Tier-3 CRAN archives."""

from __future__ import annotations

import argparse
from contextlib import contextmanager
from dataclasses import dataclass
import gzip
import hashlib
import io
import json
import operator
from pathlib import Path, PurePosixPath
import re
import shutil
import tarfile
from tempfile import TemporaryFile, TemporaryDirectory
from urllib.error import HTTPError
from urllib.request import urlopen

from fetch_tier2 import ROOT, MAX_DOWNLOAD, MAX_UNPACKED, MAX_ENTRIES

CACHE = ROOT / "target" / "corpus" / "tier3"
MANIFEST_DIR = ROOT / "conformance" / "layer4" / "tier3"
CRAN = "https://cran.r-project.org"
MIRROR = "https://raw.githubusercontent.com/cran"
LICENSES = {"MIT", "BSD_2_clause", "BSD_3_clause", "Apache_2_0", "ISC", "CC0", "Unlicense"}
BASE = frozenset("""
    R base compiler datasets grDevices graphics grid methods parallel splines stats
    stats4 tcltk tools utils
""".split())
DEPENDENCY_FIELDS = {"Depends": "depends", "Imports": "imports", "LinkingTo": "linking_to"}
OPERATORS = {">=": operator.ge, "<=": operator.le, "==": operator.eq,
             ">": operator.gt, "<": operator.lt, "!=": operator.ne}
NAME = r"[A-Za-z][A-Za-z0-9.]*"
VERSION = r"[0-9]+(?:[.-][0-9]+)+"
DEPENDENCY = re.compile(rf"({NAME})(?:\s*\((>=|<=|==|!=|>|<)\s*({VERSION})\))?")
MAX_DESCRIPTION = 128 * 1024


@dataclass(frozen=True)
class Package:
    package: str
    version: str
    license: str
    sha256: str
    url: str
    needs_compilation: bool
    depends: list[str]
    imports: list[str]
    linking_to: list[str]
    test_runner: str
    bundled_licenses: dict[str, str]

    @property
    def filename(self) -> str:
        return f"{self.package}_{self.version}.tar.gz"

    @property
    def archive_url(self) -> str:
        return f"{CRAN}/src/contrib/Archive/{self.package}/{self.filename}"

    def requirements(self):
        for clause in self.depends + self.imports + self.linking_to:
            match = DEPENDENCY.fullmatch(clause)
            if match is None:
                raise ValueError(f"{self.package}: Invalid requirement {clause!r}.")
            yield match.groups()

    def dependencies(self) -> set[str]:
        return {name for name, _, _ in self.requirements()} - BASE


def version_key(version: str) -> tuple[int, ...]:
    parts = [int(part) for part in re.split(r"[.-]", version)]
    while len(parts) > 1 and parts[-1] == 0:
        parts.pop()
    return tuple(parts)


def packages(tier: str) -> list[Package]:
    manifests = ("a", "b") if tier == "all" else (tier,)
    pins = {}
    for letter in manifests:
        earlier = {}
        entries = json.loads((MANIFEST_DIR / f"manifest_{letter}.json").read_text())["packages"]
        for entry in entries:
            pin = Package(**entry)
            if (not re.fullmatch(NAME, pin.package) or pin.package in BASE or
                    not re.fullmatch(VERSION, pin.version) or
                    not re.fullmatch(r"[a-f0-9]{64}", pin.sha256) or
                    pin.url not in (f"{CRAN}/src/contrib/{pin.filename}", pin.archive_url) or
                    pin.test_runner not in ("shim", "custom") or
                    type(pin.needs_compilation) is not bool):
                raise ValueError(f"{pin.package}: Invalid manifest entry.")
            if pin.license not in LICENSES or set(pin.bundled_licenses.values()) - LICENSES:
                raise ValueError(f"{pin.package}: Licence is outside the allowlist.")
            if pin.package in earlier or pins.get(pin.package, pin) != pin:
                raise ValueError(f"{pin.package}: Duplicate or conflicting pin.")
            for name, comparison, minimum in pin.requirements():
                if name not in BASE and name not in earlier:
                    raise ValueError(f"{pin.package}: Add the pin for {name} before this entry.")
                version = earlier[name].version if name in earlier else "4.6.2"
                if comparison and not OPERATORS[comparison](version_key(version), version_key(minimum)):
                    raise ValueError(f"{pin.package}: {name} {version} does not satisfy "
                                     f"{comparison} {minimum}.")
            earlier[pin.package] = pins[pin.package] = pin
    return list(pins.values())


def description_fields(data: bytes) -> dict[str, str]:
    fields = {}
    key = ""
    for line in data.decode("latin-1").splitlines():
        if not line.strip():
            break
        if line[0].isspace() and key:
            fields[key] += " " + line.strip()
        else:
            key, separator, value = line.partition(":")
            if not separator or not key or key in fields:
                raise ValueError("Invalid DESCRIPTION field.")
            fields[key] = value.strip()
    return fields


def clauses(fields: dict[str, str], field: str) -> list[str]:
    return [clause.strip() for clause in fields.get(field, "").split(",") if clause.strip()]


def validate_description(pin: Package, fields: dict[str, str]) -> None:
    license_name = fields.get("License", "")
    license_id = re.sub(r" \+ file LICENSE$", "", license_name)
    if license_name == "Apache License (== 2.0) | file LICENSE":
        license_id = "Apache_2_0"
    license_id = {"Apache License (== 2.0)": "Apache_2_0", "Apache-2.0": "Apache_2_0",
                  "CC0-1.0": "CC0"}.get(license_id, license_id)
    if license_id not in LICENSES or license_id != pin.license:
        raise ValueError(f"{pin.package}: Licence does not match the allowlist and pin: {license_name!r}.")
    if (fields.get("Package"), fields.get("Version")) != (pin.package, pin.version):
        raise ValueError(f"{pin.package}: DESCRIPTION name or version does not match the pin.")
    if fields.get("NeedsCompilation") != ("yes" if pin.needs_compilation else "no"):
        raise ValueError(f"{pin.package}: NeedsCompilation does not match the pin.")
    for field, attribute in DEPENDENCY_FIELDS.items():
        if clauses(fields, field) != getattr(pin, attribute):
            raise ValueError(f"{pin.package}: {field} does not match the pin.")


def download(url: str, limit: int) -> bytes:
    with urlopen(url, timeout=30) as response:
        data = response.read(limit + 1)
    if len(data) > limit:
        raise ValueError(f"Download exceeds the size limit: {url}")
    return data


def preflight(pin: Package) -> None:
    current = f"{CRAN}/web/packages/{pin.package}/DESCRIPTION"
    try:
        fields = description_fields(download(current, MAX_DESCRIPTION))
    except HTTPError as error:
        if error.code != 404:
            raise
        fields = {}
    if fields.get("Version") != pin.version:
        url = f"{MIRROR}/{pin.package}/{pin.version}/DESCRIPTION"
        fields = description_fields(download(url, MAX_DESCRIPTION))
    validate_description(pin, fields)


def check_digest(pin: Package, data: bytes) -> None:
    if len(data) > MAX_DOWNLOAD or hashlib.sha256(data).hexdigest() != pin.sha256:
        raise ValueError(f"{pin.package}: Archive exceeds the size limit or has the wrong SHA-256.")


def read_archive(pin: Package, path: Path) -> bytes:
    with path.open("rb") as stream:
        data = stream.read(MAX_DOWNLOAD + 1)
    check_digest(pin, data)
    return data


def archive_members(pin: Package, archive: tarfile.TarFile) -> dict[str, tarfile.TarInfo]:
    members = {}
    seen = {}
    for member in archive:
        path = PurePosixPath(member.name)
        parts = path.parts
        if (not parts or parts[0] != pin.package or ".." in parts or
                "\\" in member.name or member.issparse() or
                member.type not in (tarfile.REGTYPE, tarfile.AREGTYPE, tarfile.DIRTYPE) or
                not 0 <= member.size <= MAX_UNPACKED or
                (len(parts) == 1 and not member.isdir())):
            raise ValueError(f"{pin.package}: Invalid archive entry {member.name!r}.")
        if path.as_posix().casefold() in seen or len(members) >= MAX_ENTRIES:
            raise ValueError(f"{pin.package}: Duplicate entry or too many archive entries.")
        members[path.as_posix()] = member
        seen[path.as_posix().casefold()] = member

    for name in members:
        for parent in PurePosixPath(name).parents:
            folded = seen.get(parent.as_posix().casefold())
            if folded and (not folded.isdir() or parent.as_posix() not in members):
                raise ValueError(f"{pin.package}: Invalid parent directory for {name!r}.")
    return members


@contextmanager
def checked_archive(pin: Package, data: bytes):
    check_digest(pin, data)
    with gzip.GzipFile(fileobj=io.BytesIO(data)) as compressed, TemporaryFile() as raw:
        size = 0
        while chunk := compressed.read(1024 * 1024):
            size += len(chunk)
            if size > MAX_UNPACKED:
                raise ValueError(f"{pin.package}: Archive exceeds the decompression limit.")
            raw.write(chunk)
        raw.seek(0)
        with tarfile.open(fileobj=raw, mode="r:") as archive:
            members = archive_members(pin, archive)
            desc = members.get(f"{pin.package}/DESCRIPTION")
            if not desc or not desc.isfile() or desc.size > MAX_DESCRIPTION:
                raise ValueError(f"{pin.package}: Missing or oversized DESCRIPTION.")
            with archive.extractfile(desc) as stream:
                fields = description_fields(stream.read())
            validate_description(pin, fields)
            yield archive, members, fields


def unpack(pin: Package, path: Path, directory: Path) -> dict[str, str]:
    with checked_archive(pin, read_archive(pin, path)) as (archive, members, fields):
        (directory / pin.package).mkdir()
        for name, member in members.items():
            destination = directory / name
            if member.isdir():
                destination.mkdir(parents=True, exist_ok=True)
            else:
                destination.parent.mkdir(parents=True, exist_ok=True)
                with archive.extractfile(member) as source, destination.open("xb") as output:
                    shutil.copyfileobj(source, output)
                destination.chmod(0o644 | (member.mode & 0o111))
    return fields


def fetch(pin: Package, directory: Path, verify_only: bool) -> None:
    path = directory / pin.filename
    try:
        data = read_archive(pin, path)
    except (FileNotFoundError, ValueError):
        if verify_only:
            raise
    else:
        with checked_archive(pin, data):
            return
    try:
        data = download(pin.url, MAX_DOWNLOAD)
    except HTTPError as error:
        if error.code != 404 or pin.url == pin.archive_url:
            raise
        data = download(pin.archive_url, MAX_DOWNLOAD)
    with checked_archive(pin, data), TemporaryDirectory(dir=directory) as temp:
        partial = Path(temp) / pin.filename
        partial.write_bytes(data)
        partial.replace(path)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tier", choices=("a", "b", "all"), default="all")
    parser.add_argument("--cache-dir", type=Path, default=CACHE)
    parser.add_argument("--verify-only", action="store_true", help="Check the cache without the network.")
    args = parser.parse_args()
    try:
        pins = packages(args.tier)
        if not args.verify_only:
            for pin in pins:
                preflight(pin)
            args.cache_dir.mkdir(parents=True, exist_ok=True)
        for pin in pins:
            fetch(pin, args.cache_dir, args.verify_only)
            print(f"{pin.package} {pin.version} ({pin.license}) verified.", flush=True)
    except (OSError, EOFError, tarfile.TarError, ValueError, KeyError, TypeError) as error:
        parser.exit(1, f"{error}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
