#!/usr/bin/env bash
# Phase 4 Done: HTTP quote equals `cpm market quote` on the last on-chain θ.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

LEDGER="${LEDGER:-$ROOT/test-ledger}"
URL="${URL:-http://127.0.0.1:8899}"
API="${API:-http://127.0.0.1:8080}"
DEPLOY="$ROOT/target/deploy"
CPM="${CPM:-$ROOT/target/debug/cpm}"
API_BIN="${API_BIN:-$ROOT/target/debug/market-api}"
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

echo "== build CLI + market-api =="
cargo build -p cli -p readpath --bins
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
pkill -f '/target/debug/market-api' >/dev/null 2>&1 || true
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
cleanup() {
  kill "$API_PID" >/dev/null 2>&1 || true
  kill "$VAL_PID" >/dev/null 2>&1 || true
}
trap cleanup EXIT

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

echo "== faucet / vault / deposit / create / buy =="
cpm faucet 100000
cpm vault-init
cpm deposit 50000
CREATE_OUT="$(cpm market create-gaussian phase4-read first-print --n 8 --c-m 5 --close-in 600 --challenge-secs 20)"
echo "$CREATE_OUT"
MARKET_PK="$(echo "$CREATE_OUT" | sed -n 's/.*market=\([^ ]*\).*/\1/p')"
if [[ -z "$MARKET_PK" ]]; then
  echo "failed to parse create output"
  exit 1
fi
cpm settle fund-cm "$MARKET_PK" 5
cpm trade buy-set "$MARKET_PK" 01 60

echo "== market-api =="
RPC_URL="$URL" LISTEN="127.0.0.1:8080" EMBED_INDEXER=1 "$API_BIN" >/tmp/cpm-market-api.log 2>&1 &
API_PID=$!

quote=""
for i in $(seq 1 30); do
  if quote="$(curl -fsS "$API/v1/markets/$MARKET_PK/quote?mask=01&shares=1" 2>/dev/null)"; then
    break
  fi
  sleep 1
  if [[ "$i" -eq 30 ]]; then
    echo "market-api never indexed $MARKET_PK"
    tail -n 80 /tmp/cpm-market-api.log
    exit 1
  fi
done

CLI_OUT="$(cpm market quote "$MARKET_PK" --mask 01 --shares 1)"
echo "cli  $CLI_OUT"
echo "http $quote"

field() { echo "$CLI_OUT" | sed -n "s/.*$1=\([^ ]*\).*/\1/p"; }
http_field() { echo "$quote" | python3 -c "import json,sys; print(json.load(sys.stdin)['$1'])"; }

for pair in p_s_raw:p_s_raw p_s_bps:p_s_bps c_s_raw:c_s_raw coverage_bps:coverage_bps rho_hat_bps:rho_hat_bps l_max_usdc:l_max_usdc c_max_usdc:c_max_usdc r_net:r_net; do
  cli_k="${pair%%:*}"
  http_k="${pair##*:}"
  a="$(field "$cli_k")"
  b="$(http_field "$http_k")"
  if [[ "$a" != "$b" ]]; then
    echo "mismatch $cli_k cli=$a http=$b"
    exit 1
  fi
done

PDF="$(curl -fsS "$API/v1/markets/$MARKET_PK/pdf")"
HTTP_P0="$(echo "$PDF" | python3 -c 'import json,sys; print(json.load(sys.stdin)["cells"][0]["p_bps"])')"
CLI_P0="$(cpm market pdf "$MARKET_PK" | sed -n 's/^cell=0 p_bps=\([^ ]*\).*/\1/p')"
if [[ "$HTTP_P0" != "$CLI_P0" ]]; then
  echo "pdf mismatch cell0 http=$HTTP_P0 cli=$CLI_P0"
  exit 1
fi
if [[ "$HTTP_P0" == "$(field p_s_bps)" ]]; then
  :
else
  echo "mask=01 p_s_bps must equal cell0 p_bps"
  exit 1
fi

echo "PHASE4_OK market=$MARKET_PK p_s_bps=$(field p_s_bps) coverage_bps=$(field coverage_bps) rho_hat_bps=$(field rho_hat_bps)"
