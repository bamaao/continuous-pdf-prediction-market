#!/usr/bin/env bash
# Start localnet with current target/deploy/*.so. Does not pkill this script.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
LEDGER="${LEDGER:-/tmp/cpm-phase6-ledger}"
rm -rf "$LEDGER"
mkdir -p "$LEDGER"
exec solana-test-validator \
  --reset \
  --ledger "$LEDGER" \
  --rpc-port 8899 \
  --bpf-program VaULt11111111111111111111111111111111111111 target/deploy/vault.so \
  --bpf-program Market1111111111111111111111111111111111111 target/deploy/market.so \
  --bpf-program Rso1111111111111111111111111111111111111111 target/deploy/resolution.so \
  --bpf-program Rsk1111111111111111111111111111111111111111 target/deploy/risk.so \
  --account EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v fixtures/usdc-mint.json
