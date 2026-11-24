# Tier-2 pure R package tests

Source the local oracle configuration. Then fetch the pinned CRAN archives and run the GNU R 4.6.2 tests.

```sh
source env/env.sh
python3 tools/corpus/fetch_tier2.py
python3 tools/corpus/run_tier2.py --oracle-only
```

Use `--oracle-only --shim-only` to run the shim self-tests. To compare GNU R with Roxide, omit `--oracle-only`. The runner uses `target/debug/roxide` by default. Use `--roxide PATH` to select another binary. You can give package names to run a subset of the corpus.

The fetcher checks each archive against the SHA-256 value in [the manifest](manifest.json). It checks the licence in `DESCRIPTION` and rejects packages with a `src` directory or a compiled-code declaration. It puts archives and extracted packages under `env/corpus/tier2/`. Git ignores this directory. The runner prepares a separate temporary library for each engine. It compares the four summary counts and compares the shim self-test output byte for byte.

The MIT shim uses `all.equal()` for both testthat editions. It reports snapshot tests as skips. The shim implements the expectations in Section 5 of [the Milestone 3 specification](../../../docs/M3_W3_BASE_CRAN_SPEC.md). The implementation also uses base R operations needed for the specified behavior, such as `stop()`, `do.call()`, and caller and working-directory access. See the [public testthat reference](https://testthat.r-lib.org/reference/index.html).

The runner sets a fixed random seed before each package suite. The shim skips a test when R reports that a package listed in `Suggests` is missing. This also covers direct calls to optional packages such as `callr` and `rlang`. The shim reports errors for undeclared missing packages and unsupported test APIs. A failing test, install error, or difference between engines gives the runner a nonzero exit status.

## GNU R 4.6.2 results

These results are from macOS arm64 with the XQuartz libraries available to GNU R. These libraries let the withr Cairo graphics tests run. The runner uses an isolated library for the corpus. Withr skips snapshot tests and tests that need missing optional packages. Crayon explicitly skips one test.

| Suite | Pass | Fail | Skip | Warn |
| --- | ---: | ---: | ---: | ---: |
| Shim self-tests | 124 | 0 | 0 | 0 |
| R6 | 625 | 0 | 0 | 0 |
| withr | 333 | 0 | 18 | 0 |
| crayon | 532 | 0 | 1 | 0 |
| praise | 609 | 0 | 0 | 0 |
| magrittr | 52 | 0 | 0 | 0 |
| zeallot | 100 | 0 | 0 | 0 |
