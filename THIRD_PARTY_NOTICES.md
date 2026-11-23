# Third-Party Notices

Roxide is dual-licensed under `MIT OR Apache-2.0`.

This file tracks all third-party crates, libraries, datasets, and published algorithms adopted into Roxide, together with their verified permissive licenses. Copies of third-party license texts live under `third_party/licences/`.

## External Rust Crates (Milestone 1 Wave 3 — ADR-0008)

- **`regex`** (`MIT OR Apache-2.0`) — Rust Project Developers. Pure-Rust regular expression engine.
- **`fancy-regex`** (`MIT`) — Raph Levien & contributors. Pure-Rust regex engine with lookaround and backreferences.
- **`flate2`** (`MIT OR Apache-2.0`) & **`miniz_oxide`** (`MIT OR Zlib OR Apache-2.0`) — Pure-Rust DEFLATE/gzip compression and decompression.
- **`bzip2`** (`MIT OR Apache-2.0`) & **`libbz2-rs-sys`** (`bzip2-1.0.6`) — Pure-Rust bzip2 compression and decompression.
- **`lzma-rust2`** (`Apache-2.0`) — Pure-Rust XZ/LZMA2 compression and decompression (`#![forbid(unsafe_code)]` with default non-SIMD features).
- **`zstd`** (`MIT` / `BSD-3-Clause`), **`zstd-safe`** (`BSD-3-Clause`), **`zstd-sys`** (`BSD-3-Clause AND BSD-2-Clause`, bundled libzstd 1.5.7 selected under BSD-3-Clause).

## Reference Algorithms & Specifications

- **R Language Definition / R Internals / Writing R Extensions** — Clean-room behavioral specification reference (no GNU R source code or test suites consulted or copied).
- **IEEE 754-2019** — Binary floating-point arithmetic specification (quiet/signaling NaN payload semantics for `NA_real_`).
- **Howard Hinnant's Proleptic Gregorian Civil Calendar Algorithms** (Public Domain) — Clean civil date / epoch-day conversions in `roxide-base::datetime`.
