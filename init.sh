#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Install commit-msg hook rejecting AI attribution trailers
HOOK_PATH="$ROOT/.git/hooks/commit-msg"
cat > "$HOOK_PATH" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
MSG_FILE="$1"
if grep -iE '^(Co-Authored-By:|Claude-Session:|Generated with )' "$MSG_FILE" >/dev/null 2>&1; then
  echo "ERROR: Commit message contains forbidden AI trailer (Co-Authored-By / Claude-Session / Generated with)." >&2
  exit 1
fi
EOF
chmod +x "$HOOK_PATH"
echo "Installed commit-msg hook at $HOOK_PATH"

if [ -f "$ROOT/env/env.sh" ]; then
  # shellcheck disable=SC1091
  source "$ROOT/env/env.sh"
  echo "Loaded env/env.sh:"
  echo "  rustc: $(rustc --version)"
  echo "  cargo: $(cargo --version)"
  if [ -n "${ROXIDE_ORACLE_RSCRIPT:-}" ] && [ -x "${ROXIDE_ORACLE_RSCRIPT}" ]; then
    echo "  oracle: $("$ROXIDE_ORACLE_RSCRIPT" --vanilla -e 'cat(R.version.string, "\n")')"
  else
    echo "  oracle: NOT FOUND (expected at ${ROXIDE_ORACLE_RSCRIPT:-unset})" >&2
    exit 1
  fi
else
  echo "WARNING: env/env.sh not found yet." >&2
fi
