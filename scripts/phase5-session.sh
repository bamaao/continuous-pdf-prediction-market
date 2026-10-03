#!/usr/bin/env bash
# Phase 5 Done: two Session buys; gateway pending+durable receipt; withdraw is main-wallet only.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

URL="${URL:-http://127.0.0.1:8899}"
GW="${GW:-http://127.0.0.1:8081}"
# WSL rustc ≠ Windows rustc; keep host binaries and the ledger off /mnt/e.
if [[ "$(uname -s)" == "Linux" ]]; then
  export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/cpm-target}"
fi
LEDGER="${LEDGER:-${CARGO_TARGET_DIR:-$ROOT/target}/phase5-ledger}"
DEPLOY="${DEPLOY:-${CARGO_TARGET_DIR:-$ROOT/target}/deploy}"
CPM="${CPM:-${CARGO_TARGET_DIR:-$ROOT/target}/debug/cpm}"
GW_BIN="${GW_BIN:-${CARGO_TARGET_DIR:-$ROOT/target}/debug/trading-gateway}"
KEYPAIR="${KEYPAIR:-$HOME/.config/solana/id.json}"
SESS="${SESS:-/tmp/cpm-session.json}"
RECEIPT_DIR="${RECEIPT_DIR:-/tmp/cpm-receipts}"
AUTH="$ROOT/fixtures/usdc-mint-authority.json"
MINT_ACC="$ROOT/fixtures/usdc-mint.json"

VAULT="VaULt11111111111111111111111111111111111111"
MARKET="Market1111111111111111111111111111111111111"
RES="Rso1111111111111111111111111111111111111111"
RISK="Rsk1111111111111111111111111111111111111111"
# MagicBlock / GPL SessionTokenV2 (CR-04)
SESSION_KEYS="KeyspM2ssCJbqUhQ4k7sveSiY4WjnYsrXkC8oDbwde5"
USDC="EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"

cpm() { "$CPM" --url "$URL" --keypair "$KEYPAIR" "$@"; }

need_so() {
  local name="$1"
  local force="${2:-0}"
  if [[ "$force" != "1" && -f "$DEPLOY/$name.so" ]]; then
    echo "reuse $name.so"
    return
  fi
  echo "building $name.so"
  cargo-build-sbf --manifest-path "programs/$name/Cargo.toml"
}

need_session_keys_so() {
  mkdir -p "$DEPLOY"
  if [[ -f "$DEPLOY/session_keys.so" ]]; then
    echo "reuse session_keys.so"
    return
  fi
  if [[ -f /tmp/cpm-target/deploy/session_keys.so ]]; then
    cp /tmp/cpm-target/deploy/session_keys.so "$DEPLOY/session_keys.so"
    echo "reuse /tmp/cpm-target/deploy/session_keys.so"
    return
  fi
  echo "building session-keys 3.1.1 (CR-04 SessionTokenV2)"
  local tmp
  tmp="$(mktemp -d)"
  (
    cd "$tmp"
    cargo new --lib session_keys_build >/dev/null
    cd session_keys_build
    # Pull the crate as a path-less dependency with cdylib so cargo-build-sbf emits .so
    cat > Cargo.toml <<'EOF'
[package]
name = "session_keys_build"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib", "lib"]
name = "session_keys"

[dependencies]
session-keys = { version = "3.1.1", default-features = false }
EOF
    cargo-build-sbf
  )
  local built
  built="$(find "$tmp" -name 'session_keys.so' | head -n1)"
  if [[ -z "$built" ]]; then
    echo "failed to build session_keys.so"
    exit 1
  fi
  cp "$built" "$DEPLOY/session_keys.so"
  rm -rf "$tmp"
}

echo "== build CLI + gateway + programs =="
cargo build -p cli -p gateway
"$CPM" write-local-mint --authority "$AUTH" --account "$MINT_ACC"
need_so vault
need_so market
need_so resolution
need_so risk
need_session_keys_so

if ! [[ -f "$KEYPAIR" ]]; then
  mkdir -p "$(dirname "$KEYPAIR")"
  solana-keygen new --no-bip39-passphrase --silent -o "$KEYPAIR"
fi
echo "== validator =="
pkill -f solana-test-validator >/dev/null 2>&1 || true
pkill -f '/target/debug/trading-gateway' >/dev/null 2>&1 || true
rm -rf "$LEDGER"
mkdir -p "$LEDGER"
rm -f "$SESS"
rm -rf "$RECEIPT_DIR"
mkdir -p "$RECEIPT_DIR"
solana-keygen new --no-bip39-passphrase --silent -o "$SESS"
solana-test-validator \
  --reset \
  --ledger "$LEDGER" \
  --rpc-port 8899 \
  --bpf-program "$VAULT" "$DEPLOY/vault.so" \
  --bpf-program "$MARKET" "$DEPLOY/market.so" \
  --bpf-program "$RES" "$DEPLOY/resolution.so" \
  --bpf-program "$RISK" "$DEPLOY/risk.so" \
  --bpf-program "$SESSION_KEYS" "$DEPLOY/session_keys.so" \
  --account "$USDC" "$MINT_ACC" \
  >/tmp/cpm-validator.log 2>&1 &
