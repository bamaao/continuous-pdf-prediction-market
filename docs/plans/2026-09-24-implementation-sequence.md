# Implementation Sequence

> **For Claude:** Use this order. Do not start Next.js, ER, or Keeper before `crates/math` and the L1 programs exist. Product rules stay in `docs/product-specification.md`; tickets use `docs/software-requirements-specification.md`.

**Goal:** Build the full in-scope system in dependency order. Sequencing is not an MVP cut — every listed feature still ships.

**Architecture:** Math is one Rust crate. Programs call it. WASM and Quote call the same crate. Clients only compose transactions. Vault and resolution never Delegate.

**Tech stack:** Rust, Anchor, Solana + MagicBlock ER, Axum, Next.js, Circle USDC.

---

## Principle

| Do first | Why |
| --- | --- |
| `crates/math` + CI vectors | INV-01–07; Quote / chain / WASM must match |
| L1 programs (`vault` → `market` → `resolution` → `risk`) | Money and $x^*$ are the ledger |
| `crates/client` + CLI | Create / deposit / resolve without a UI |
| Indexer + Quote + Market API | Read path |
| Trading Gateway + Session | Hot writes |
| Next.js (one site) | Last consumer of stable IDL + APIs |
| MagicBlock Delegate / Commit / journal | After instructions work on local validator |
| Keeper, Notifier, official TWA | After the loop is real |

| Do not start now | Why |
| --- | --- |
| Flutter / RN / store apps | Forbidden (CR-01, XX-11) |
| TEE / Light | After public ledger works |
| Jupiter / multi-mint | Forbidden |
| Committee as a second product | Same Next.js app / `apps/committee` after resolve ixs exist |
| Pixel-perfect all five boards in week 1 | All `create_*` and math families are in-scope; UI lights them as IDL lands |

---

## Phase 0 — Workspace (now)

Create the repo layout from technical-architecture §9. Empty binaries are fine. CI runs `cargo test -p math`.

- `Cargo.toml` workspace: `crates/math`, `crates/math-wasm`, `crates/client`, `crates/cli`, `crates/services/*`, `programs/*`
- `packages/sdk`, `apps/web` placeholders
- `.gitignore` already present
- `README.md` points at docs

**Done when:** `cargo test -p math` is wired (even if only placeholder tests).

---

## Phase 1 — `crates/math` (first real code)

SRS: INV-01–07, NFR-07, NFR-12, FR-TRD-02–04, FR-SET-01–05.

Implement, with tests first:

1. Q64.64 + `exp` / `ln` LUT (no IEEE in public API used by programs)
2. 1D grid LMSR: $Z$, $p_S$, $C_S(q)$, $P_S(q)$, $\theta\leftarrow\theta+q$, $E$, $L_{\max}$
3. 2D football grid + set masks (1X2, totals, integer/half/quarter)
4. Discrete Dirichlet / Bernoulli
5. Priors: truncated Gauss, lognormal-on-log-grid, independent Poisson (+ optional Dixon–Coles), uniform Dirichlet
6. $\rho$, $C_{\max}$, surplus $S$, layer $H_{A,D}$, auction sort (unit premium, stable time)
7. Fee $\phi C$ excluded from payout numerator
8. `crates/math-wasm` wrapping the same functions

**Done when:** CI vectors cover INV-01–07 and a football set-projection case. No TypeScript LMSR.

---

## Phase 2 — Anchor programs on local validator

Order inside this phase (each has tests):

1. **`vault`** — `deposit` / `withdraw` unused; mint constraint USDC; no Session; no `admin_withdraw` (FR-WAL-03, FR-SET-06, CR-05, CR-08)
2. **`market`** — `create_{skellam,gaussian,lognormal,dirichlet,bernoulli}` only (listing names are metadata); store `p0_mass`, $\beta$, timestamps; grid buffer; `buy_set` / `buy_skellam_set` calling `crates/math` (L1 before Delegate). Football: one 2D $\theta$, typed lines expand to $S$ (FR-MKT-05, FR-TRD-11, FR-MKT-12)
3. **`resolution`** — `submit_result`, challenge, $M/N$, finalize, `RESOLUTION_FAILED` refunds (FR-RES-*)
4. **`risk`** — published layers, bid, lock $\ge D$, lowest premium (FR-RSK-*)
5. **`vault.settle`** — $L=E(x^*)$, global $\rho$, surplus only if $\rho=1$ (FR-SET-*)

