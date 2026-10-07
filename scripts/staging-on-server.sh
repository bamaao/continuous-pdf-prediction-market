#!/usr/bin/env bash
# Runs ON the Linux staging host (Ubuntu or CentOS/RHEL): build from source,
# optional program deploy, restart services. Invoked by deploy-staging.sh over SSH
# or manually: cd /opt/cpm && bash scripts/staging-on-server.sh
# See deploy/README.md for per-distro package install. Not for Windows.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

RUNTIME_ENV="${RUNTIME_ENV:-$ROOT/deploy/runtime.env}"
if [[ ! -f "$RUNTIME_ENV" ]]; then
  echo "missing $RUNTIME_ENV — copy deploy/runtime.env.example and fill RPC_URL / ER_RPC / secrets"
  exit 1
fi
# shellcheck disable=SC1090
set -a
source "$RUNTIME_ENV"
set +a

: "${RPC_URL:?RPC_URL required}"
: "${ER_RPC:?ER_RPC required (MagicBlock test ER)}"
: "${DATABASE_URL:?DATABASE_URL required}"
: "${PLATFORM_PUBKEY:?PLATFORM_PUBKEY required}"
: "${SIWS_SECRET:?SIWS_SECRET required for CPM_ENV=staging}"
: "${SOLANA_KEYPAIR:?SOLANA_KEYPAIR required}"

if [[ "${CPM_ENV:-}" != "staging" && "${CPM_ENV:-}" != "production" && "${CPM_ENV:-}" != "prod" ]]; then
  echo "CPM_ENV must be staging (or production); got '${CPM_ENV:-}'"
  exit 1
fi
if [[ "${ALLOW_MEMORY_ONLY:-0}" == "1" ]]; then
  echo "ALLOW_MEMORY_ONLY is forbidden when CPM_ENV=$CPM_ENV"
  exit 1
fi
if [[ ! -f "$SOLANA_KEYPAIR" ]]; then
  echo "SOLANA_KEYPAIR not found: $SOLANA_KEYPAIR"
  exit 1
fi

export PATH="${HOME}/.local/share/solana/install/active_release/bin:${HOME}/.cargo/bin:${PATH}"
DEPLOY_DIR="${DEPLOY_DIR:-$ROOT/target/deploy}"
VAR_DIR="${VAR_DIR:-$ROOT/var}"
RUN_DIR="${RUN_DIR:-$ROOT/var/run}"
LOG_DIR="${LOG_DIR:-$ROOT/var/log}"
mkdir -p "$VAR_DIR" "$RUN_DIR" "$LOG_DIR" \
  "${NOTIFY_DIR:-$VAR_DIR/notify}" \
  "${JOURNAL_REPLICA_DIR:-$VAR_DIR/journal-replica}" \
  "${JOURNAL_OBJECT_DIR:-$VAR_DIR/journal-object}" \
  "${RECEIPT_DIR:-$VAR_DIR/receipts}"

DO_BUILD="${STAGING_REMOTE_BUILD:-1}"
DO_RESTART="${STAGING_REMOTE_RESTART:-1}"
DO_PROGRAMS="${STAGING_DEPLOY_PROGRAMS:-0}"
DO_CHAIN_BOOTSTRAP="${STAGING_CHAIN_BOOTSTRAP:-0}"

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "missing command: $1"
    exit 1
  }
}

stop_pidfile() {
  local name="$1"
  local pf="$RUN_DIR/$name.pid"
  if [[ -f "$pf" ]]; then
    local pid
    pid="$(cat "$pf" || true)"
    if [[ -n "${pid:-}" ]] && kill -0 "$pid" >/dev/null 2>&1; then
      echo "stop $name pid=$pid"
      kill "$pid" >/dev/null 2>&1 || true
      sleep 1
      kill -9 "$pid" >/dev/null 2>&1 || true
    fi
    rm -f "$pf"
  fi
}

start_bg() {
  local name="$1"
  shift
  local log="$LOG_DIR/$name.log"
  stop_pidfile "$name"
  echo "start $name -> $log"
  nohup env "$@" >>"$log" 2>&1 &
  echo $! >"$RUN_DIR/$name.pid"
}