VAL_PID=$!
GW_PID=""
cleanup() {
  kill "$GW_PID" >/dev/null 2>&1 || true
  kill "$VAL_PID" >/dev/null 2>&1 || true
}
trap cleanup EXIT

for i in $(seq 1 40); do
  if cpm index-status >/dev/null 2>&1; then break; fi
  sleep 1
  if [[ "$i" -eq 40 ]]; then
    tail -n 80 /tmp/cpm-validator.log
    exit 1
  fi
done

solana airdrop 100 --url "$URL" --keypair "$KEYPAIR" >/dev/null
solana airdrop 10 --url "$URL" --keypair "$SESS" >/dev/null

start_gw() {
  RPC_URL="$URL" LISTEN="127.0.0.1:8081" RECEIPT_DIR="$RECEIPT_DIR" "$GW_BIN" >/tmp/cpm-gateway.log 2>&1 &
  GW_PID=$!
}
wait_gw() {
  local i
  for i in $(seq 1 30); do
    if curl -fsS "$GW/v1/health" | grep -q 'holds_keys.:false'; then
      return 0
    fi
    sleep 1
  done
  echo "gateway never became healthy"
  tail -n 40 /tmp/cpm-gateway.log || true
  return 1
}
start_gw

echo "== vault / market =="
cpm faucet 100000
cpm vault-init
cpm deposit 50000
cpm committee init --m 1 || true
CREATE_OUT="$(cpm market create-gaussian phase5-session first --n 8 --close-in 600 --challenge-secs 20)"
echo "$CREATE_OUT"
MARKET_PK="$(echo "$CREATE_OUT" | sed -n 's/.*market=\([^ ]*\).*/\1/p')"
cpm settle fund-cm "$MARKET_PK"

echo "== session + SessionTokenV2 + gateway =="
OPEN_OUT="$(cpm session open --authority "$SESS" --hours 2 --usdc 20000 --market "$MARKET_PK")"
echo "$OPEN_OUT"
echo "$OPEN_OUT" | grep -q 'token='
wait_gw || exit 1

OWNER="$(solana-keygen pubkey "$KEYPAIR")"
sec_code="$(curl -s -o /tmp/gw-secret.out -w '%{http_code}' -X POST "$GW/v1/submit" \
  -H 'content-type: application/json' \
  -d '{"tx_b64":"AA==","owner":"x","market":"y","nonce":9,"private_key":"nope"}')"
if [[ "$sec_code" != "400" ]]; then
  echo "gateway accepted a secret payload: HTTP $sec_code"
  cat /tmp/gw-secret.out
  exit 1
fi

echo "== two session buys via gateway (pending then confirm) =="
cpm trade buy-set "$MARKET_PK" 01 20 --session "$SESS" --nonce 1 --gateway "$GW"
curl -fsS "$GW/v1/receipt?owner=$OWNER&market=$MARKET_PK&nonce=1" | tee /tmp/cpm-receipt-1.json
grep -q '"status":"confirmed"' /tmp/cpm-receipt-1.json
if grep -qiE 'private|secret|mnemonic|keypair' /tmp/cpm-receipt-1.json; then
  echo "receipt leaked secrets"
  exit 1
fi
echo "== gateway restart must keep the receipt =="
kill "$GW_PID" >/dev/null 2>&1 || true
sleep 1
start_gw
wait_gw || exit 1
curl -fsS "$GW/v1/receipt?owner=$OWNER&market=$MARKET_PK&nonce=1" | tee /tmp/cpm-receipt-1b.json
grep -q '"status":"confirmed"' /tmp/cpm-receipt-1b.json
cpm trade buy-set "$MARKET_PK" 01 20 --session "$SESS" --nonce 2 --gateway "$GW"
echo "== gateway same-nonce retry is idempotent =="
cpm trade buy-set "$MARKET_PK" 01 20 --session "$SESS" --nonce 2 --gateway "$GW"

echo "== withdraw must reject the session key =="
if "$CPM" --url "$URL" --keypair "$SESS" withdraw 1 >/tmp/cpm-sess-withdraw.log 2>&1; then
  echo "session withdraw unexpectedly succeeded"
  cat /tmp/cpm-sess-withdraw.log
  exit 1
fi
echo "session withdraw rejected (expected)"
cpm withdraw 1
echo "PHASE5_OK market=$MARKET_PK two session buys, pending receipt survived restart, withdraw is main-wallet only"
