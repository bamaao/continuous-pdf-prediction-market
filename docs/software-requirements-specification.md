# Software Requirements Specification

**Continuous PDF Prediction Market**

| Item | Content |
| --- | --- |
| Document | SRS |
| Version | 1.1 |
| Status | Baseline for implementation |
| Audience | Engineers, QA, reviewers |
| Normative sources | `product-specification.md` (product rules), `system-architecture.md`, `technical-architecture.md` |
| Language | English |

This SRS states **what the system shall do** so work can be ticketed and accepted. Formulas, market-type construction, and algorithms stay in the product / technical specs. If this file conflicts with `product-specification.md` on a product rule, **the product specification wins**; then this SRS must be updated.

Priority: every in-scope requirement is **MUST**. There is no MVP slice. Out-of-scope items are **SHALL NOT**.

Keywords follow RFC 2119: SHALL, SHALL NOT, SHOULD, MAY.

---

## 1. Purpose and scope

### 1.1 Purpose

Specify verifiable requirements for a continuous-PDF prediction market: traders buy outcome sets on an LMSR curve; Risk LPs sell layered tail cover; a committee writes $x^*$ on-chain; payout uses one global recovery rate $\rho$.

### 1.2 In scope

- Five distribution families: Skellam (football scores), Gaussian, lognormal, Dirichlet, Bernoulli. Listing names are metadata, not create instructions.
- LMSR trading on MagicBlock ER; funds and settlement on Solana L1
- Risk-capital auction in parallel with trading
- Soft solvency and pro-rata settlement
- Next.js client (web, PWA, in-wallet browser, official-site Android TWA)
- Rust Axum services, Anchor programs, CLI for ops

### 1.3 Out of scope

See §10. Those items SHALL NOT be implemented and SHALL NOT be ticketed as “later”.

### 1.4 Document map

| Need | Read |
| --- | --- |
| Why / economic rules / market construction | `product-specification.md` |
| Machines, paths, recovery, security | `system-architecture.md` |
| Stack, algorithms, wallets, TEE boundary | `technical-architecture.md` |
| Shall / shall-not for build and QA | this file |
| Pictures | `business-flow.png`, `system-arch.png`, `tech-arch.png` |

---

## 2. Stakeholders and roles

| ID | Role | Capability |
| --- | --- | --- |
| R-TRADER | Trader | Deposit USDC, buy/sell sets, view PDF / coverage / $\hat\rho$, withdraw unused margin, receive payout |
| R-LP | Risk LP | Quote published layers, lock collateral, collect premium / surplus share, take layer loss $H$ |
| R-CREATOR | Authorized creator | Create markets, inject $C_M$, set $\beta$, family, committee, timestamps |
| R-CMTE | Committee member / authorized reporter | `submit_result`, challenge, vote; attach evidence |
| R-KEEP | Keeper | `close` / Commit / Undelegate, open report window, alerts |
| R-OPS | Operator (read) | Dashboards, never Vault withdraw
| R-PLAT | Platform | Receive $\phi$ fees and $\alpha_P$ surplus when $\rho=1$ |

Identity is a Solana pubkey. There is no password account.

---

## 3. Definitions (normative)

| Term | Meaning |
| --- | --- |
| $f(x)$ / $P$ | Market PDF or discrete mass; $\int f=1$ or $\sum P=1$ |
| $\theta$ | LMSR state; buy $S$ of size $q$ does $\theta_k\leftarrow\theta_k+q$ on $k\in S$ |
| $C_S(q)$ | LMSR cost of buying $q$ of set $S$ |
| $\phi$ | Platform fee rate; fee $=\phi\cdot C_S(q)$ |
| $E(x)$ | Face exposure at $x$ |
| $L_{\max}$ | $\sup_x E(x)$ (display / auction signal only) |
| $L$ | Settlement liability $E(x^*)$ |
| $C_M$ | Market capital injected at listing |
| $C_R^{\mathrm{final}}$ | Locked, drawable Risk LP capacity at settlement |
| $R_{\mathrm{net}}$ | Trading proceeds net of fees and owed premia |
| $C_{\max}$ | $R_{\mathrm{net}}+C_M+C_R^{\mathrm{final}}$ |
| $\rho$ | $\min(1,C_{\max}/L)$; one value for every winner |
| Receipted fill | ER (or L1) executed the ix, returned a signed receipt, and the receipt reached a replicated journal quorum |
| Session | Time-limited, amount-limited delegated signer for `buy_set` / `sell_set` only |
| USDC | Circle official SPL USDC mint on the target cluster |

