# Continuous PDF Prediction Market

Prediction market on a **continuous PDF** (LMSR) plus a **risk-capital auction**. Settlement is Circle SPL USDC on Solana L1; trading runs on MagicBlock ER.

This README is a three-minute **business map**. Normative detail lives in the specs below — do not treat this page as the source of truth for formulas or SHALL rules.

## Specs (start here)

| Doc | Role |
| --- | --- |
| [docs/software-requirements-specification.md](docs/software-requirements-specification.md) | SRS — numbered SHALL / SHALL NOT for tickets and QA |
| [docs/product-specification.md](docs/product-specification.md) | Product rules: markets, LMSR (§1.2.4), listing language (§1.2.5), soft solvency, $\rho$, committee |
| [docs/risk-capital-guide.md](docs/risk-capital-guide.md) | Risk LP handbook: auction, rank, draw, surplus 20% cover pool (Chinese: [risk-capital-guide.zh.md](docs/risk-capital-guide.zh.md)) |
| [docs/system-architecture.md](docs/system-architecture.md) | Clients, services, funds, crash recovery, data security |
| [docs/technical-architecture.md](docs/technical-architecture.md) | Next.js / Anchor / Axum, algorithms, wallets |

Diagrams (if present): `docs/business-flow.png`, `docs/system-arch.png`, `docs/tech-arch.png`.

Public landing page (static): [`website/`](website/) — deployed by [`.github/workflows/pages.yml`](.github/workflows/pages.yml). After the first push, set **Settings → Pages → Source** to **GitHub Actions**. Site URL:

`https://bamaao.github.io/continuous-pdf-prediction-market/`

Docs on Pages: `…/docs/readme.html` (and sibling pages for product / SRS / risk / architecture). Lifecycle demo videos live under `website/assets/video/`.

Local preview: open `website/index.html` or `npx serve website`. Regenerate doc HTML after editing specs: `python website/build-docs.py` (CI also runs this).

---

## Core business

The product discovers two prices at once:

| Market | What is discovered |
| --- | --- |
| **A — Prediction** | $P(X\in I)$ — probability that the outcome falls in an interval / atom set |
| **B — Risk capital** | Cost of underwriting the gap when final payout exceeds the market’s own trading revenue |

$$
\text{Probability Discovery} + \text{Risk Capital Discovery}
$$

Unlike YES/NO books, each listing maintains a density $f(x)$ with $\int f=1$. Traders buy a set $S$ (interval, score cell, election atom, YES…). Price is current probability mass; **hit pays 1 USDC per share** (then × market-wide $\rho$), miss pays 0.

Distribution families (locked at create; cannot swap later):

| Family | Typical underlying | Create ix |
| --- | --- | --- |
| Skellam | Football / scores | `create_skellam_market` |
| Gaussian | CPI / macro prints | `create_gaussian_market` |
| Lognormal | Same-day BTC / ETH | `create_lognormal_market` |
| Dirichlet | Elections | `create_dirichlet_market` |
| Bernoulli | Deadline YES / NO | `create_bernoulli_market` |

Product copy: users create a **prediction market**. Do not use “board”, “book”, or “open/close the book” as product nouns.

