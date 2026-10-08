#!/usr/bin/env bash
set -u
export PATH="${HOME}/.local/node_modules/.bin:${PATH}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
tmux kill-session -t cpm-er 2>/dev/null || true
tmux new-session -d -s cpm-er "cd '$ROOT' && ER_RESET=1 bash scripts/start-ephemeral-validator.sh 2>&1 | tee /tmp/cpm-er.log"
for i in $(seq 1 40); do
  if ss -lntp 2>/dev/null | grep -q ':7799'; then
    echo "ER up (${i}s)"
    exit 0
  fi
  sleep 1
done
echo "ER not listening"
tail -n 20 /tmp/cpm-er.log || true
exit 1
