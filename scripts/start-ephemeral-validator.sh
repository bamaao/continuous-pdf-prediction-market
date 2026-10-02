#!/usr/bin/env bash
# MagicBlock local ER. L1 must already be on :8899 / :8900.
set -euo pipefail
export PATH="${HOME}/.local/node_modules/.bin:/usr/bin:/bin:${HOME}/.cargo/bin"
BIN="$(command -v ephemeral-validator)"
LEDGER="${ER_LEDGER:-/tmp/cpm-er-ledger}"
# ER_RESET=0 keeps the ledger (FR-DUR-01 restart). Default still resets.
RESET_ARGS=()
if [[ "${ER_RESET:-1}" != "0" ]]; then
  rm -rf "$LEDGER"
  RESET_ARGS+=(--reset)
fi
exec "$BIN" \
  --lifecycle ephemeral \
  --remotes http://127.0.0.1:8899 \
  --remotes ws://127.0.0.1:8900 \
  --listen 127.0.0.1:7799 \
  --storage "$LEDGER" \
  "${RESET_ARGS[@]}" \
  --no-tui
