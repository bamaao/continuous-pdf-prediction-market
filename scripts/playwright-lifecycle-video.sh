#!/usr/bin/env bash
# Record a Playwright WebM of ONE distribution family's main lifecycle
# (create → review → session → trade → auction → resolve → settle → LP/platform).
#
# Usage (stack must already be up: web :3000, market-api :8080, gateway, test RPC):
#   bash scripts/playwright-lifecycle-video.sh
#   bash scripts/playwright-lifecycle-video.sh Gaussian
#   LIFECYCLE_SLOW_MO_MS=80 bash scripts/playwright-lifecycle-video.sh Skellam
#
# Output: tmp/playwright-lifecycle/video/lifecycle-<Family>-*.webm
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FAMILY="${1:-Gaussian}"

export LIFECYCLE_FAMILIES="$FAMILY"
export LIFECYCLE_BRANCHES="${LIFECYCLE_BRANCHES:-0}"
export LIFECYCLE_VIDEO=1
export LIFECYCLE_DEMO_DATA="${LIFECYCLE_DEMO_DATA:-1}"
export LIFECYCLE_DEMO_DATA_PATH="${LIFECYCLE_DEMO_DATA_PATH:-$ROOT/fixtures/lifecycle-demo.json}"
export LIFECYCLE_VIDEO_DIR="${LIFECYCLE_VIDEO_DIR:-$ROOT/tmp/playwright-lifecycle/video}"
# Human-like pacing (override to taste).
export LIFECYCLE_SLOW_MO_MS="${LIFECYCLE_SLOW_MO_MS:-280}"
export LIFECYCLE_VIDEO_ACTION_PAUSE_MS="${LIFECYCLE_VIDEO_ACTION_PAUSE_MS:-1800}"
export LIFECYCLE_VIDEO_STEP_PAUSE_MS="${LIFECYCLE_VIDEO_STEP_PAUSE_MS:-4000}"
export LIFECYCLE_HEADED="${LIFECYCLE_HEADED:-0}"
export LIFECYCLE_CLOSE_IN="${LIFECYCLE_CLOSE_IN:-100}"
export WEB_URL="${WEB_URL:-http://127.0.0.1:3000}"
export MARKET_API="${MARKET_API:-http://127.0.0.1:8080}"
export GATEWAY="${GATEWAY:-http://127.0.0.1:8081}"
export RPC_URL="${RPC_URL:-http://127.0.0.1:8899}"

echo "== lifecycle video (human pace): family=$FAMILY slow_mo=${LIFECYCLE_SLOW_MO_MS}ms action=${LIFECYCLE_VIDEO_ACTION_PAUSE_MS}ms step=${LIFECYCLE_VIDEO_STEP_PAUSE_MS}ms =="
echo "   video dir: $LIFECYCLE_VIDEO_DIR"

python3 "$ROOT/scripts/playwright-lifecycle-report.py"