wait_http() {
  local url="$1"
  local label="$2"
  local i
  for i in $(seq 1 60); do
    if curl -fsS "$url" >/dev/null 2>&1; then
      echo "ok $label"
      return 0
    fi
    sleep 1
  done
  echo "timeout waiting for $label ($url)"
  tail -n 80 "$LOG_DIR/${label}.log" 2>/dev/null || true
  exit 1
}

if [[ "$DO_BUILD" == "1" ]]; then
  need_cmd cargo
  need_cmd cargo-build-sbf
  need_cmd npm
  need_cmd solana

  echo "== cargo release bins =="
  cargo build --release -p cli -p gateway -p readpath

  echo "== programs (sbf) =="
  local_vault_features=()
  if [[ "${VAULT_FEATURES_DEVNET:-1}" == "1" ]]; then
    local_vault_features=(--features devnet)
  fi
  cargo-build-sbf --manifest-path programs/vault/Cargo.toml "${local_vault_features[@]}"
  cargo-build-sbf --manifest-path programs/market/Cargo.toml
  cargo-build-sbf --manifest-path programs/resolution/Cargo.toml
  cargo-build-sbf --manifest-path programs/risk/Cargo.toml

  echo "== web =="
  (
    cd apps/web
    if [[ -f package-lock.json ]]; then
      npm ci
    else
      npm install
    fi
    NEXT_PUBLIC_RPC_URL="${PUBLIC_RPC_URL:-$RPC_URL}" \
      NEXT_PUBLIC_MARKET_API="${PUBLIC_MARKET_API}" \
      NEXT_PUBLIC_GATEWAY="${PUBLIC_GATEWAY}" \
      NEXT_PUBLIC_CHAIN_ID="${PUBLIC_CHAIN_ID:-devnet}" \
      CPM_ENV="${CPM_ENV}" \
      SIWS_SECRET="${SIWS_SECRET}" \
      npm run build
  )
fi

CPM="${CPM:-$ROOT/target/release/cpm}"
API_BIN="${API_BIN:-$ROOT/target/release/market-api}"
GW_BIN="${GW_BIN:-$ROOT/target/release/gateway}"
for bin in "$CPM" "$API_BIN" "$GW_BIN"; do
  [[ -x "$bin" ]] || {
    echo "missing binary: $bin (run with STAGING_REMOTE_BUILD=1)"
    exit 1
  }
done

export SOLANA_RPC_URL="$RPC_URL"
solana config set --url "$RPC_URL" --keypair "$SOLANA_KEYPAIR" >/dev/null

VAULT_ID="VaULt11111111111111111111111111111111111111"
MARKET_ID="Market1111111111111111111111111111111111111"
RES_ID="Rso1111111111111111111111111111111111111111"
RISK_ID="Rsk1111111111111111111111111111111111111111"

if [[ "$DO_PROGRAMS" == "1" ]]; then
  echo "== deploy programs to test L1 =="
  need_cmd solana
  for pair in "vault:$VAULT_ID" "market:$MARKET_ID" "resolution:$RES_ID" "risk:$RISK_ID"; do
    name="${pair%%:*}"
    pid="${pair##*:}"
    so="$DEPLOY_DIR/$name.so"
    [[ -f "$so" ]] || {
      echo "missing $so"
      exit 1
    }
    key="$ROOT/keys/${name}-keypair.json"
    if [[ -f "$key" ]]; then
      echo "deploy $name ($pid) with $key"
      solana program deploy "$so" \
        --program-id "$key" \
        --url "$RPC_URL" \
        --keypair "$SOLANA_KEYPAIR" \
        --max-len 2000000
    else
      echo "upgrade $name at $pid (no keys/${name}-keypair.json — upgrade-only)"
      solana program deploy "$so" \
        --program-id "$pid" \
        --url "$RPC_URL" \
        --keypair "$SOLANA_KEYPAIR"
    fi
  done
fi

