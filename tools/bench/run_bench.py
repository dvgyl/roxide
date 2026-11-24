#!/usr/bin/env python3
"""Compare GNU R 4.6.2 with the Roxide AST and JIT engines."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from fnmatch import fnmatchcase
import json
import math
import os
from pathlib import Path
import platform
import re
import signal
from statistics import median, quantiles
import subprocess
import sys
import time


BENCH_DIR = Path(__file__).resolve().parent
DEFAULT_ROXIDE = BENCH_DIR.parents[1] / "target" / "release" / "roxide"
ENGINE_NAMES = ("oracle", "roxide-ast", "roxide-jit")
MIB = 1024**2

# RUSAGE_CHILDREN keeps the lifetime high-water mark. Start a new
# measurement process for each sample so it has only one child to measure.
LINUX_TIMER = """
import resource
import subprocess
import sys
import time

start = time.perf_counter()
status = subprocess.call(sys.argv[1:])
elapsed = time.perf_counter() - start
peak = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss * 1024
print(f"ROXIDE_BENCH_METRICS {elapsed} {peak}", file=sys.stderr)
sys.exit(status if status >= 0 else 128 - status)
"""


@dataclass(frozen=True)
class Engine:
    name: str
    executable: str
    env: dict[str, str]

    def command(self, *args: str) -> list[str]:
        return [self.executable, "--vanilla", *args]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    selection = parser.add_mutually_exclusive_group()
    selection.add_argument("--engines", help="Comma-separated engine names. Default: all.")
    selection.add_argument("--engine", choices=ENGINE_NAMES, help="Run one engine.")
    roxide = parser.add_mutually_exclusive_group()
    roxide.add_argument("--roxide-bin", type=Path, help=f"Default: {DEFAULT_ROXIDE}")
    roxide.add_argument("--roxide-rhome", type=Path, help="Use Rscript from this R home.")
    parser.add_argument("--warmup", type=int, default=1, help="Warmups per engine and suite.")
    parser.add_argument("--runs", type=int, default=5, help="Timed runs per engine and suite.")
    parser.add_argument("--timeout", type=float, default=60, help="Seconds per child process.")
    parser.add_argument("--filter", default="*", help="Glob for a suite name or file name.")
    parser.add_argument("--json-out", type=Path, help="Write the JSON report to this path.")
    parser.add_argument("--md-out", type=Path, help="Also write the Markdown table to this path.")
    args = parser.parse_args()
    if args.engine:
        args.engines = [args.engine]
    elif args.engines is None:
        args.engines = list(ENGINE_NAMES)
    else:
        args.engines = [name.strip() for name in args.engines.split(",")]
    if set(args.engines).difference(ENGINE_NAMES) or len(set(args.engines)) != len(args.engines):
        parser.error("--engines must list distinct names from " + ", ".join(ENGINE_NAMES))
    if args.warmup < 0 or args.runs < 1:
        parser.error("--warmup must be at least 0 and --runs must be at least 1.")
    if not math.isfinite(args.timeout) or args.timeout <= 0:
        parser.error("--timeout must be finite and greater than 0.")
    if args.json_out and args.md_out and args.json_out.resolve() == args.md_out.resolve():
        parser.error("--json-out and --md-out must use different paths.")
    if sys.platform not in {"darwin", "linux"}:
        parser.error("Peak RSS measurement requires macOS or Linux.")
    return args


def make_engine(name: str, args: argparse.Namespace) -> Engine:
    env = os.environ.copy()
    for key in ("R_HOME", "R_DEFAULT_PACKAGES", "R_ENABLE_JIT", "ROXIDE_JIT"):
        env.pop(key, None)
    env.update(
        LC_ALL="C",
        OMP_NUM_THREADS="1",
        OPENBLAS_NUM_THREADS="1",
        VECLIB_MAXIMUM_THREADS="1",
        R_MAX_VSIZE="512M",
    )
    if name == "oracle":
        executable = env.get("ROXIDE_ORACLE_RSCRIPT")
        if not executable:
            raise RuntimeError("Source env/env.sh to set ROXIDE_ORACLE_RSCRIPT.")
        env["R_ENABLE_JIT"] = "3"
        return Engine(name, executable, env)

    if args.roxide_rhome:
        rhome = args.roxide_rhome.absolute()
        env["R_HOME"] = str(rhome)
        binary = rhome / "bin" / "Rscript"
    else:
        binary = (args.roxide_bin or DEFAULT_ROXIDE).absolute()
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise RuntimeError(f"Roxide executable is missing or cannot run: {binary}")
    env["ROXIDE_JIT"] = "0" if name == "roxide-ast" else "3"
    return Engine(name, str(binary), env)


def run_child(command: list[str], env: dict[str, str], timeout: float) -> tuple[str, str]:
    with subprocess.Popen(
        command, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, encoding="utf-8", errors="replace", start_new_session=True,
    ) as child:
        try:
            stdout, stderr = child.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            child.communicate()
            raise
    if child.returncode:
        raise RuntimeError(f"Exit status {child.returncode}.\n{stdout}{stderr}".rstrip())
    return stdout, stderr


def run_sample(
    engine: Engine, suite: Path, timeout: float,
) -> tuple[str, float, int]:
    command = engine.command(str(suite))
    if sys.platform == "darwin":
        start = time.perf_counter()
        stdout, stderr = run_child(["/usr/bin/time", "-l", *command], engine.env, timeout)
        elapsed = time.perf_counter() - start
        peaks = re.findall(r"^\s*(\d+)\s+maximum resident set size\s*$", stderr, re.M)
    else:
        stdout, stderr = run_child(
            [sys.executable, "-c", LINUX_TIMER, *command], engine.env, timeout,
        )
        metrics = re.search(r"^ROXIDE_BENCH_METRICS (\S+) (\d+)$", stderr, re.M)
        if metrics is None:
            raise RuntimeError("Missing Linux measurement.")
        elapsed_text, peak = metrics.groups()
        elapsed = float(elapsed_text)
        peaks = [peak]
    if len(peaks) != 1 or int(peaks[0]) <= 0:
        raise RuntimeError(f"Expected one positive peak RSS measurement.\n{stderr}")

    results = [line.removeprefix("RESULT:") for line in stdout.splitlines()
               if line.startswith("RESULT:")]
    if len(results) != 1 or not results[0].strip():
        raise RuntimeError(f"Expected one nonempty RESULT line.\n{stdout}{stderr}")
    return results[0], elapsed, int(peaks[0])


def benchmark(suite: Path, engines: list[Engine], args: argparse.Namespace) -> dict:
    samples = {engine.name: [] for engine in engines}
    expected = None
    reference = ""
    for index in range(args.warmup + args.runs):
        warmup = index < args.warmup
        phase = f"warmup {index + 1}" if warmup else f"run {index - args.warmup + 1}"
        for engine in engines:
            label = f"{engine.name} {phase}"
            try:
                checksum, elapsed, peak = run_sample(engine, suite, args.timeout)
            except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
                raise RuntimeError(f"{suite.name}, {label}: {error}") from error
            if expected is None:
                expected, reference = checksum, label
            if checksum != expected:
                raise RuntimeError(
                    f"{suite.name}, {label}: Checksum mismatch.\n"
                    f"Expected {expected!r} from {reference}.\nGot {checksum!r}."
                )
            if not warmup:
                samples[engine.name].append({"wall_seconds": elapsed, "peak_rss_bytes": peak})

    results = {}
    for name, runs in samples.items():
        times = [run["wall_seconds"] for run in runs]
        q1, _, q3 = quantiles(times, method="inclusive") if len(times) > 1 else times * 3
        results[name] = {
            "runs": runs,
            "median_wall_seconds": median(times),
            "iqr_wall_seconds": q3 - q1,
            "max_peak_rss_bytes": max(run["peak_rss_bytes"] for run in runs),
        }
    speedups = {}
    if "roxide-jit" in results:
        jit_time = results["roxide-jit"]["median_wall_seconds"]
        for baseline in ("roxide-ast", "oracle"):
            if baseline in results:
                speedups[f"roxide-jit/{baseline}"] = (
                    results[baseline]["median_wall_seconds"] / jit_time
                )
    return {
        "suite": suite.stem,
        "checksum": expected,
        "engines": results,
        "speedups": speedups,
    }


def markdown(report: dict) -> str:
    names = list(report["engines"])
    comparisons = list(report["benchmarks"][0]["speedups"])
    columns = ["Suite", *names, *comparisons]
    text = [
        "## Benchmark results",
        "",
        f'{report["warmup"]} warmup(s) and {report["runs"]} timed run(s) per engine and suite.',
        "Engine columns show median wall time (ms), interquartile range (ms), "
        "and highest peak memory use (MiB).",
    ]
    if comparisons:
        text.append("Speedups divide baseline median wall time by JIT median wall time.")
    text.extend(["", "| " + " | ".join(columns) + " |",
                 "| --- | " + " | ".join(["---:"] * (len(columns) - 1)) + " |"])
    for result in report["benchmarks"]:
        cells = [result["suite"]]
        for name in names:
            stats = result["engines"][name]
            cells.append(
                f'{stats["median_wall_seconds"] * 1000:.2f} / '
                f'{stats["iqr_wall_seconds"] * 1000:.2f} / '
                f'{stats["max_peak_rss_bytes"] / MIB:.2f}'
            )
        cells.extend(f'{result["speedups"][key]:.2f}x' for key in comparisons)
        text.append("| " + " | ".join(cells) + " |")
    return "\n".join(text) + "\n"


def main() -> int:
    args = parse_args()
    suites = [
        path for path in sorted((BENCH_DIR / "suites").glob("*.R"))
        if fnmatchcase(path.name, args.filter) or fnmatchcase(path.stem, args.filter)
    ]
    if not suites:
        sys.exit(f"No benchmark suites match {args.filter!r}.")
    try:
        engines = [make_engine(name, args) for name in args.engines]
        report = {
            "platform": platform.platform(),
            "warmup": args.warmup,
            "runs": args.runs,
            "timeout_seconds": args.timeout,
            "engines": {engine.name: {"command": engine.command()} for engine in engines},
            "benchmarks": [],
        }
        for engine in engines:
            if engine.name == "oracle":
                version, _ = run_child(
                    engine.command("-e", "cat(as.character(getRversion()))"),
                    engine.env, args.timeout,
                )
                if version.strip() != "4.6.2":
                    raise RuntimeError(f"Expected GNU R 4.6.2. Got {version.strip()!r}.")
                report["engines"]["oracle"]["version"] = version.strip()
            else:
                report["engines"][engine.name]["roxide_jit"] = int(engine.env["ROXIDE_JIT"])
        for suite in suites:
            print(f"Running {suite.name}", file=sys.stderr, flush=True)
            report["benchmarks"].append(benchmark(suite, engines, args))
        table = markdown(report)
        if args.json_out:
            args.json_out.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        if args.md_out:
            args.md_out.write_text(table, encoding="utf-8")
        print(table, end="")
    except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
        print(f"Benchmark failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