---

## 4. Functional requirements

### 4.1 Markets and priors

| ID | Requirement | Verify |
| --- | --- | --- |
| FR-MKT-01 | The system SHALL create a market only with a locked distribution family, $\beta$, grid or atoms, $C_M$, committee (or public committee id), `close_ts`, `risk_lock_ts`, and a resolution rule. | Create ix rejects missing fields |
| FR-MKT-02 | The system SHALL NOT change the distribution family after creation. | Trade / admin ix cannot rewrite family |
| FR-MKT-03 | At creation $\theta=0$. Displayed prices SHALL equal the prior $f_0$ / $P_0$. | Quote vs stored `p0_mass` |
| FR-MKT-04 | $f_0$ / $P_0$ SHALL be written once and SHALL NOT be rewritten by oracles or operators. | No ix mutates `p0_mass` after create |
| FR-MKT-05 | Football: one match `score_scope` SHALL be one Skellam board sharing a 2D score PDF. Derived books (1X2, totals, spreads, exact score) SHALL be set projections, not separate $\theta$ stores. | Shared `theta[i][j]`; `buy_skellam_set` |
| FR-MKT-06 | Football SHALL trade pre-match and in-play until `close_ts` (default: that scope’s full-time whistle). Kickoff SHALL NOT close the book. | Clock vs `close_ts` only |
| FR-MKT-07 | CPI / macro listings SHALL call `create_gaussian_market`. $x^*$ is the first official print. | Create + resolve path |
| FR-MKT-08 | Election winner, `TOP_N`, and vote-share SHALL all use `create_dirichlet_market` with `layout` atoms / top-$n$ / simplex. There SHALL NOT be a separate vote-share family. | Three layouts + resolve |
| FR-MKT-09 | Daily price listings SHALL call `create_lognormal_market` and lock `price_rule`. | Create + `price_rule` + resolve |
| FR-MKT-10 | Binary events SHALL use `create_bernoulli_market` (YES/NO). YES MAY finalize early only when the defined event has occurred, not because it “looks likely”. | Early-YES guard |
| FR-MKT-11 | Listing SHALL open that board’s risk-auction book immediately. | Auction book exists after create |
| FR-MKT-12 | On-chain create SHALL be the five family instructions only: `create_skellam_market`, `create_gaussian_market`, `create_lognormal_market`, `create_dirichlet_market`, `create_bernoulli_market`. Listing names are metadata. | IDL whitelist |

### 4.2 Wallet, deposit, session

| ID | Requirement | Verify |
| --- | --- | --- |
| FR-WAL-01 | The client SHALL connect via Wallet Standard (web) or the equivalent in-wallet / TWA path (mobile). Required wallets: Phantom, Solflare, Backpack. | Manual + adapter tests |
| FR-WAL-02 | After connect, the client SHALL run SIWS; the BFF MAY issue a JWT for query / push only. JWT SHALL NOT authorize on-chain spends. | JWT cannot `buy_set` |
| FR-WAL-03 | The user SHALL `vault.deposit` USDC on L1 with the **main wallet** before trading. Unconfirmed deposits SHALL NOT increase ER spendable balance. | Balance after finalized deposit only |
| FR-WAL-04 | Opening, renewing, or revoking a Session SHALL require the main wallet. | Session ix signer |
| FR-WAL-05 | A Session SHALL encode expiry, remaining USDC, allowed instructions (`buy_set` / `sell_set` / `buy_skellam_set` / `sell_skellam_set` only), and an optional market whitelist. | On-chain session account |
| FR-WAL-06 | In-board set buys and sells SHALL be signed by the Session. Withdraw, create, inject $C_M$, risk `bid`, and resolution SHALL require the main wallet (or KMS for keepers / reporters). | Negative tests |
| FR-WAL-07 | The UI SHALL expose revoke-session separately from disconnect-wallet. Disconnecting SHALL NOT be treated as on-chain revoke. | UX + chain state |
| FR-WAL-08 | The system SHALL NOT store mnemonic phrases. Session secrets SHALL NOT be stored in plaintext `localStorage`. | Review + scanner |
| FR-WAL-09 | Trading Gateway SHALL forward signed ER txs and SHALL NOT hold Session private keys. | Code review |

