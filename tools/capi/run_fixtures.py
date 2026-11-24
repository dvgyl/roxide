#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
"""Run the native C API fixtures on macOS arm64 with GNU R 4.6.2 and Roxide."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import difflib
import fnmatch
import math
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parents[2]
DRIVERS = ROOT / "tests" / "capi"
INCLUDE = ROOT / "crates" / "roxide-capi" / "include"
NEGATIVE = "06_negative_roxide_only"
STACK_LINE = re.compile(rb"(?m)^(Warning: stack imbalance in '[^'\n]+', )\d+ then \d+$")
POINTER = re.compile(rb"(?<![\w.])0x[0-9a-fA-F]+(?![\w.])")


@dataclass(frozen=True)
class Result:
    status: int
    stdout: bytes
    stderr: bytes


def normalize(output: bytes, scratch: Path) -> bytes:
    for path in (scratch, scratch.resolve()):
        output = output.replace(os.fsencode(path) + b"/", b"<temp>/")
    output = POINTER.sub(b"<address>", output)
    output = STACK_LINE.sub(rb"\g<1><before> then <after>", output)
    return re.sub(rb"[ \t]+(?=\r?\n|$)", b"", output)


def run(command: list[str], scratch: Path, env: dict[str, str], timeout: float) -> Result:
    try:
        process = subprocess.run(command, cwd=scratch, env=env, timeout=timeout,
                                 capture_output=True, check=False)
    except subprocess.TimeoutExpired as error:
        raise RuntimeError(f"{Path(command[0]).name} exceeded {timeout:g} seconds.") from error
    return Result(process.returncode, normalize(process.stdout, scratch),
                  normalize(process.stderr, scratch))


def show_output(result: Result) -> None:
    for stream, data in (("stdout", result.stdout), ("stderr", result.stderr)):
        if data:
            print(f"  {stream} = {data!r}", file=sys.stderr)


def compile_library(source: Path, output: Path, include: Path, flags: list[str],
                    env: dict[str, str], timeout: float) -> None:
    compiler = ["clang++", "-std=gnu++17"] if source.suffix == ".cc" else ["clang", "-std=c11"]
    command = [*compiler, "-dynamiclib", "-arch", "arm64", "-fno-caret-diagnostics",
               f"-I{include}", str(source), "-o", str(output), *flags]
    result = run(command, output.parent, env, timeout)
    if result.status:
        show_output(result)
        raise RuntimeError(f"Compilation of {output.name} failed with status {result.status}.")


def build_fixture(name: str, scratch: Path, rhome: Path | None,
                  env: dict[str, str], timeout: float) -> dict[str, str]:
    include = INCLUDE if rhome is None else rhome / "include"
    flags = (["-undefined", "dynamic_lookup", "-w"] if rhome is None
             else [f"-L{rhome / 'lib'}", "-lR"])
    fixtures = DRIVERS / "fixtures"
    extension = ".cc" if (fixtures / f"{name}.cc").is_file() and name != NEGATIVE else ".c"
    source = fixtures / (name + extension)
    library = scratch / f"f{name}.so"
    compile_library(source, library, include, flags, env, timeout)
    paths = {"ROXIDE_FIXTURE_SO": str(library)}

    if name == NEGATIVE:
        cpp = scratch / "fixture_exception.so"
        compile_library(source.with_suffix(".cc"), cpp, include, flags, env, timeout)
        # Give the foreign dependency a valid load path without using GNU R libraries.
        stub = scratch / "libR.dylib"
        compile_library(source, stub, include,
                        ["-DFIXTURE_FOREIGN_STUB", "-Wl,-install_name,@rpath/libR.dylib"],
                        env, timeout)
        foreign = scratch / "fixture_foreign.so"
        compile_library(source, foreign, include,
                        ["-DFIXTURE_FOREIGN_CLIENT", str(stub), "-Wl,-rpath,@loader_path"],
                        env, timeout)
        paths.update(ROXIDE_FIXTURE_CPP_SO=str(cpp), ROXIDE_FIXTURE_FOREIGN_SO=str(foreign),
                     ROXIDE_FIXTURE_LOAD_MARKER=str(scratch / "foreign-loaded"))
    return paths


def expect_result(name: str, result: Result) -> None:
    if result.status:
        raise RuntimeError(f"Rscript exited with status {result.status}.")
    markers = {
        "01_vectors_attrib": (result.stderr, b"Warning: stack imbalance in '.Call', <before> then <after>"),
        "04_mem_extptr_weakref_rmath": (result.stdout, b"Exit finalizer ran."),
        "05_dotcall_register_ccallable": (result.stdout, b"Native unload ran."),
    }
    if name in markers:
        output, marker = markers[name]
        if output.splitlines().count(marker) != 1:
            raise RuntimeError(f"Expected one output line {marker!r}.")


def compare(name: str, baseline: Result, candidate: Result,
            reference: str, engine: str) -> bool:
    if baseline == candidate:
        return True
    print(f"FAIL {name} ({reference} vs {engine})", file=sys.stderr)
    if baseline.status != candidate.status:
        print(f"  Exit status: {baseline.status} vs {candidate.status}", file=sys.stderr)
    for stream in ("stdout", "stderr"):
        expected = getattr(baseline, stream)
        actual = getattr(candidate, stream)
        if expected != actual:
            diff = difflib.diff_bytes(difflib.unified_diff,
                                      expected.splitlines(keepends=True),
                                      actual.splitlines(keepends=True),
                                      f"{reference} {stream}".encode(),
                                      f"{engine} {stream}".encode())
            for line in diff:
                print(f"  {line!r}", file=sys.stderr)
    return False


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--engine", choices=("oracle", "roxide", "both"), default="both")
    parser.add_argument("--roxide-rhome", type=Path, default=ROOT / "target" / "rhome",
                        help="Path to a staged Roxide R_HOME.")
    parser.add_argument("--torture", action="store_true",
                        help="Repeat each Roxide fixture with garbage collection torture.")
    parser.add_argument("--include-negative", action="store_true",
                        help="Run the safety fixtures on Roxide.")
    parser.add_argument("--filter", default="*", help="Shell pattern for the fixture name.")
    parser.add_argument("--timeout", type=float, default=120,
                        help="Time limit in seconds for each compile or Rscript process.")
    args = parser.parse_args()
    if not math.isfinite(args.timeout) or args.timeout <= 0:
        parser.error("--timeout must be a finite positive number.")
    if args.engine == "oracle" and (args.include_negative or args.torture):
        parser.error("--include-negative and --torture require a Roxide engine.")

    drivers = [path for path in sorted(DRIVERS.glob("*.R"))
               if fnmatch.fnmatchcase(path.stem, args.filter)
               and (path.stem != NEGATIVE or args.include_negative)]
    if not drivers:
        parser.error("No fixtures match the filter.")

    env = os.environ.copy()
    for key in ("CPATH", "C_INCLUDE_PATH", "CPLUS_INCLUDE_PATH", "LIBRARY_PATH", "R_HOME",
                "ROXIDE_GC_TORTURE", "ROXIDE_GC_TORTURE_BOOT"):
        env.pop(key, None)
    env.update(LC_ALL="C", TZ="UTC", R_MAX_VSIZE="512M")

    scripts = {}
    rhome = args.roxide_rhome.resolve()
    if args.engine != "roxide":
        oracle = env.get("ROXIDE_ORACLE_RSCRIPT")
        if not oracle:
            parser.error("Set ROXIDE_ORACLE_RSCRIPT by sourcing env/env.sh.")
        scripts["oracle"] = oracle
    if args.engine != "oracle":
        scripts["roxide"] = str(rhome / "bin" / "Rscript")
        if not (rhome / "bin" / "Rscript").is_file():
            parser.error(f"Roxide Rscript is missing from {rhome / 'bin'}.")
        if not (rhome / "include" / "Rinternals.h").is_file() or not (rhome / "lib" / "libR.dylib").is_file():
            parser.error(f"Roxide headers or libR.dylib are missing from {rhome}.")

    with tempfile.TemporaryDirectory(prefix="roxide-capi-") as temporary:
        root = Path(temporary)
        if "oracle" in scripts:
            try:
                version = run([scripts["oracle"], "--vanilla", "-e",
                               'cat(R.version.string, "\\n", sep = "")'],
                              root, env, args.timeout)
            except (RuntimeError, OSError) as error:
                print(f"FAIL oracle: {error}", file=sys.stderr)
                return 1
            if version.status or not re.match(rb"^R version 4\.6\.2(?: |\n)", version.stdout):
                show_output(version)
                print("FAIL oracle: Expected GNU R 4.6.2.", file=sys.stderr)
                return 1
            print(version.stdout.decode("utf-8").strip(), flush=True)

        passed = 0
        for driver in drivers:
            name = driver.stem
            results = {}
            success = True
            for engine, script in scripts.items():
                if name == NEGATIVE and engine == "oracle":
                    continue
                scratch = root / name / engine
                scratch.mkdir(parents=True)
                try:
                    paths = build_fixture(name, scratch, None if engine == "oracle" else rhome,
                                          env, args.timeout)
                except (RuntimeError, OSError) as error:
                    print(f"FAIL {name} ({engine}): {error}", file=sys.stderr)
                    success = False
                    continue

                runtime = {**env, **paths}
                runs = {engine: runtime}
                if engine == "roxide":
                    runtime["R_HOME"] = str(rhome)
                    if args.torture:
                        runs["roxide torture"] = {**runtime, "ROXIDE_GC_TORTURE": "1"}
                for label, run_env in runs.items():
                    result = None
                    try:
                        result = run([script, "--vanilla", str(driver)], scratch,
                                     run_env, args.timeout)
                        results[label] = result
                        expect_result(name, result)
                    except (RuntimeError, OSError) as error:
                        print(f"FAIL {name} ({label}): {error}", file=sys.stderr)
                        if result is not None:
                            show_output(result)
                        success = False
            if "oracle" in results and "roxide" in results:
                success &= compare(name, results["oracle"], results["roxide"], "oracle", "roxide")
            if "roxide" in results and "roxide torture" in results:
                success &= compare(name, results["roxide"], results["roxide torture"],
                                   "roxide", "roxide torture")
            if success:
                print(f"PASS {name} ({', '.join(results)})", flush=True)
                passed += 1

    print(f"{passed}/{len(drivers)} fixtures passed.")
    return int(passed != len(drivers))


if __name__ == "__main__":
    raise SystemExit(main())