**Done when:** `solana-test-validator` + Anchor tests: deposit → create CPI market → buy → submit_result → settle with $\rho<1$ fixture.

---

## Phase 3 — Client crate + CLI

SRS: FR-CLI-01. IDL-generated; no second discriminator.

- `crates/client` from IDL
- `crates/cli`: `market create-*`, `trade buy-set`, `risk bid`, `resolve *`, `keeper` stub, `index status` stub

**Done when:** A scripted local loop uses CLI only (no browser).

---

## Phase 4 — Read path

- Indexer: local RPC poll (`crates/services/readpath` `indexer`); Yellowstone later
- Postgres projections optional (`DATABASE_URL` + `infra/docker-compose.readpath.yml`); memory store is the local cache. Not the ledger.
- Quote Engine (`crates/services/quote`, `crates/math` only)
- Market API (`market-api`): `GET /v1/health`, `/v1/markets`, `/v1/markets/{pk}/quote`, `/pdf`, `/ws`

**Done when:** HTTP/WSS can show $p_S$, $C_S$, coverage, $\hat\rho$ matching the last on-chain $\theta$.

Local check: `scripts/phase4-readpath.sh` compares `GET /v1/markets/:id/quote` to `cpm market quote` on the same fill.

---

## Phase 5 — Trading Gateway + Session

SRS: FR-WAL-04–09, FR-TRD-09.

- Session PDA on L1 (`market.open_session` / `renew` / `revoke`); MagicBlock `session-keys` when ER is on (Phase 7)
- In-board `buy_set` / `sell_set` / Skellam fills: Session or main wallet. Vault / resolution / withdraw still main wallet only
- Per-owner per-market `FillNonce`; same nonce retries are no-ops
- Gateway (`trading-gateway`) forwards client-signed txs; never stores keys
- Submit ACK is `pending` after a durable receipt write; confirm is background; same `nonce` retries (NFR-13–20, FR-TRD-09)

**Done when:** Two buys with the same Session, then withdraw still requires main wallet; gateway `pending` receipt survives a gateway process restart.

Local check: `scripts/phase5-session.sh`

---

## Phase 6 — Next.js (`apps/web`)

Same app for lobby, board, auction, portfolio, resolve. `apps/committee` can be a route group in the same app until it earns a split.

Order of screens: connect + SIWS → deposit → board (1D + football heat) → auction → resolve → portfolio.

WASM quotes from `crates/math-wasm`. No second LMSR.

**Done when:** Manual path matches CLI loop on localnet.

---

## Phase 7 — MagicBlock ER + journal

- Delegate market + grid; `buy_set` ER-only when `is_delegated`
- Periodic Commit + `trades_root`
- Fill journal (ER replica log + object-store append) — FR-DUR-*
- Undelegate at `close_ts`

**Done when:** Kill one ER process; receipted fills replay; Vault USDC unchanged.

---

## Phase 8 — Keeper, notify, TWA

- Keeper: close, Commit, open report window, idempotent
- Notifier: email / Web Push (`market_id` only)
- Official Android TWA wrapper (no new business code)
- Staging + production config isolation

**Done when:** SRS §11 release checklist can be run.

---

## Parallelism (only after Phase 1)

| Track A | Track B |
| --- | --- |
| Programs Phase 2 | math-wasm + Quote types |
| After Phase 2: CLI | After Phase 2: Indexer schema |
| After Phase 5: Web wallet | After Phase 4: Quote WSS |

Do not parallelize a second math implementation.

---

## First tickets (Phase 0–1)

1. Workspace + `crates/math` crate
2. Q64.64 tests
3. 1D LMSR tests then impl
4. $\rho$ / $C_{\max}$ / surplus / $H$ tests then impl
5. 2D football + set masks
6. Remaining priors
7. WASM stub exporting the same fns

Then stop and run Phase 2 vault tests.
