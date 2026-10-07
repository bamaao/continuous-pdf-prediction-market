#!/usr/bin/env bash
# Start localnet with current target/deploy/*.so plus MagicBlock dumps (mb-test-validator).
set -euo pipefail
export PATH="${HOME}/.local/node_modules/.bin:${HOME}/.local/share/solana/install/active_release/bin:/usr/bin:/bin:${HOME}/.cargo/bin"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
LEDGER="${LEDGER:-/tmp/cpm-phase6-ledger}"
rm -rf "$LEDGER"
mkdir -p "$LEDGER"
VAULT_SO="target/deploy/vault.so"
need_vault=0
if [[ "${FORCE_VAULT_REBUILD:-1}" == "1" ]]; then
  need_vault=1
elif [[ ! -f "$VAULT_SO" ]]; then
  need_vault=1
elif [[ -n "$(find programs/vault -type f \( -name '*.rs' -o -name 'Cargo.toml' \) -newer "$VAULT_SO" 2>/dev/null | head -n 1)" ]]; then
  need_vault=1
fi
if [[ "$need_vault" == "1" ]]; then
  echo "== rebuild vault.so (CoverLp / LossPool account layout must match compose) =="
  cargo-build-sbf --manifest-path programs/vault/Cargo.toml
fi
SESSION_SO="target/deploy/session_keys.so"
SESSION_ARGS=()
if [[ -f "$SESSION_SO" ]]; then
  SESSION_ARGS=(--bpf-program KeyspM2ssCJbqUhQ4k7sveSiY4WjnYsrXkC8oDbwde5 "$SESSION_SO")
fi

exec mb-test-validator \
  --reset \
  --ledger "$LEDGER" \
  --rpc-port 8899 \
  --bpf-program VaULt11111111111111111111111111111111111111 target/deploy/vault.so \
  --bpf-program Market1111111111111111111111111111111111111 target/deploy/market.so \
  --bpf-program Rso1111111111111111111111111111111111111111 target/deploy/resolution.so \
  --bpf-program Rsk1111111111111111111111111111111111111111 target/deploy/risk.so \
  --bpf-program ComtrB2KEaWgXsW1dhr1xYL4Ht4Bjj3gXnnL6KMdABq fixtures/magicblock_committor_program.so \
  "${SESSION_ARGS[@]}" \
  --account EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v fixtures/usdc-mint.json
