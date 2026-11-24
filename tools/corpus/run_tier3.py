#!/usr/bin/env python3
"""Install and test the pinned Tier-3 CRAN corpus in temporary libraries."""

from __future__ import annotations

import argparse
from dataclasses import asdict, dataclass, field
import json
import math
import os
from pathlib import Path
import re
import shlex
import shutil
import signal
import subprocess
import sys
import tarfile
from tempfile import TemporaryDirectory, TemporaryFile

from fetch_tier3 import CACHE, Package, clauses, packages, unpack
from run_tier2 import SHIM, SUMMARY

ORACLE_VERSION = "4.6.2"
TAIL_BYTES = 8000
DONE = "ROXIDE_TIER3_DONE"


def literal(value: str | Path) -> str:
    return json.dumps(str(value), ensure_ascii=False)


def check_timeout(value: str) -> float:
    seconds = float(value)
    if not math.isfinite(seconds) or seconds <= 0:
        raise argparse.ArgumentTypeError("Use a finite timeout greater than zero.")
    return seconds


@dataclass
class Stage:
    status: str
    log: str = ""


@dataclass
class Test:
    status: str
    passed: int = 0
    failed: int = 0
    skipped: int = 0
    errors: int = 0
    warnings: int = 0
    log: str = ""

    def counts(self) -> tuple:
        return self.status, self.passed, self.failed, self.skipped, self.errors, self.warnings


@dataclass
class Suite:
    install: Stage
    namespace: Stage = field(default_factory=lambda: Stage("skip"))
    tests: dict[str, Test] = field(default_factory=dict)


def mac_oracle_launcher(root: Path, original: Path) -> Path:
    home = root / "oracle-home"
    (home / "bin").mkdir(parents=True)
    for item in ("bin/exec", "bin/Rcmd", "bin/Rscript", "bin/SHLIB", "bin/INSTALL",
                 "doc", "etc", "include", "lib", "library", "modules", "share"):
        (home / item).symlink_to(original / item)

    (home / "Makevars").write_text('LIBR = -L"$(R_HOME)/lib" -lR\n', encoding="utf-8")
    launcher = home / "bin" / "R"
    launcher.write_text(
        "#!/bin/sh\n"
        f"export R_HOME={shlex.quote(str(home))}\n"
        'export R_SHARE_DIR="$R_HOME/share" R_INCLUDE_DIR="$R_HOME/include"\n'
        'export R_DOC_DIR="$R_HOME/doc"\n'
        'export R_MAKEVARS_USER="$R_HOME/Makevars"\n'
        'export DYLD_FALLBACK_LIBRARY_PATH="$R_HOME/lib"\n'
        'if [ "$1" = CMD ]; then\n'
        '    shift\n'
        '    exec "$R_HOME/bin/Rcmd" "$@"\n'
        'fi\n'
        'exec "$R_HOME/bin/exec/R" "$@"\n',
        encoding="utf-8",
    )
    launcher.chmod(0o700)
    return launcher


@dataclass(frozen=True)
class Engine:
    name: str
    r: Path
    script: list[str | Path]
    root: Path
    timeout: float

    @property
    def library(self) -> Path:
        return self.root / "library"

    def run(self, command: list[str | Path], cwd: Path) -> Stage:
        env = {key: value for key, value in os.environ.items()
               if key not in ("R_HOME", "R_TESTS") and
               not key.startswith(("R_LIBS", "R_PROFILE", "R_ENVIRON"))}
        env.update({
            "R_LIBS": str(self.library), "R_LIBS_USER": str(self.library), "R_LIBS_SITE": "NULL",
            "R_MAKEVARS_USER": os.devnull, "R_MAX_VSIZE": "1G", "MAKEFLAGS": "-j1",
            "NOT_CRAN": "false", "CI": "false", "LC_ALL": "C", "R_DEFAULT_PACKAGES":
            "datasets,utils,grDevices,graphics,stats,methods",
        })
        if self.name == "roxide":
            env["R_HOME"] = str(self.r.parent.parent)
        with TemporaryFile() as output:
            try:
                with subprocess.Popen(command, cwd=cwd, env=env, stdout=output,
                                      stderr=subprocess.STDOUT, start_new_session=True) as process:
                    try:
                        code = process.wait(timeout=self.timeout)
                        status = "pass" if code == 0 else "fail"
                        detail = "" if code == 0 else f"Process exit status {code}.\n"
                    except subprocess.TimeoutExpired:
                        try:
                            os.killpg(process.pid, signal.SIGKILL)
                        except ProcessLookupError:
                            pass
                        process.wait()
                        status = "error"
                        detail = f"Time limit reached ({self.timeout:g} seconds).\n"
            except OSError as error:
                return Stage("error", str(error))
            size = output.tell()
            output.seek(max(0, size - TAIL_BYTES))
            return Stage(status, detail + output.read().decode("utf-8", errors="replace").strip())

    def install(self, source: Path) -> Stage:
        startup = ["--use-vanilla"] if self.name == "oracle" else []
        return self.run([self.r, "CMD", "INSTALL", *startup, "--no-multiarch", "--no-byte-compile",
                         f"--library={self.library}", source], self.root)

    def evaluate(self, expression: str, cwd: Path) -> Stage:
        return self.run([*self.script, "--vanilla", "-e", expression], cwd)

    def namespace(self, pin: Package) -> Stage:
        name = literal(pin.package)
        compiled = "TRUE" if pin.needs_compilation else "FALSE"
        result = self.evaluate(
            f"ns <- loadNamespace({name}); dlls <- getLoadedDLLs(); "
            f"if ({compiled}) stopifnot({name} %in% names(dlls)); "
            f'cat("{DONE}\\n")', self.root)
        if result.status == "pass" and not has_marker(result.log):
            return Stage("error", result.log)
        return result


