#!/usr/bin/env bash
# Wipe local historical state: L1/ER ledgers + restart validators in tmux.
set -u
export PATH="${HOME}/.local/share/solana/install/active_release/bin:${HOME}/.local/node_modules/.bin:${HOME}/.cargo/bin:${PATH}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "== stop old validators =="
pkill -f '[s]olana-test-validator' 2>/dev/null || true
pkill -f '[m]b-test-validator' 2>/dev/null || true
pkill -f '[e]phemeral-validator' 2>/dev/null || true
sleep 2

echo "== clear ledgers =="
rm -rf /tmp/cpm-phase6-ledger /tmp/cpm-er-ledger
mkdir -p /tmp/cpm-phase6-ledger /tmp/cpm-er-ledger

tmux kill-session -t cpm-l1 2>/dev/null || true
tmux kill-session -t cpm-er 2>/dev/null || true

echo "== start L1 =="
tmux new-session -d -s cpm-l1 "cd '$ROOT' && FORCE_VAULT_REBUILD=0 bash scripts/start-local-validator.sh 2>&1 | tee /tmp/cpm-l1.log"
for i in $(seq 1 90); do
  if curl -s http://127.0.0.1:8899 -H 'Content-Type: application/json' \
      -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' | grep -q ok; then
    echo "L1 up (${i}s)"
    break
  fi
  sleep 1
done
curl -s http://127.0.0.1:8899 -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"getSlot"}'
echo

echo "== start ER =="
tmux new-session -d -s cpm-er "cd '$ROOT' && ER_RESET=1 bash scripts/start-ephemeral-validator.sh 2>&1 | tee /tmp/cpm-er.log"
for i in $(seq 1 45); do
  if ss -lntp 2>/dev/null | grep -q ':7799'; then
    echo "ER up (${i}s)"
    break
  fi
  sleep 1
done
ss -lntp 2>/dev/null | grep -E '8899|7799' || true
tmux ls || true
echo "== done =="
