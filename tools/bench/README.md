# Roxide benchmarks

Source `env/env.sh` before you run GNU R. In a linked Git worktree, use the
environment script from the checkout that has the oracle installed.

```sh
source env/env.sh
python3 tools/bench/run_bench.py --roxide-bin target/release/roxide \
  --json-out bench.json --md-out bench.md
```

Use an existing Roxide build with JIT support. If you have a staged R home,
use `--roxide-rhome` with the path to that directory.

```sh
python3 tools/bench/run_bench.py --engine oracle --filter '04_*' --runs 3
```

Each sample starts a fresh process and includes process startup in the wall
time. The runner uses the C locale, requests one thread from the numerical
libraries, and sets `ROXIDE_JIT=0` for AST or `ROXIDE_JIT=3` for JIT. GNU R
uses JIT level 3.

Each script uses seed 42. All selected engines and repetitions must return
the same checksum for a suite. The vector math checksum uses six decimal
places to allow for small floating-point differences between libraries.
The runner writes reports after all checks pass.

The JSON report includes wall time and peak resident set size (RSS) for each
timed run. RSS measures physical memory use. The Markdown table shows the
median wall time, the difference between the 75th and 25th percentiles, and
the highest peak RSS. A speedup of 2 means the JIT took half the baseline
median wall time.
