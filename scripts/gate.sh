#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [ -f "$ROOT/env/env.sh" ]; then
  # shellcheck disable=SC1091
  source "$ROOT/env/env.sh"
fi

echo "==> [1/6] Clean-room licence scan"
python3 tools/measure/licence_scan.py

echo "==> [2/6] Cargo format check"
cargo fmt --all -- --check

echo "==> [3/6] Cargo clippy (all targets, -D warnings)"
cargo clippy --workspace --all-targets -- -D warnings

echo "==> [4/6] Rustdoc check (-D warnings)"
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

echo "==> [5/6] Cargo test (workspace)"
cargo test --workspace

echo "==> [6/6] Differential oracle smoke check"
if [ -n "${ROXIDE_ORACLE_RSCRIPT:-}" ] && [ -x "${ROXIDE_ORACLE_RSCRIPT}" ]; then
  cargo build -p roxide-cli
  cargo run -p roxide-harness -- --suite smoke
else
  echo "ERROR: ROXIDE_ORACLE_RSCRIPT not configured or executable." >&2
  exit 1
fi

echo "==> All Roxide gates PASSED."