def has_marker(log: str) -> bool:
    return DONE in log.splitlines()


def test_plan(source: Path, pin: Package) -> dict[str, bool]:
    directory = source / "tests"
    plan = {}
    if pin.test_runner == "shim":
        plan.update((path.relative_to(directory).as_posix(), True) for path in
                    (directory / "testthat").glob("*")
                    if path.is_file() and re.fullmatch(r"test.*\.[rR]", path.name))
    for path in directory.glob("*"):
        if path.is_file() and path.suffix.lower() in (".r", ".rin"):
            if plan and path.name.lower() == "testthat.r":
                continue
            plan[path.name] = False
    return dict(sorted(plan.items()))


def test_file(engine: Engine, pin: Package, fields: dict[str, str],
              path: Path, shim: bool) -> Test:
    if path.suffix.lower() == ".rin":
        return Test("skip", skipped=1, log="This script needs R CMD check input processing.")
    seed = "set.seed(1L); "
    if shim:
        optional = ", ".join(literal(dep.split()[0]) for dep in clauses(fields, "Suggests"))
        edition = fields.get("Config/testthat/edition", "2")
        options = (f"options(testthat.edition = {int(edition)}L, "
                   f"testthat.optional_packages = c({optional})); ")
        expression = (
            f"library(testthat); {seed}{options}"
            f"testthat:::run_test_files({literal(path.parent)}, {literal(path.name)}, "
            f"{literal(pin.package)}, NULL, FALSE); "
        )
    else:
        expression = f"{seed}source({literal(path.name)}, chdir = TRUE); "
    result = engine.evaluate(expression + f'cat("{DONE}\\n")', path.parent)
    if result.status != "pass" or not has_marker(result.log):
        return Test("error", errors=1, log=result.log)
    if not shim:
        return Test("pass", passed=1, log=result.log)
    summaries = SUMMARY.findall(result.log)
    if len(summaries) != 1:
        return Test("error", errors=1, log="Expected one shim summary.\n" + result.log)
    failed, warnings, skipped, passed = map(int, summaries[0])
    status = "fail" if failed else "pass" if passed else "skip"
    return Test(status, passed, failed, skipped, warnings=warnings, log=result.log)


def run_suite(engine: Engine, pins: list[Package], selected: set[str],
              cache: Path) -> dict[str, Suite]:
    engine.library.mkdir(parents=True)
    if engine.name == "oracle":
        version = engine.evaluate("cat(as.character(getRversion()))", engine.root)
        if version.status != "pass" or version.log != ORACLE_VERSION:
            raise ValueError(f"Expected GNU R {ORACLE_VERSION}.\n{version.log}")

    shim = None
    results = {}
    descriptions = {}
    sources = engine.root / "sources"
    sources.mkdir()
    for pin in pins:
        print(f"{engine.name} {pin.package} {pin.version}", flush=True)
        descriptions[pin.package] = unpack(pin, cache / pin.filename, sources)
        source = sources / pin.package
        blocked = sorted(name for name in pin.dependencies()
                         if results[name].namespace.status != "pass")
        if blocked:
            suite = Suite(Stage("skip", "Unavailable dependencies: " + ", ".join(blocked)))
        else:
            suite = Suite(engine.install(source))
            if suite.install.status == "pass":
                suite.namespace = engine.namespace(pin)
        results[pin.package] = suite

    for pin in pins:
        if pin.package not in selected:
            continue
        suite = results[pin.package]
        source = sources / pin.package
        for name, use_shim in test_plan(source, pin).items():
            if suite.namespace.status != "pass":
                suite.tests[name] = Test("skip", skipped=1, log="Package namespace is unavailable.")
                continue
            if use_shim and shim is None:
                shim = engine.install(SHIM)
            if use_shim and shim.status != "pass":
                suite.tests[name] = Test("error", errors=1, log=shim.log)
            else:
                suite.tests[name] = test_file(
                    engine, pin, descriptions[pin.package], source / "tests" / name, use_shim)
    return results