### 4.3 Trading (LMSR)

| ID | Requirement | Verify |
| --- | --- | --- |
| FR-TRD-01 | `buy_set` / `sell_set` / `buy_skellam_set` / `sell_skellam_set` SHALL execute only on ER while the market is delegated and `now < close_ts`. L1 is allowed only before Delegate (tests). | Status + clock |
| FR-TRD-02 | Fill price SHALL be the LMSR pure probability $p_S$ / $C_S(q)$. Coverage / $\hat\rho$ SHALL be displayed and SHALL NOT be baked into the quote. | Quote vs chain cost |
| FR-TRD-03 | Buying the same $S$ again SHALL raise $p_S$ and $C_S$ (LMSR). | Monotonicity test |
| FR-TRD-04 | Each fill SHALL charge fee $\phi\cdot C_S(q)$ to the platform. Fees SHALL NOT enter the payout pool or $R_{\mathrm{net}}$ as user capital. | Vault ledgers |
| FR-TRD-05 | The system SHALL NOT reject a valid order because $L'_{\max}>C_M+C_R$. | Case $L_{\max}$ huge, balance OK |
| FR-TRD-06 | The system SHALL reject only: insufficient USDC available, illegal set / $q$, market not TRADING, Session unauthorized, or nonce replay. | Negative tests |
| FR-TRD-07 | After a fill the system SHALL update $\theta$, $E$, $L_{\max}$, and broadcast the new PDF. | Event + indexer |
| FR-TRD-08 | Low coverage SHALL trigger a strong UI warning and SHALL still allow the order. | UI + chain accept |
| FR-TRD-09 | A fill is complete only when FR-DUR-01 holds. Until then the client SHALL show `pending` and retry the same `nonce`. | Idempotency |
| FR-TRD-10 | Quote Engine preview SHALL be read-only and SHALL NOT be the ledger. | Preview ≠ settle |
| FR-TRD-11 | Skellam fills SHALL use one shared $\theta_{ij}$. Typed lines SHALL go through `buy_skellam_set` (expand $S$, then `crates/math::lmsr_update`). Custom unions MAY use `buy_set`. $L_{\max}$ SHALL be $\max_{ij}E_{ij}$. Quarter lines SHALL be two half-fills of $q/2$ on the same book. Programs SHALL NOT implement a second LMSR. | Home buy raises exact 2-1; over + AH stack on intersection; $p_{1}+p_{X}+p_{2}=1$ |

### 4.4 Risk auction

| ID | Requirement | Verify |
| --- | --- | --- |
| FR-RSK-01 | Layers SHALL be those published at listing. LPs SHALL NOT invent attachments. | Bid ix checks layer id |
| FR-RSK-02 | Fills SHALL consume the lowest unit premium first within a layer, subject to capacity and concentration $\gamma$. | Matching test |
| FR-RSK-03 | An LP’s locked collateral SHALL be $\ge D_i$. Unlocked promises SHALL NOT count in $C_R$. | Vault vs book |
| FR-RSK-04 | After fill, that LP’s $D_i$ SHALL NOT increase because later traders raise $L_{\max}$. | Immutable $D_i$ |
| FR-RSK-05 | `risk_lock_ts` SHALL be required, and `close_ts \le risk_lock_ts`. After `risk_lock_ts`, new auction fills SHALL stop. | Clock |
| FR-RSK-06 | Auction SHALL NOT block prediction trading. | Parallel books |
| FR-RSK-07 | Layer payout SHALL be $H_{A,D}(L)=\min((L-A)^+,D)$. | Settlement vector |
| FR-RSK-08 | One collateral lock SHALL underwrite one board only. | PDA / accounting |

### 4.5 Halt, resolution, settlement

