#!/usr/bin/env bash
# Populate a minimal R_HOME layout for Roxide (bin/R, bin/Rscript, library/<builtin>/DESCRIPTION).
# Usage: scripts/make_r_home.sh [ROXIDE_BIN] [R_HOME_DIR]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ROXIDE_BIN="${1:-$ROOT/target/debug/roxide}"
R_HOME_DIR="${2:-$ROOT/target/r-home}"

mkdir -p "$R_HOME_DIR/bin" "$R_HOME_DIR/library"
ln -sf "$ROXIDE_BIN" "$R_HOME_DIR/bin/R"
ln -sf "$ROXIDE_BIN" "$R_HOME_DIR/bin/Rscript"

BUILTINS=(base stats utils tools methods grDevices graphics grid datasets)
for pkg in "${BUILTINS[@]}"; do
  mkdir -p "$R_HOME_DIR/library/$pkg"
  cat > "$R_HOME_DIR/library/$pkg/DESCRIPTION" <<EOF
Package: $pkg
Version: 4.6.2
Priority: base
Title: Roxide Built-in $pkg Package
Description: Clean-room Roxide implementation of the R 4.6.2 $pkg package.
License: MIT + file LICENSE
Built: R 4.6.2; ; 2026-10-29 00:00:00 UTC; unix
EOF
done

echo "Populated R_HOME at $R_HOME_DIR"
