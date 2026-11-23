#!/usr/bin/env python3
"""Install the Tier-2 corpus in temporary libraries and compare test results."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import fcntl
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import signal
import subprocess
import sys
from tempfile import TemporaryDirectory

from fetch_tier2 import CACHE, ROOT, packages, validate_description

SHIM = ROOT / "tools" / "corpus" / "testthat_shim"
SUMMARY = re.compile(r"^\[ FAIL (\d+) \| WARN (\d+) \| SKIP (\d+) \| PASS (\d+) \]$", re.M)
TIMEOUT = 120


def run(command: list[str | Path], env: dict[str, str], cwd: Path) -> subprocess.CompletedProcess:
    with subprocess.Popen(command, cwd=cwd, env=env, text=True, encoding="utf-8",
                          errors="replace", stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                          start_new_session=True) as process:
        try:
            stdout, stderr = process.communicate(timeout=TIMEOUT)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.communicate()
            raise
        return subprocess.CompletedProcess(command, process.returncode, stdout, stderr)


def oracle_install_launcher(root: Path, rscript: Path) -> list[str | Path]:
    configured = os.environ.get("ROXIDE_ORACLE_R_HOME")
    if sys.platform != "darwin" or not configured:
        return [rscript]

    # The relocated framework's R launcher assumes /Library/Frameworks.
    # Give the installer and its child processes a temporary R home.
    original = Path(configured).resolve()
    home = root / "oracle-home"
    bin_dir = home / "bin"
    bin_dir.mkdir(parents=True)
    for entry in original.iterdir():
        if entry.name != "bin":
            (home / entry.name).symlink_to(entry)
    for entry in (original / "bin").iterdir():
        if entry.name != "R":
            (bin_dir / entry.name).symlink_to(entry)
    launcher = bin_dir / "R"
    launcher.write_text(
        "#!/bin/sh\n"
        f"export R_HOME={shlex.quote(str(home))}\n"
        'export DYLD_FALLBACK_LIBRARY_PATH="$R_HOME/lib"\n'
        'exec "$R_HOME/bin/exec/R" "$@"\n',
        encoding="ascii",
    )
    launcher.chmod(0o700)
    return [launcher, "--no-echo"]


@dataclass
class Engine:
    name: str
    executable: Path
    install_launcher: list[str | Path]
    library: Path

    def environment(self) -> dict[str, str]:
        return os.environ | {
            "R_LIBS": "",
            "R_LIBS_USER": str(self.library),
            "R_LIBS_SITE": "NULL",
            "R_MAX_VSIZE": "1G",
            "NOT_CRAN": "false",
            "CI": "false",
            "LC_ALL": "C",
        }

    def install(self, source: Path) -> None:
        flags = [f"--library={self.library}", "--no-byte-compile", source]
        if self.name == "GNU R":
            flags = ["--no-configure", *flags]
            arguments = ", ".join(json.dumps(str(flag), ensure_ascii=False) for flag in flags)
            cmd = [*self.install_launcher, "--vanilla", "-e",
                   f"tools:::.install_packages(c({arguments}), no.q = TRUE)"]
        else:
            cmd = [self.executable, "CMD", "INSTALL", *flags]
        result = run(cmd, self.environment(), self.library)
        if result.returncode:
            raise RuntimeError(f"Install failed.\n{result.stdout}{result.stderr}")

    def test(self, expression: str) -> tuple[tuple[int, ...], str]:
        result = run([self.executable, "--vanilla", "-e", expression],
                     self.environment(), self.library)
        summaries = SUMMARY.findall(result.stdout)
        if len(summaries) != 1:
            raise RuntimeError(f"Expected one test summary.\n{result.stdout}{result.stderr}")
        counts = tuple(map(int, summaries[0]))
        if result.returncode != int(counts[0] > 0):
            raise RuntimeError(f"Unexpected exit status {result.returncode}.\n"
                               f"{result.stdout}{result.stderr}")
        return counts, result.stdout


def test_expression(source: Path, package: str, description: str) -> str:
    edition = re.search(r"^Config/testthat/edition:\s*([23])\s*$", description, re.M)
    path = json.dumps(str(source / "tests" / "testthat"), ensure_ascii=False)
    name = json.dumps(package)
    setup = f"options(testthat.edition = {edition[1]}L); " if edition else ""
    return f"library(testthat); {setup}test_dir({path}, package = {name})"


def format_counts(counts: tuple[int, ...]) -> str:
    fail, warn, skip, passed = counts
    return f"{passed} pass, {fail} fail, {skip} skip, {warn} warn"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--oracle-only", action="store_true", help="Run only GNU R.")
    parser.add_argument("--shim-only", action="store_true", help="Run only the shim self-tests.")
    parser.add_argument("--roxide", type=Path, default=ROOT / "target" / "debug" / "roxide",
                        help="Path to the Roxide binary.")
    parser.add_argument("packages", metavar="PACKAGE", nargs="*")
    args = parser.parse_args()
    pins = packages()
    unknown = set(args.packages) - {package.name for package in pins}
    if unknown:
        parser.error("Unknown package names: " + ", ".join(sorted(unknown)))
    if args.shim_only and args.packages:
        parser.error("Select --shim-only or package names.")

    oracle = os.environ.get("ROXIDE_ORACLE_RSCRIPT")
    if not oracle or not Path(oracle).is_file():
        parser.error("Source env/env.sh to configure the GNU R oracle.")
    if not args.oracle_only and not args.roxide.is_file():
        parser.error(f"Roxide binary does not exist: {args.roxide}")
    rscript = Path(oracle).resolve()
    failed = False
    with TemporaryDirectory(prefix="roxide-tier2-") as temp:
        root = Path(temp)
        engines = [Engine("GNU R", rscript, oracle_install_launcher(root, rscript), root / "gnu-r")]
        if not args.oracle_only:
            engines.append(Engine("Roxide", args.roxide.resolve(), [], root / "roxide"))

        suites = [("Shim self-tests", SHIM,
                   "library(testthat); test_file(" +
                   json.dumps(str(SHIM / "tests" / "test_shim.R"), ensure_ascii=False) + ")")]
        if not args.shim_only:
            if not CACHE.is_dir():
                parser.error("Run fetch_tier2.py to fetch the packages.")
            try:
                with (CACHE / ".lock").open("a") as lock:
                    fcntl.flock(lock, fcntl.LOCK_SH)
                    for package in pins:
                        if args.packages and package.name not in args.packages:
                            continue
                        fetched = CACHE / "src" / package.name
                        if not (fetched / "DESCRIPTION").is_file():
                            parser.error(f"{package.name} is missing. Run fetch_tier2.py.")
                        source = root / "sources" / package.name
                        shutil.copytree(fetched, source)
                        description = (source / "DESCRIPTION").read_text(encoding="latin-1")
                        validate_description(package, description)
                        suites.append((package.name, source,
                                       test_expression(source, package.name, description)))
            except (OSError, ValueError) as error:
                parser.exit(1, f"{error}\n")

        for engine in engines:
            engine.library.mkdir()
        try:
            version = run([rscript, "--vanilla", "-e", "cat(as.character(getRversion()))"],
                          engines[0].environment(), root)
        except (OSError, subprocess.TimeoutExpired) as error:
            parser.exit(1, f"Cannot start GNU R: {error}\n")
        if version.returncode or version.stdout != "4.6.2":
            parser.exit(1, f"Expected GNU R 4.6.2.\n{version.stdout}{version.stderr}\n")
        shim_ready = set()
        for name, source, expression in suites:
            print(name, flush=True)
            results = []
            for engine in engines:
                if source != SHIM and engine.name not in shim_ready:
                    print(f"  {engine.name}: Shim installation failed.")
                    continue
                try:
                    engine.install(source)
                    if source == SHIM:
                        shim_ready.add(engine.name)
                    counts, output = engine.test(expression)
                except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
                    print(f"  {engine.name}: {error}")
                    failed = True
                    continue
                results.append((counts, output))
                print(f"  {engine.name}: {format_counts(counts)}")
                if counts[0]:
                    failed = True
                    for line in output.splitlines():
                        if line.startswith(("not ok ", "  ")):
                            print(f"    {line}")
            if len(results) == 2:
                matching = results[0][0] == results[1][0]
                if source == SHIM:
                    matching = matching and results[0][1] == results[1][1]
                print("  Engines match." if matching else "  Engines differ.")
                failed |= not matching
    return int(failed)


if __name__ == "__main__":
    raise SystemExit(main())
