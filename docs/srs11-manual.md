# SRS §11.1 documented manual tests

Automated evidence is in `docs/srs11-fr-matrix.json`. This page is the **manual** column: a reviewer runs the steps and records pass/fail. A missing procedure is a §11.1 FAIL.

## Wallet and session

| ID | Steps | Pass |
| --- | --- | --- |
| FR-WAL-01 | Desktop: Wallet Adapter “Select Wallet” → Phantom, Solflare, Backpack. Phone without injected wallet: Open in Phantom / Solflare (not a fake in-page wallet). | Named wallets appear; mobile deep-link copy matches |
| FR-WAL-07 | Connect → open Session → Disconnect. On-chain Session still live. Then Revoke Session. Session account status is revoked. | Disconnect ≠ revoke |
| FR-WAL-08 | DevTools → Application. No mnemonic, no Session secret in `localStorage`. Session wrap is IndexedDB + WebCrypto (`packages/sdk/src/session-store.ts`). | No plaintext secrets |

## Client surfaces

| ID | Steps | Pass |
| --- | --- | --- |
| FR-UI-02 | Same origin on desktop Chrome and phone (PWA / in-wallet / TWA). iOS has no TWA. | One Next.js origin |
| FR-UI-03 | Open a Gaussian and a Skellam board. PDF / 11×11, $p_S$, $C_S(q)$, coverage, $\hat\rho$, fee are separate fields. | No field reused as another |
| FR-UI-04 | Board and portfolio copy: fills are public on-chain; no anonymity promise. | Copy review |
| FR-UI-28 | `/ops` has heartbeat only. No Close / Commit / Undelegate / halt button. | Negative UI |
| FR-UI-29 | Nav: Lobby, Portfolio, Committee, Create, Auctions. `/review` only for `R-REVIEW`. | Nav review |
| CR-02 | Product is not submitted to App Store or Google Play. TWA APK is sideload / official wrapper only. | No store listing |
| CR-03 | No Apple IAP or Play Billing SDK in `apps/web` or `apps/android-twa`. | Grep + review |
| CR-10 | Light Protocol, if present, archives **closed** books only. | Architecture review |
| CR-11 | TEE, if present, attests ER only. Never committee or Vault. | Architecture review |
| CR-12 | Implied PDF and $E$ stay public. No private ER encryption of the main book. | Architecture review |

## Durability ops (complements the live drill)

| ID | Steps | Pass |
| --- | --- | --- |
| FR-DUR-03 | After a receipted fill, both `JOURNAL_REPLICA_DIR` and `JOURNAL_OBJECT_DIR` have the same jsonl chain. They are different directories (two copies). | Dual append |
| FR-DUR-04 | Stop Postgres. L1 Vault mint and journal files still read. Next.js never uses `DATABASE_URL`. Recovery: L1 checkpoint → journal replay → indexer. | PG is not the ledger |
| FR-DUR-06 | Keeper `--once` / interval Commit while `now < close_ts` after N fills, not only at halt. | Commit cadence |

Run the live kill drill with `python scripts/srs11-dur-drill.py` (FR-DUR-01 / NFR-09). Do not treat a leftover dump as a pass.
