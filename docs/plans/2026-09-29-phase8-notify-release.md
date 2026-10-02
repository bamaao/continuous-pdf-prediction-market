# Phase 8 remainder — inbox feed + local SRS §11 runner

> **For Claude:** Official sequence ends at Phase 8. This slice is leftover Phase 8, not a new official phase. Local-first. No MagicBlock ER. No TWA APK. Do not claim SRS §11 item 3 (ER + PG primary kill) passed. Never silent-SKIP.

**Goal:** Inbox reads `GET /v1/notify`; keeper events are `{ts,kind,market}` only; a local SRS §11 runner executes existing tests and prints PASS / FAIL / SKIP with reasons.

**Architecture:** Keeper `append_event` writes `events.jsonl`. Next.js inbox polls Market API. `/ops` stays read-only.

**Tech stack:** `crates/notify`, `packages/sdk`, `apps/web` Inbox, `scripts/srs11-local.py`

---

## Done when

1. Keeper writes `events.jsonl`; each line is `{ts,kind,market}` only.
2. Inbox polls `/v1/notify` and lists `market` as the link.
3. `scripts/srs11-local.py` runs math, journal, gateway, resolution, vault, notify, settle-flow FR-SET-03, sdk tests; the ER + PG drill is SKIP with a reason.
4. `.env.staging.example` and `.env.production.example` exist; non-local still forbids `ALLOW_MEMORY_ONLY`.

Out of slice: Bubblewrap APK, MagicBlock ER RPC, “every FR-* has a dedicated test” as one suite.
