# Continuous PDF Prediction Market

Prediction market on a continuous PDF (LMSR) plus a risk-capital auction. Settlement is USDC on Solana L1; trading runs on MagicBlock ER.

## Specs (start here)

| Doc | Role |
| --- | --- |
| [docs/software-requirements-specification.md](docs/software-requirements-specification.md) | SRS — numbered SHALL / SHALL NOT for tickets and QA |
| [docs/product-specification.md](docs/product-specification.md) | Product rules: markets, LMSR, soft solvency, $\rho$, committee |
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

Implementation order: [docs/plans/2026-09-24-implementation-sequence.md](docs/plans/2026-09-24-implementation-sequence.md). Math crate first; Next.js last.