| ID | Requirement | Verify |
| --- | --- | --- |
| FR-HAL-01 | At `close_ts` Keeper SHALL stop prediction fills and Commit / Undelegate $\theta$, $E$, `trades_root` to L1. | Clock + accounts |
| FR-RES-01 | $x^*$ SHALL be written only by `submit_result`. Pyth and sports APIs SHALL be evidence, never writers of $x^*$. | No auto-oracle ix |
| FR-RES-02 | After a proposal, a challenge window SHALL run. No challenge → finalize. Challenge → $M/N$ vote. | State machine |
| FR-RES-03 | Failed vote SHALL extend or `RESOLUTION_FAILED`. Failed finalization SHALL refund users, return LP collateral, and return unused premium. | Refund balances |
| FR-RES-04 | Football SHALL report a score pair; CPI the first official print; election the defined winner / TOP_N set / shares; price the `price_rule` scalar; binary YES or NO. | Type-specific accounts |
| FR-RES-05 | Live Pyth evidence, if used, SHALL be read in `[observe_ts, observe_ts+\Delta]` in the same transaction. After that window, current Pyth SHALL NOT be treated as the historical price. | Time + feed checks |
| FR-SET-01 | Settlement SHALL use $L=E(x^*)$, not $L_{\max}$. | Fixture $E\neq L_{\max}$ |
| FR-SET-02 | $C_{\max}$ SHALL equal $R_{\mathrm{net}}+C_M+C_R^{\mathrm{final}}$. | Ledger identity |
| FR-SET-03 | $\rho=\min(1,C_{\max}/L)$ (or $\rho=1$ if $L=0$). Every winner SHALL receive $\rho\cdot q$. FIFO or entry-order haircuts SHALL NOT be used. | All winners same $\rho$ |
| FR-SET-04 | Surplus $S=\max(R_{\mathrm{net}}+C_M-L,0)$ SHALL be paid only if $\rho=1$, split $\alpha_R+\alpha_P=1$. If $\rho<1$ then $S=0$. | Surplus cases |
| FR-SET-05 | Dust from $\lfloor\rho q\rfloor$ SHALL go to reserves, not to a preferred user. | Remainder account |
| FR-SET-06 | There SHALL be no `admin_withdraw`. Outflows are: user unused margin, settlement payout, LP draw, surplus split, VOID / failed-resolution refunds. | Instruction whitelist |

### 4.6 Durability and recovery

| ID | Requirement | Verify |
| --- | --- | --- |
| FR-DUR-01 | A receipted fill SHALL persist across a single-node crash. $\theta$ SHALL be reconstructible as $\mathrm{Update}(\theta_0,\mathrm{trade}_1,\ldots,\mathrm{trade}_t)$. | Kill ER / indexer / PG, replay |
| FR-DUR-02 | L1 Commit SHALL be a checkpoint, not the definition of “fill exists”. | Fills between commits survive |
| FR-DUR-03 | The fill journal SHALL have at least two durable copies that do not share a process (ER replica log and object-store append log as specified). | Ops checklist |
| FR-DUR-04 | Postgres / Redis SHALL NOT be the ledger. Recovery order: L1 checkpoint → replay journal → rebuild index. | Runbook drill |
| FR-DUR-05 | Recovery SHALL NOT delete receipted fills to “align” state. Mismatch SHALL halt the board and alert. | Fault injection |
| FR-DUR-06 | ER SHALL Commit on an interval (time or N fills), not only at `close_ts`. | Commit cadence |

### 4.7 Client and ops surfaces

| ID | Requirement | Verify |
| --- | --- | --- |
| FR-UI-01 | Next.js SHALL provide lobby, `/m/[id]`, `/auction/[id]`, `/portfolio`, `/resolve/[id]`. | Routes |
| FR-UI-02 | Mobile SHALL be the same site (PWA / in-wallet browser / official TWA). | Same origin / build |
| FR-UI-03 | The client SHALL show PDF (or 11×11 football heat), $p_S$, $C_S(q)$, coverage, $\hat\rho$, and fee separately. | UI review |
| FR-UI-04 | Copy SHALL state that fills are public on-chain. The product SHALL NOT promise on-chain anonymity. | Copy review |
| FR-CLI-01 | CLI SHALL support create-*, close/undelegate, buy-set, risk bid, resolve, keeper, index status. | Command list |
| FR-IDX-01 | Indexer SHALL follow ER + L1 and rebuild from the journal after lag. Reads SHALL be shed if lag exceeds the threshold. | Lag metric |
| FR-NTF-01 | Notifications SHALL use Web Push / email / in-app, payload `market_id` only. | Push contract |