def package_selection(pins: list[Package], names: str | None) -> tuple[list[Package], set[str]]:
    if names is None:
        return pins, {pin.package for pin in pins}
    selected = {name.strip() for name in names.split(",")}
    unknown = selected - {pin.package for pin in pins}
    if unknown:
        raise ValueError("Unknown packages: " + ", ".join(repr(name) for name in sorted(unknown)))
    install = set(selected)
    for pin in reversed(pins):
        if pin.package in install:
            install.update(pin.dependencies())
    return [pin for pin in pins if pin.package in install], selected


def summarize(pins: list[Package], results: dict[str, dict[str, Suite]]) -> bool:
    engines = list(results)
    compare = len(engines) == 2
    header = ["Package", "Check", *engines, *(["Match"] if compare else [])]
    print("| " + " | ".join(header) + " |")
    print("|" + "|".join("---" for _ in header) + "|")
    failed = False

    def row(package: str, check: str, values: list[Stage | Test]) -> None:
        nonlocal failed
        metrics = [value.counts() if isinstance(value, Test) else (value.status,) for value in values]
        match = len(set(metrics)) == 1
        formatted = [value.status if isinstance(value, Stage) else
                     f"{value.passed} pass, {value.failed} fail, {value.skipped} skip, "
                     f"{value.errors} error, {value.warnings} warn" for value in values]
        fields = [package, check, *formatted, *(["yes" if match else "no"] if compare else [])]
        print("| " + " | ".join(value.replace("|", "\\|") for value in fields) + " |")
        failed |= not match or any(value.status in ("fail", "error") for value in values)

    for pin in pins:
        suites = [results[engine][pin.package] for engine in engines]
        row(pin.package, "Install", [suite.install for suite in suites])
        row(pin.package, "Namespace and DLLs", [suite.namespace for suite in suites])
        for name in sorted(set().union(*(suite.tests for suite in suites))):
            row(pin.package, name, [suite.tests.get(name, Test("error", errors=1))
                                    for suite in suites])
    return failed


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tier", choices=("a", "b", "all"), default="all")
    parser.add_argument("--engine", choices=("oracle", "roxide", "both"), default="both")
    parser.add_argument("--roxide-rhome", type=Path)
    parser.add_argument("--cache-dir", type=Path, default=CACHE)
    parser.add_argument("--packages", help="Comma-separated list of packages to test.")
    parser.add_argument("--timeout", type=check_timeout, default=120, help="Seconds per command.")
    parser.add_argument("--json-out", type=Path)
    args = parser.parse_args()
    try:
        pins, selected = package_selection(packages(args.tier), args.packages)
        cache = args.cache_dir.resolve()
        results = {}
        with TemporaryDirectory(prefix="roxide-tier3-") as temp:
            root = Path(temp)
            engines = []
            if args.engine in ("both", "oracle"):
                oracle = os.environ.get("ROXIDE_ORACLE_R") or shutil.which("R")
                rscript = os.environ.get("ROXIDE_ORACLE_RSCRIPT") or shutil.which("Rscript")
                configured = os.environ.get("ROXIDE_ORACLE_R_HOME")
                if sys.platform == "darwin" and configured:
                    oracle = mac_oracle_launcher(root, Path(configured).absolute())
                    script = [oracle, "--no-echo"]
                else:
                    if not oracle or not rscript:
                        parser.error("Set ROXIDE_ORACLE_R and ROXIDE_ORACLE_RSCRIPT.")
                    oracle = Path(oracle).absolute()
                    script = [Path(rscript).absolute()]
                engines.append(Engine("oracle", oracle, script, root / "oracle", args.timeout))
            if args.engine in ("both", "roxide"):
                if not args.roxide_rhome:
                    parser.error("Specify --roxide-rhome for the Roxide engine.")
                binary = args.roxide_rhome.resolve() / "bin" / "R"
                if not binary.is_file():
                    parser.error(f"Roxide launcher is missing: {binary}")
                engines.append(Engine("roxide", binary, [binary, "--quiet"], root / "roxide",
                                      args.timeout))
            for engine in engines:
                results[engine.name] = run_suite(engine, pins, selected, cache)
        if args.json_out:
            args.json_out.write_text(json.dumps({
                "tier": args.tier,
                "packages": {pin.package: pin.version for pin in pins},
                "tested_packages": sorted(selected),
                "engines": {name: {package: asdict(suite) for package, suite in suites.items()}
                            for name, suites in results.items()},
            }, indent=2) + "\n")
        return int(summarize(pins, results))
    except (OSError, EOFError, tarfile.TarError, ValueError, KeyError, TypeError) as error:
        parser.exit(1, f"{error}\n")


if __name__ == "__main__":
    raise SystemExit(main())
