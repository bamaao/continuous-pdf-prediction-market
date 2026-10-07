# Continuous PDF Prediction Market

Prediction market on a continuous PDF (LMSR) plus a risk-capital auction. Settlement is USDC on Solana L1; trading runs on MagicBlock ER.

## Specs (start here)

| Doc | Role |
| --- | --- |
| [docs/software-requirements-specification.md](docs/software-requirements-specification.md) | SRS — numbered SHALL / SHALL NOT for tickets and QA |
| [docs/product-specification.md](docs/product-specification.md) | Product rules: markets, LMSR (§1.2.4), listing language (§1.2.5), soft solvency, $\rho$, committee |
| [docs/risk-capital-guide.md](docs/risk-capital-guide.md) | Risk LP handbook: auction, rank, draw, surplus 20% cover pool (Chinese: [risk-capital-guide.zh.md](docs/risk-capital-guide.zh.md)) |
| [docs/system-architecture.md](docs/system-architecture.md) | Clients, services, funds, crash recovery, data security |
| [docs/technical-architecture.md](docs/technical-architecture.md) | Next.js / Anchor / Axum, algorithms, wallets |

Diagrams: `docs/business-flow.png`, `docs/system-arch.png`, `docs/tech-arch.png`.

## Locked for implementation

- Client: Next.js only (PWA / wallet WebView / official Android TWA). No store apps.
- Chain: Anchor + MagicBlock ER + session-keys. `vault` / `resolution` never Delegate.
- Collateral: Circle SPL USDC only.
- Outcomes: committee `submit_result` only. No oracle writes $x^*$.
- Solvency: do not reject when $L_{\max}$ is high; settle $L=E(x^*)$; one global $\rho$.

Suggested repo layout is in `docs/technical-architecture.md` section 9.

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
