# Prior Art & Dependency Licence Log

Every external Rust crate, C library, published algorithm, or specification adopted in Roxide is logged here **before** being added to `Cargo.toml`, with verified licence compatibility (`MIT OR Apache-2.0`).

| Name | Version / Reference | Licence | Purpose | Clean-Room / ADR |
| --- | --- | --- | --- | --- |
| Rust Standard Library (`std`) | Rust 1.99.1+ (edition 2024) | `MIT OR Apache-2.0` | Core runtime & standard collections | Baseline |
| GNU R 4.6.2 macOS arm64 binary (oracle only) | `R-4.6.2-arm64.pkg` (CRAN `sonoma-arm64`) | `GPL-2.0-or-later` (external black-box binary only; gitignored in `env/oracle/`, **never** linked or distributed) | Black-box differential oracle for `roxide-harness` | ADR-0001 |

## Forbidden Sources (Strict Clean-Room Wall)

The following must **never** be read, copied, translated, or vendored into Roxide:
- GNU R source code (`src/main/*.c`, `src/nmath/*.c`, `src/library/*/R/*.R`, `src/library/*/src/*`, `src/include/*` internals)
- GNU R test suite (`tests/*`)
- Any GPL/AGPL/LGPL-only crate or CRAN package source code
