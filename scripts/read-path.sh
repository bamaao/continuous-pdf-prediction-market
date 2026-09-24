#!/usr/bin/env bash
# Phase 4: HTTP quote must match crates/math on the last indexed θ.
set -euo pipefail
API="${API:-http://127.0.0.1:8080}"
MASK="${MASK:-01}"
SHARES="${SHARES:-1}"

health="$(curl -fsS "$API/v1/health")"
echo "health $health"

markets="$(curl -fsS "$API/v1/markets")"
echo "markets $markets"
market="$(echo "$markets" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d[0]["market"] if d else "")')"
if [[ -z "$market" ]]; then
  echo "no projected markets (start validator + cpm loop, then market-api with EMBED_INDEXER=1)"
  exit 1
fi

quote="$(curl -fsS "$API/v1/markets/$market/quote?mask=$MASK&shares=$SHARES")"
pdf="$(curl -fsS "$API/v1/markets/$market/pdf")"
echo "quote $quote"
echo "pdf_cells $(echo "$pdf" | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["cells"]))')"
echo "READPATH_OK market=$market"
