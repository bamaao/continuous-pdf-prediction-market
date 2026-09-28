#!/usr/bin/env bash
# Phase 6 localnet stack: validator + Market API + gateway + one Gaussian book.
# Leaves processes running. Writes tmp/phase6.env for Playwright.
set -euo pipefail

ROOT="${ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"
cd "$ROOT"

URL="${URL:-http://127.0.0.1:8899}"
API="${API:-http://127.0.0.1:8080}"
GW="${GW:-http://127.0.0.1:8081}"
LEDGER="${LEDGER:-/tmp/cpm-phase6-ledger}"
if [[ "$(uname -s)" == "Linux" ]]; then
  export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/cpm-target}"
fi
CPM="${CPM:-${CARGO_TARGET_DIR:-$ROOT/target}/debug/cpm}"
GW_BIN="${GW_BIN:-${CARGO_TARGET_DIR:-$ROOT/target}/debug/trading-gateway}"
API_BIN="${API_BIN:-${CARGO_TARGET_DIR:-$ROOT/target}/debug/market-api}"
KEYPAIR="${KEYPAIR:-$HOME/.config/solana/id.json}"
DEPLOY="$ROOT/target/deploy"
AUTH="$ROOT/fixtures/usdc-mint-authority.json"
MINT_ACC="$ROOT/fixtures/usdc-mint.json"
RECEIPT_DIR="${RECEIPT_DIR:-/tmp/cpm-receipts}"

VAULT="VaULt11111111111111111111111111111111111111"
MARKET="Market1111111111111111111111111111111111111"
RES="Rso1111111111111111111111111111111111111111"
RISK="Rsk1111111111111111111111111111111111111111"
USDC="EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"

need_so() {
  local name="$1"
  if [[ -f "$DEPLOY/$name.so" ]]; then
    echo "reuse $name.so"
    return
  fi
  echo "building $name.so"
  cargo-build-sbf --manifest-path "programs/$name/Cargo.toml"
}

echo "== build host bins + ensure programs =="
cargo build -p cli -p gateway -p readpath
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
pkill -f '[s]olana-test-validator' >/dev/null 2>&1 || true
pkill -f '[t]rading-gateway' >/dev/null 2>&1 || true
pkill -f '[m]arket-api' >/dev/null 2>&1 || true
rm -rf "$LEDGER"
mkdir -p "$LEDGER" "$RECEIPT_DIR"
setsid solana-test-validator \
  --reset \
  --ledger "$LEDGER" \
  --rpc-port 8899 \
  --bpf-program "$VAULT" "$DEPLOY/vault.so" \
  --bpf-program "$MARKET" "$DEPLOY/market.so" \
  --bpf-program "$RES" "$DEPLOY/resolution.so" \
  --bpf-program "$RISK" "$DEPLOY/risk.so" \
  --account "$USDC" "$MINT_ACC" \
  >/tmp/cpm-validator.log 2>&1 < /dev/null &
echo $! >/tmp/cpm-val.pid

cpm() { "$CPM" --url "$URL" --keypair "$KEYPAIR" "$@"; }

for i in $(seq 1 40); do
  if cpm index-status >/dev/null 2>&1; then break; fi
  sleep 1
  if [[ "$i" -eq 40 ]]; then
    tail -n 80 /tmp/cpm-validator.log
    exit 1
  fi
done

solana airdrop 100 --url "$URL" --keypair "$KEYPAIR" >/dev/null

echo "== vault / market =="
cpm faucet 100000
cpm vault-init
cpm deposit 50000
CREATE_OUT="$(cpm market create-gaussian phase6-web first --n 8 --c-m 5 --close-in 600 --challenge-secs 20)"
echo "$CREATE_OUT"
MARKET_PK="$(echo "$CREATE_OUT" | sed -n 's/.*market=\([^ ]*\).*/\1/p')"
cpm settle fund-cm "$MARKET_PK" 5

setsid env RPC_URL="$URL" LISTEN="127.0.0.1:8080" EMBED_INDEXER=1 "$API_BIN" >/tmp/cpm-market-api.log 2>&1 < /dev/null &
echo $! >/tmp/cpm-api.pid
setsid env RPC_URL="$URL" LISTEN="127.0.0.1:8081" RECEIPT_DIR="$RECEIPT_DIR" "$GW_BIN" >/tmp/cpm-gateway.log 2>&1 < /dev/null &
echo $! >/tmp/cpm-gw.pid

for i in $(seq 1 40); do
  if curl -fsS "$API/v1/health" >/dev/null && curl -fsS "$GW/v1/health" >/dev/null; then
    break
  fi
  sleep 1
  if [[ "$i" -eq 40 ]]; then
    echo "market-api or gateway never became healthy"
    tail -n 40 /tmp/cpm-market-api.log /tmp/cpm-gateway.log || true
    exit 1
  fi
done

for i in $(seq 1 40); do
  if curl -fsS "$API/v1/markets/$MARKET_PK/book" >/dev/null; then
    break
  fi
  sleep 1
  if [[ "$i" -eq 40 ]]; then
    echo "indexer never projected $MARKET_PK"
    tail -n 40 /tmp/cpm-market-api.log || true
    exit 1
  fi
done

mkdir -p "$ROOT/tmp"
{
  echo "PHASE6_MARKET=$MARKET_PK"
  echo "PHASE6_KEYPAIR=$KEYPAIR"
  echo "WEB_URL=${WEB:-http://127.0.0.1:3000}"
} > "$ROOT/tmp/phase6.env"
echo "PHASE6_STACK_OK market=$MARKET_PK"
echo "stack left running; pids in /tmp/cpm-val.pid /tmp/cpm-api.pid /tmp/cpm-gw.pid"
