#!/usr/bin/env bash
# Start localnet with current target/deploy/*.so plus MagicBlock dumps (mb-test-validator).
set -euo pipefail
export PATH="${HOME}/.local/node_modules/.bin:${HOME}/.local/share/solana/install/active_release/bin:/usr/bin:/bin:${HOME}/.cargo/bin"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
LEDGER="${LEDGER:-/tmp/cpm-phase6-ledger}"
rm -rf "$LEDGER"
mkdir -p "$LEDGER"
exec mb-test-validator \
  --reset \
  --ledger "$LEDGER" \
  --rpc-port 8899 \
  --bpf-program VaULt11111111111111111111111111111111111111 target/deploy/vault.so \
  --bpf-program Market1111111111111111111111111111111111111 target/deploy/market.so \
  --bpf-program Rso1111111111111111111111111111111111111111 target/deploy/resolution.so \
  --bpf-program Rsk1111111111111111111111111111111111111111 target/deploy/risk.so \
  --bpf-program ComtrB2KEaWgXsW1dhr1xYL4Ht4Bjj3gXnnL6KMdABq fixtures/magicblock_committor_program.so \
  --account EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v fixtures/usdc-mint.json