---

## 5. External interface requirements

| ID | Interface | Requirement |
| --- | --- | --- |
| IR-01 | Solana L1 | Vault, create, Delegate/Commit, resolution, settle; dedicated RPC, not a public free endpoint for users |
| IR-02 | MagicBlock ER | `buy_set` / `sell_set` / optional risk fills; `<10` ms in-chain target |
| IR-03 | Wallet Adapter | Web: `@solana/wallet-adapter-react`. Mobile: deep link / injected provider / MWA on TWA |
| IR-04 | USDC mint | Circle official SPL USDC; `mint == USDC_MINT` on every funds ix |
| IR-05 | Pyth | Optional same-tx evidence; never scheduler of $x^*$ |
| IR-06 | Object store | Committee evidence and append-only fill journal; L1 stores hashes / `trades_root` |
| IR-07 | BFF / Market API | Metadata, positions, snapshots; no hot-path fill |
| IR-08 | Quote WSS | PDF / book push, `<50` ms target |
| IR-09 | Trading Gateway | Client-signed ER txs, `<10` ms ER execution excluding wallet UI |
| IR-10 | IDL | One Anchor IDL for `packages/sdk` and `crates/client`; no hand-rolled discriminators |

---

## 6. Non-functional requirements

| ID | Requirement | Target |
| --- | --- | --- |
| NFR-01 | ER `buy_set` execution | $<10$ ms (excludes wallet popup) |
| NFR-02 | ER gas | 0 |
| NFR-03 | Quote / position API | $<150$ ms |
| NFR-04 | PDF WebSocket | $<50$ ms |
| NFR-05 | Halt Commit / Undelegate | seconds |
| NFR-06 | Grid size | $N=256\sim1024$ (1D); football $11\times11$ |
| NFR-07 | Consensus math | Q64.64; no IEEE float; `crates/math` shared by chain, Quote, WASM |
| NFR-08 | Index rebuild | From journal / chain; RPO for PG query copy $\le1$ min (not ledger) |
| NFR-09 | Monthly drill | Kill Indexer, Keeper, PG primary, one ER; Vault identity and receipted fills unchanged |
| NFR-10 | Observability | ER latency, fill success, index lag, coverage, Vault balance, Keeper heartbeat |
| NFR-11 | Availability (Keeper) | Active-standby; idempotent `close` |
| NFR-12 | Precision | Same test vectors on chain, Quote, WASM |

---

## 7. Constraints (locked stack)

| ID | Constraint |
| --- | --- |
| CR-01 | User client SHALL be Next.js only. Flutter, React Native, native store apps SHALL NOT be built. |
| CR-02 | The system SHALL NOT submit a trading app to the App Store or Google Play. |
| CR-03 | Apple IAP and Play Billing SHALL NOT be used. |
| CR-04 | Programs SHALL be Anchor + `ephemeral-rollups-sdk` + `session-keys`. |
| CR-05 | Four programs: `market`, `risk`, `vault`, `resolution`. `vault` and `resolution` SHALL never Delegate and SHALL reject Session as authority. |
| CR-06 | All backend services SHALL be Rust Axum. |
| CR-07 | Chain clients SHALL be `@coral-xyz/anchor` + `@solana/web3.js` (not `@solana/kit` in parallel). |
| CR-08 | Collateral and payouts SHALL be Circle SPL USDC only. SOL pays L1 fees only. |
| CR-09 | No in-protocol swap, Jupiter CPI, or second mint. |
| CR-10 | Light Protocol, if used, SHALL archive **closed** books only; never Vault or hot $\theta$. |
| CR-11 | TEE, if used, SHALL attest ER integrity and MAY attest a live Pyth read. TEE SHALL NOT replace the committee or L1 Vault. |
| CR-12 | PDF / $E$ / $L_{\max}$ SHALL remain public. Private ER encryption SHALL NOT be used for the main book. |

---

## 8. Data and security requirements

