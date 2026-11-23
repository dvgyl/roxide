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

The runner reports missing optional packages and unsupported test APIs as failures when the upstream tests require them. For example, some withr tests require `callr` and `rlang`. A failing test, install error, or difference between engines gives the runner a nonzero exit status.
