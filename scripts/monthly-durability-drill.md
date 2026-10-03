# Monthly durability drill (NFR-09)

Run once per calendar month before a release candidate. Record PASS / FAIL with timestamps.

## Preconditions

- Local L1 (`:8899`) + ER (`:7799`) + Market API + machine PostgreSQL `:5432`
- `DATABASE_URL` set; `ALLOW_MEMORY_ONLY` unset
- A receipted fill exists (any family)

## Steps

1. **Indexer kill** — stop the indexer process for 60s; confirm Market API still serves last projections; restart; confirm lag recovers (`cpm index-status` or `GET /v1/ops/status`).
2. **Keeper failover** — start two `python scripts/keeper-standby.py --interval 5` with different `KEEPER_ID`; kill the active holder; confirm the standby takes the lease within `KEEPER_LEASE_SECS` and runs `keeper --once`.
3. **ER kill** — stop ephemeral-validator; confirm open boards reject new ER fills; restart ER; confirm undelegate / writeback can resume.
4. **PG primary kill** (admin) — `python scripts/srs11-dur-drill.py` as Administrator on Windows (or equivalent). Dump leftover = FAIL. Receipted fills and Vault identity must survive.

## Record

| Date | Indexer | Keeper HA | ER | PG | Notes |
| --- | --- | --- | --- | --- | --- |
| | | | | | |

Do not claim SRS §11.3 until step 4 PASSes.
