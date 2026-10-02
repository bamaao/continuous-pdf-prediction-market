# Phase 8 — Keeper, notify, TWA

> **For Claude:** Local-first. CLI is the only keeper write path (FR-UI-28). No MagicBlock ER. Do not restart `:3000` unless a web surface must change.

**Goal:** `cpm keeper --once` is idempotent: Commit journal `trades_root`, halt/undelegate after `close_ts`, write a heartbeat `/ops` can read; notifications carry `market_id` only.

**Architecture:** Keeper is the CLI/KMS (R-KEEP). `/ops` stays read-only. Notify is a file JSONL (`NOTIFY_DIR`) plus `GET /v1/notify`. Official TWA is a wrapper manifest of the same Next.js origin — no second client. `CPM_ENV=staging|production` refuses `ALLOW_MEMORY_ONLY`.

**Tech stack:** `crates/notify`, `crates/cli` keeper loop, Market API ops/notify, `apps/android-twa`.

---

## Done when

1. `cpm keeper --once` writes heartbeat; a second run does not double-Commit the same root.
2. `GET /v1/ops/status` shows that heartbeat (not the indexer slot masquerading as keeper).
3. Notify records are `{ts,kind,market}` only.
4. `apps/android-twa` has a TWA manifest and no business routes.

SRS §11 full release checklist is **out of this slice** (needs CI + ER drill).
