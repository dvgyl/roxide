#!/usr/bin/env python3
"""Fetch and validate the pinned Tier-2 CRAN packages."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import fcntl
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import shutil
import tarfile
from tempfile import TemporaryDirectory
from urllib.request import urlopen

ROOT = Path(__file__).resolve().parents[2]
CACHE = ROOT / "env" / "corpus" / "tier2"
MANIFEST = ROOT / "conformance" / "layer4" / "tier2" / "manifest.json"
MAX_DOWNLOAD = 16 * 1024 * 1024
MAX_UNPACKED = 64 * 1024 * 1024
MAX_ENTRIES = 4096
PERMISSIVE = re.compile(
    r"(?:MIT|BSD(?:_[23]_clause|-[23]-Clause)?|Apache(?: License)?(?: \(== 2\.0\)|-2\.0)?)"
    r"(?: \+ file LICENSE)?",
    re.IGNORECASE,
)


@dataclass(frozen=True)
class Package:
    name: str
    version: str
    sha256: str
    urls: list[str]

    @property
    def filename(self) -> str:
        return f"{self.name}_{self.version}.tar.gz"


def packages() -> list[Package]:
    return [Package(**entry) for entry in json.loads(MANIFEST.read_text())["packages"]]


def fetch_archive(package: Package, directory: Path) -> Path:
    destination = directory / package.filename
    if destination.is_file():
        with destination.open("rb") as cached:
            if hashlib.sha256(cached.read(MAX_DOWNLOAD + 1)).hexdigest() == package.sha256:
                return destination

    last_error = None
    with TemporaryDirectory(dir=directory) as temp:
        partial = Path(temp) / package.filename
        for url in package.urls:
            try:
                with urlopen(url, timeout=30) as response:
                    data = response.read(MAX_DOWNLOAD + 1)
            except OSError as error:
                last_error = error
                continue
            if len(data) > MAX_DOWNLOAD:
                raise ValueError(f"{package.name}: Archive exceeds the download limit.")
            if hashlib.sha256(data).hexdigest() != package.sha256:
                raise ValueError(f"{package.name}: SHA-256 does not match the manifest.")
            partial.write_bytes(data)
            partial.replace(destination)
            return destination
    raise OSError(f"{package.name}: Cannot download the archive: {last_error}")


def validate_description(package: Package, description: str) -> str:
    fields: dict[str, str] = {}
    key = ""
    for line in description.splitlines():
        if not line.strip():
            break
        if line[0].isspace() and key:
            fields[key] += " " + line.strip()
        else:
            key, separator, value = line.partition(":")
            if not separator or key in fields:
                raise ValueError(f"{package.name}: Invalid DESCRIPTION field.")
            fields[key] = value.strip()

    if fields.get("Package") != package.name or fields.get("Version") != package.version:
        raise ValueError(f"{package.name}: DESCRIPTION does not match the manifest.")
    license_name = fields.get("License", "")
    if not PERMISSIVE.fullmatch(license_name):
        raise ValueError(f"{package.name}: Licence is outside the allowlist: {license_name!r}")
    if fields.get("NeedsCompilation", "no").lower() != "no" or fields.get("LinkingTo"):
        raise ValueError(f"{package.name}: DESCRIPTION declares compiled code.")
    return license_name


def archive_members(package: Package, archive: tarfile.TarFile) -> list[tarfile.TarInfo]:
    members = []
    seen = set()
    size = 0
    for member in archive:
        path = PurePosixPath(member.name)
        parts = path.parts
        if (not parts or parts[0] != package.name or ".." in parts or
                "\\" in member.name or not (member.isfile() or member.isdir())):
            raise ValueError(f"{package.name}: Invalid archive entry: {member.name!r}")
        if len(parts) > 1 and parts[1].casefold() == "src":
            raise ValueError(f"{package.name}: Archive contains a src directory.")
        name = path.as_posix().casefold()
        if name in seen or (len(parts) == 1 and not member.isdir()):
            raise ValueError(f"{package.name}: Invalid duplicate or package root: {member.name!r}")
        seen.add(name)
        size += member.size
        if size > MAX_UNPACKED or len(members) >= MAX_ENTRIES:
            raise ValueError(f"{package.name}: Archive exceeds extraction limits.")
        members.append(member)
    return members


def unpack(package: Package, archive_path: Path, directory: Path) -> str:
    destination = directory / package.name
    with tarfile.open(archive_path, "r:gz") as archive:
        members = archive_members(package, archive)
        description = archive.extractfile(f"{package.name}/DESCRIPTION")
        if description is None:
            raise ValueError(f"{package.name}: Archive has no DESCRIPTION file.")
        with description:
            license_name = validate_description(package, description.read().decode("latin-1"))

        with TemporaryDirectory(dir=directory) as temp:
            stage = Path(temp) / "stage"
            for member in members:
                path = stage / member.name
                if member.isdir():
                    path.mkdir(parents=True, exist_ok=True)
                else:
                    path.parent.mkdir(parents=True, exist_ok=True)
                    with archive.extractfile(member) as content, path.open("xb") as output:
                        shutil.copyfileobj(content, output)
            previous = Path(temp) / "previous"
            if destination.exists():
                destination.rename(previous)
            try:
                (stage / package.name).rename(destination)
            except OSError:
                if previous.exists():
                    previous.rename(destination)
                raise
    return license_name


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.parse_args()
    archives = CACHE / "archives"
    sources = CACHE / "src"
    archives.mkdir(parents=True, exist_ok=True)
    sources.mkdir(parents=True, exist_ok=True)
    try:
        with (CACHE / ".lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            for package in packages():
                archive = fetch_archive(package, archives)
                license_name = unpack(package, archive, sources)
                print(f"{package.name} {package.version}: {license_name}")
    except (OSError, tarfile.TarError, ValueError, KeyError) as error:
        parser.exit(1, f"{error}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