Full definition: [product-specification §1](docs/product-specification.md#1-product-definition).

---

## Core concepts

| Concept | Meaning |
| --- | --- |
| $f(x)$ / $\theta$ | Prior at create; fills update $\theta$, LMSR renormalizes $f$. Frozen after `close_ts`. Committee reports $x^*$, never rewrites $f$. |
| Shares $q$ on set $S$ | Position size. Hit face = $q$ USDC (ordinary lines), not $q/p_S$. |
| $C_S(q)$ | LMSR contract cost into the vault (payout stack). Fee $\phi\cdot C_S$ is separate. |
| $R_{\mathrm{net}}$ / $T$ | Trading revenue (buyer contract cost). Fees never enter. |
| $L = E(c)$ | Liability at the realized atom after $x^*$ is final — **not** trading-period $L_{\max}$. |
| $C_R$ | Risk LP capital locked via the auction; may be 0. |
| $C_P$ | Optional platform adjustment pool + listing tap; capped; default tap 0. |
| $C_{\max}$ | $T + C_R^{\mathrm{final}} + C_P^{\mathrm{alloc}}$ — maximum payable to winners. |
| $\rho$ | One market-wide recovery rate $\min(1, C_{\max}/L)$, written **before** any winner claim. |
| Coverage / $\hat\rho$ | Display-only during trading; not settlement $\rho$. |
| Protocol PDA | Official `platform`, `fee_bps`, report clocks, $\alpha_R$, etc. Create copies them onto `Market`; applicants cannot choose. |

Roles (detail: [§3](docs/product-specification.md#3-roles)):

| Role | Job |
| --- | --- |
| **Trader** | Buy probability sets; claim $\lfloor\rho\cdot\mathrm{face}\rfloor$ if hit |
| **Applicant / reviewer** | SIWS apply → review approve opens trading **and** the risk auction |
| **Risk LP** | Tail underwriter: lock $D_i$, may be drawn if $T < L$; earn premium / surplus share when $\rho=1$ |
| **Committee / resolver** | On-chain `submit_result` for $x^*$ — no oracle writes outcomes |
| **Platform** | Claims $\phi$; optional $S_P$; funds $C_P^{\mathrm{pool}}$; admin path after missed report |

---

## Core business flow

Lifecycle sketch ([§1.2](docs/product-specification.md#12-end-to-end-business-flow), [§11](docs/product-specification.md#11-market-lifecycle)):

```text
① Choose distribution family
② Apply (SIWS) → reviewer approve / create_*  → trading + risk auction OPEN
③ Trade (LMSR buys) ‖ Risk LPs quote / lock C_R
④ close_ts → stop prediction fills AND risk auction; freeze f
⑤ Wait for the event
⑥ report_open_ts… → committee submit_result → challenge / finalize x*
     (missed window → only platform admin_submit_result or admin_void_resolution)
⑦ begin_settle → lock L, C_max, ρ → winners claim; draw Risk LP / C_P if needed
⑧ If ρ=1 and surplus S>0 → 20% S to LP cover pool; rest α_R / α_P (or all S_P if no C_R)
```

Settlement cases (same section):

| Funds vs $L$ | $\rho$ | What winners get |
| --- | --- | --- |
| $L \le T$ | 1 | Full face; no risk draw |
| $T < L \le C_{\max}$ | 1 | Full face; draw $C_R$ then $C_P$ as needed |
| $L > C_{\max}$ | $C_{\max}/L$ | Every winner the **same** ratio — no FIFO |

VOID / `RESOLUTION_FAILED` refund `cost_paid`; they do **not** use $\rho$.

---

## Core business rules (locked)

These are protocol rules, not UI hints. Programs, Quote, and desks must follow them. Full table: [product-specification §1.1](docs/product-specification.md#11-locked-business-rules).

1. **Sells probability; share face is 1.** Price ≈ $p_S$; hit pays 1 per share (× $\rho$). Never treat face as $1/p_S$. ([§1.2.2](docs/product-specification.md#122-core-business-rule-the-product-sells-probability-share-face-is-1-locked))
2. **Popular stretches get dearer.** Hot / overlapping buys raise $C_S$ via LMSR; not a wall-clock markup. ([§1.2.4](docs/product-specification.md#124-core-business-rule-popular-intervals-get-more-expensive-locked))
3. **Sells are closed.** No unwind at live LMSR; hold to settlement claim or VOID refund.
4. **Soft solvency.** Do not reject fills when $L_{\max}$ is high; settle on $L=E(c)$ with one global $\rho$.
5. **One $\rho$, users first.** Write $\rho$ before any payout; surplus only when $\rho=1$; FIFO forbidden. ([§1.2.1](docs/product-specification.md#121-core-business-rule-recovery-rate-and-surplus-locked))
6. **Fees $\phi$ ≠ surplus $S_P$ ≠ $C_P$.** $\phi$ always platform (`claim_fees`); never enters $C_{\max}$ / cover pool. ([§1.2.7](docs/product-specification.md#127-core-business-rule-phi-is-always-the-platform-s_p-is-settlement-leftover-locked))
7. **Official platform params from Protocol.** Create copies them; lobby hides markets whose `platform` ≠ official key.
8. **Close stops both markets.** At `close_ts`, prediction fills and risk-auction quotes/fills stop (`risk_lock_ts ≤ close_ts`).
9. **Committee writes $x^*$.** No oracle outcome writes. Missed report: platform slash-or-VOID only — no auto-fail / auto-extend.
10. **Canonical listing language is English**; locale strings are display-only. ([§1.2.5](docs/product-specification.md#125-core-business-rule-how-a-global-lobby-names-a-market-locked))
11. **Anyone with SIWS may apply; review opens trading.** Duplicate event key → `409`. Rejected apps never become markets.
12. **Surplus cover slice.** When $\rho=1$ and $S>0$, first $S_C=20\%S$ funds the protocol LP cover pool (`cover_lp_loss`); remainder splits per listing $\alpha_R$/$\alpha_P$.

Invariants summary: [§12](docs/product-specification.md#12-protocol-invariants). Risk LP economics: [risk-capital-guide](docs/risk-capital-guide.md).

---

## Locked for implementation

- Client: Next.js only (PWA / wallet WebView / official Android TWA). No store apps.
- Chain: Anchor + MagicBlock ER + session-keys. `vault` / `resolution` never Delegate.
- Collateral: Circle SPL USDC only.
- Outcomes: committee `submit_result` only. No oracle writes $x^*$.
- Solvency: do not reject when $L_{\max}$ is high; settle $L=E(x^*)$; one global $\rho$.

Suggested repo layout: [docs/technical-architecture.md](docs/technical-architecture.md) §9.

Implementation order: [docs/plans/2026-09-24-implementation-sequence.md](docs/plans/2026-09-24-implementation-sequence.md).

```bash
# Machine PostgreSQL on :5432 (do not start Docker if that port is already taken):
#   psql -U postgres -h 127.0.0.1 -f infra/local-pg.sql
# DATABASE_URL defaults to postgres://cpm:cpm@127.0.0.1:5432/cpm
cargo run -p readpath --bin market-api
cd apps/web && npm install && npm run dev
```

Staging (Linux): prep with `scripts/staging-prep-ubuntu.sh` or `scripts/staging-prep-centos.sh`, then `bash scripts/deploy-staging.sh` — [deploy/README.md](deploy/README.md).

Listing names and the fill journal live in **local Postgres**, not process memory. A Market API restart reloads them. `ALLOW_MEMORY_ONLY=1` is only for unit tests.

Web talks to `market-api` (`:8080`), Trading Gateway (`:8081`), and local RPC. Quotes come from `crates/math` (WASM crate + Market API). Do not reimplement LMSR in TypeScript. Persistence: `docs/architecture/ddd-sqlx.md`.

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
