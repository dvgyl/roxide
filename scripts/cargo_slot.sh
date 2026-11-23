#!/usr/bin/env bash
# Acquire one of 2 workspace-wide Cargo build/test slots (16 GB RAM macOS discipline)
# using Python fcntl.flock (macOS lacks flock(1)).
# Usage: scripts/cargo_slot.sh <command...>
set -euo pipefail

if [[ $# -eq 0 ]]; then
  echo "Usage: scripts/cargo_slot.sh <command...>" >&2
  exit 2
fi

export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
export RUST_TEST_THREADS="${RUST_TEST_THREADS:-3}"

exec python3 - "$@" <<'PY'
import fcntl
import os
import subprocess
import sys
import time

cmd = sys.argv[1:]
slots = [
    "/tmp/roxide-cargo-slot-0.lock",
    "/tmp/roxide-cargo-slot-1.lock",
]

acquired_fd = None
while acquired_fd is None:
    for path in slots:
        fd = os.open(path, os.O_CREAT | os.O_RDWR, 0o666)
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            acquired_fd = fd
            break
        except BlockingIOError:
            os.close(fd)
    if acquired_fd is None:
        time.sleep(0.5)

try:
    proc = subprocess.run(cmd)
    sys.exit(proc.returncode)
finally:
    try:
        fcntl.flock(acquired_fd, fcntl.LOCK_UN)
    finally:
        os.close(acquired_fd)
PY