if [[ "$DO_CHAIN_BOOTSTRAP" == "1" ]]; then
  echo "== chain bootstrap (vault / protocol / committee) =="
  "$CPM" --url "$RPC_URL" --keypair "$SOLANA_KEYPAIR" vault-init || true
  PLATFORM_PUBKEY="$PLATFORM_PUBKEY" \
    "$CPM" --url "$RPC_URL" --keypair "$SOLANA_KEYPAIR" protocol-init || true
  "$CPM" --url "$RPC_URL" --keypair "$SOLANA_KEYPAIR" committee init --m 1 || true
fi

if [[ "$DO_RESTART" == "1" ]]; then
  echo "== restart services =="
  stop_pidfile market-api
  stop_pidfile gateway
  stop_pidfile web

  start_bg market-api \
    CPM_ENV="$CPM_ENV" \
    RPC_URL="$RPC_URL" \
    ER_RPC="$ER_RPC" \
    DATABASE_URL="$DATABASE_URL" \
    LISTEN="${LISTEN_API:-127.0.0.1:8080}" \
    EMBED_INDEXER="${EMBED_INDEXER:-1}" \
    PLATFORM_PUBKEY="$PLATFORM_PUBKEY" \
    REVIEWER_PUBKEYS="${REVIEWER_PUBKEYS:-}" \
    OPERATOR_PUBKEYS="${OPERATOR_PUBKEYS:-}" \
    INDEXER_POLL_MS="${INDEXER_POLL_MS:-60000}" \
    INDEX_LAG_SHED_SLOTS="${INDEX_LAG_SHED_SLOTS:-128}" \
    NOTIFY_DIR="${NOTIFY_DIR}" \
    JOURNAL_REPLICA_DIR="${JOURNAL_REPLICA_DIR}" \
    JOURNAL_OBJECT_DIR="${JOURNAL_OBJECT_DIR}" \
    KEEPER_HEARTBEAT_PATH="${KEEPER_HEARTBEAT_PATH}" \
    "$API_BIN"

  start_bg gateway \
    CPM_ENV="$CPM_ENV" \
    RPC_URL="$RPC_URL" \
    ER_RPC="$ER_RPC" \
    LISTEN="${LISTEN_GW:-127.0.0.1:8081}" \
    RECEIPT_DIR="${RECEIPT_DIR}" \
    "$GW_BIN"

  start_bg web \
    CPM_ENV="$CPM_ENV" \
    SIWS_SECRET="$SIWS_SECRET" \
    NEXT_PUBLIC_RPC_URL="${PUBLIC_RPC_URL:-$RPC_URL}" \
    NEXT_PUBLIC_MARKET_API="${PUBLIC_MARKET_API}" \
    NEXT_PUBLIC_GATEWAY="${PUBLIC_GATEWAY}" \
    NEXT_PUBLIC_CHAIN_ID="${PUBLIC_CHAIN_ID:-devnet}" \
    PORT="${WEB_PORT:-3000}" \
    HOSTNAME="${WEB_HOSTNAME:-127.0.0.1}" \
    bash -lc "cd '$ROOT/apps/web' && exec npx next start -H '${WEB_HOSTNAME:-127.0.0.1}' -p '${WEB_PORT:-3000}'"

  api_host="${LISTEN_API:-127.0.0.1:8080}"
  gw_host="${LISTEN_GW:-127.0.0.1:8081}"
  wait_http "http://${api_host}/v1/health" market-api
  wait_http "http://${gw_host}/v1/health" gateway
  wait_http "http://${WEB_HOSTNAME:-127.0.0.1}:${WEB_PORT:-3000}" web

  echo "STAGING_OK"
  echo "  L1 RPC     $RPC_URL"
  echo "  MagicBlock $ER_RPC"
  echo "  API        http://${api_host}"
  echo "  Gateway    http://${gw_host}"
  echo "  Web        http://${WEB_HOSTNAME:-127.0.0.1}:${WEB_PORT:-3000}"
  echo "  Public API ${PUBLIC_MARKET_API}"
fi
