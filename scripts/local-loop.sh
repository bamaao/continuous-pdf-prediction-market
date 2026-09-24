#!/usr/bin/env bash
# Phase 3 acceptance (FR-CLI-01): deposit → create → buy → submit_result → settle
# using the `cpm` CLI only (no browser).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

LEDGER="${LEDGER:-$ROOT/test-ledger}"
URL="${URL:-http://127.0.0.1:8899}"
DEPLOY="$ROOT/target/deploy"
CPM="${CPM:-$ROOT/target/debug/cpm}"
KEYPAIR="${KEYPAIR:-$HOME/.config/solana/id.json}"
AUTH="$ROOT/fixtures/usdc-mint-authority.json"
MINT_ACC="$ROOT/fixtures/usdc-mint.json"

VAULT="VaULt11111111111111111111111111111111111111"
MARKET="Market1111111111111111111111111111111111111"
RES="Rso1111111111111111111111111111111111111111"
RISK="Rsk1111111111111111111111111111111111111111"
USDC="EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"

cpm() {
  "$CPM" --url "$URL" --keypair "$KEYPAIR" "$@"
}

need_so() {
  local name="$1"
  if [[ ! -f "$DEPLOY/$name.so" ]]; then
    echo "building $name.so"
    cargo-build-sbf --manifest-path "programs/$name/Cargo.toml"
  fi
}

echo "== build CLI =="
cargo build -p cli
"$CPM" write-local-mint --authority "$AUTH" --account "$MINT_ACC"

need_so vault
need_so market
need_so resolution
need_so risk

if ! [[ -f "$KEYPAIR" ]]; then
  mkdir -p "$(dirname "$KEYPAIR")"
  solana-keygen new --no-bip39-passphrase --silent -o "$KEYPAIR"
fi

echo "== validator =="
pkill -f solana-test-validator >/dev/null 2>&1 || true
rm -rf "$LEDGER"
solana-test-validator \
  --reset \
  --ledger "$LEDGER" \
  --rpc-port 8899 \
  --bpf-program "$VAULT" "$DEPLOY/vault.so" \
  --bpf-program "$MARKET" "$DEPLOY/market.so" \
  --bpf-program "$RES" "$DEPLOY/resolution.so" \
  --bpf-program "$RISK" "$DEPLOY/risk.so" \
  --account "$USDC" "$MINT_ACC" \
  >/tmp/cpm-validator.log 2>&1 &
VAL_PID=$!
trap 'kill "$VAL_PID" >/dev/null 2>&1 || true' EXIT

for i in $(seq 1 40); do
  if cpm index-status >/dev/null 2>&1; then
    break
  fi
  if ! kill -0 "$VAL_PID" >/dev/null 2>&1; then
    echo "validator exited; log:"
    tail -n 80 /tmp/cpm-validator.log
    exit 1
  fi
  sleep 1
  if [[ "$i" -eq 40 ]]; then
    echo "validator RPC never came up"
    tail -n 80 /tmp/cpm-validator.log
    exit 1
  fi
done

solana airdrop 100 --url "$URL" --keypair "$KEYPAIR" >/dev/null

echo "== faucet / vault / deposit =="
cpm faucet 100000
cpm vault-init
cpm deposit 50000

echo "== create gaussian / fund / buy =="
CREATE_OUT="$(cpm market create-gaussian cli-loop first-print --n 8 --c-m 5 --close-in 8 --challenge-secs 3)"
echo "$CREATE_OUT"
MARKET_PK="$(echo "$CREATE_OUT" | sed -n 's/.*market=\([^ ]*\).*/\1/p')"
CLOSE_TS="$(echo "$CREATE_OUT" | sed -n 's/.*close_ts=\([^ ]*\).*/\1/p')"
CHAL="$(echo "$CREATE_OUT" | sed -n 's/.*challenge_secs=\([^ ]*\).*/\1/p')"
if [[ -z "$MARKET_PK" || -z "$CLOSE_TS" ]]; then
  echo "failed to parse create output"
  exit 1
fi

cpm settle fund-cm "$MARKET_PK" 5
cpm trade buy-set "$MARKET_PK" 01 60
cpm resolve open "$MARKET_PK"

echo "== wait for close_ts=$CLOSE_TS =="
now="$(date +%s)"
if [[ "$CLOSE_TS" -gt "$now" ]]; then
  sleep $((CLOSE_TS - now + 1))
fi

echo "== resolve / settle / payout =="
cpm resolve submit "$MARKET_PK" 0
sleep $((CHAL + 1))
cpm resolve finalize "$MARKET_PK"
cpm settle begin "$MARKET_PK"
cpm settle payout "$MARKET_PK" 01

echo "CLI_LOOP_OK market=$MARKET_PK"