| ID | Requirement |
| --- | --- |
| DR-01 | Public book ($f$, fills, $E$, $x^*$, $\rho$, payouts) SHALL be public and recomputable. |
| DR-02 | Secrets (upgrade keys, keeper keys, KMS, DB, RPC tokens) SHALL use least privilege and KMS/HSM. |
| DR-03 | PII (email, push tokens) SHALL be encrypted, off-chain, and not written to L1. |
| DR-04 | TLS in transit; KMS-backed encryption at rest for PG, object store, backups. |
| DR-05 | Evidence objects: private bucket, L1 hash only. |
| DR-06 | Logs MAY include truncated pubkey; SHALL NOT include raw email, phone, government id, or raw IP by default. |
| DR-07 | A stolen index DB SHALL NOT enable an extra USDC transfer. |

---

## 9. Protocol invariants (test in CI)

| ID | Invariant |
| --- | --- |
| INV-01 | $\sum_k p_k=1$ (or $\int f=1$) after every fill |
| INV-02 | Buying $S$ does not decrease $p_S$ |
| INV-03 | If $\rho=1$, winners are paid face $q$; else $\sum$ paid $=\lfloor\rho\cdot$ faces$\rfloor$ with dust to reserves |
| INV-04 | $\sum$ USDC paid to winners $\le C_{\max}$ |
| INV-05 | Vault token balance equals the accounting sum |
| INV-06 | Same $\rho$ for every winner on a board |
| INV-07 | Fees never sit in the user payout numerator |

---

## 10. SHALL NOT (out of scope)

The system SHALL NOT:

| ID | Forbidden |
| --- | --- |
| XX-01 | Bake expected $\rho$ into LMSR price |
| XX-02 | Haircut by arrival time / FIFO |
| XX-03 | Hard-reject trading because $L_{\max}\le C_M+C_R$ fails |
| XX-04 | Let one LP lock underwrite multiple boards |
| XX-05 | Use a parameterized AMM (only $\mu,\sigma$ or $\lambda_H,\lambda_A$) |
| XX-06 | Run a full on-chain CVaR portfolio optimizer |
| XX-07 | Ship committee token governance or a large on-chain court |
| XX-08 | Accept SOL or other SPL tokens as margin / collateral / payout |
| XX-09 | Auto-swap arbitrary coins into USDC inside the protocol |
| XX-10 | Use multi-collateral or a house stablecoin |
| XX-11 | Build Flutter / RN / a store trading app or a “read-only store package” |
| XX-12 | Use IAP / Play Billing or in-store circumvention copy |

---

## 11. Acceptance for a release

A release is acceptable only if all of the following pass:

1. Every FR-* and CR-* in this SRS has an automated or documented manual test.
2. INV-01–INV-07 run in CI against `crates/math` and program fixtures.
3. FR-DUR-01 drill: kill one ER, Indexer, and PG primary; receipted fills and Vault identity unchanged.
4. FR-SET-03 fixture: two winners, $L>C_{\max}$, same $\rho$, no FIFO remainder to the earlier fill.
5. FR-RES-01: no code path writes $x^*$ except `submit_result`.
6. FR-WAL-03 / CR-08: non-USDC deposit rejected.
7. Client is the Next.js app only; no store binaries in the release.

---

## 12. Traceability (summary)

| SRS group | Product spec | Architecture |
| --- | --- | --- |
| FR-MKT-* | §§4, 11, 16 | Tech §5–6 |
| FR-WAL-* | §14.5, 16 | Tech §2 |
| FR-TRD-* | §§4–5, 7–8, 16 | Tech §6.3 |
| FR-RSK-* | §6, 16 | Tech §6.5 |
| FR-HAL/RES/SET-* | §§8–11, 16 | Tech §6.6–6.7, Sys §3 |
| FR-DUR-* | §16 fill durability | Sys §10 |
| NFR-* | §14.4 | Sys §4, 6 |
| CR-* | §16–17 | Tech §1–2, 5, 10 |
| DR-* | — | Sys §9, 12 |
| XX-* | §17 | — |

---

## 13. Change control

- New behavior requires an SRS ID and a product-spec update in the same change.
- “Should we add X later?” without an SRS ID is not a requirement.
- Stack changes (e.g. leaving Next.js or Anchor) require CR-* amendment and SDK updates in the same change.
