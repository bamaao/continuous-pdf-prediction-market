# Software Requirements Specification

**Continuous PDF Prediction Market**

| Item | Content |
| --- | --- |
| Document | SRS |
| Version | 1.37 |
| Status | Baseline for implementation |
| Audience | Engineers, QA, reviewers |
| Normative sources | `product-specification.md` (product rules), `system-architecture.md`, `technical-architecture.md`, `docs/architecture/ddd-sqlx.md` (read-path persistence) |
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
| R-CREATOR | Market applicant (any SIWS wallet) | Submit a market application (identity + compose spec). SHALL NOT sign `create_*`. The reviewer is the on-chain owner |
| R-REVIEW | System reviewer | Approve / reject / mark-duplicate; lock geo-IP blocks; write the review audit log; **after approve, open trading** (`create_*` + open the risk auction, optional `set_tap`) as board owner. SHALL NOT withdraw Vault |
| R-CMTE | Committee member / authorized reporter | `submit_result`, challenge, vote; attach evidence |
| R-KEEP | Keeper | `close` / Commit / Undelegate, open report window, alerts |
| R-OPS | Operator (read) | Dashboards, never Vault withdraw
| R-PLAT | Platform | Receive $\phi$ fees (claim anytime) and $S_P$; hold the adjustment **pool** $C_P^{\mathrm{pool}}$ and debit $C_P^{\mathrm{alloc}}$ at settlement |

Identity is a Solana pubkey. There is no password account.

The following are locked business rules (same as product §1.1): **after `close_ts`, no prediction-market fills on any family** (UI disables buy / sell; compose refuses; chain `Closed`); trading close also stops the risk auction (`risk_lock_ts \le close_ts`); fees accrue on the prediction market and `claim_fees` pays the protocol `platform`, never $C_P$; $C_P$ is a capped tap drawn only after $R_{\mathrm{net}}+C_R$ still leave a shortfall; any SIWS user MAY apply (off-chain); `close_ts` is absolute and locked at submit; a system reviewer MUST approve and **open trading** (rent); compose `create_*` and the lobby require that approved application; the risk auction opens with trading; duplicates are refused; geo blocks are IP/GeoIP.

---

## 3. Definitions (normative)

| Term | Meaning |
| --- | --- |
| $f(x)$ / $P$ / $p_k$ | Implied PDF from $p0,\theta,\beta$; $\sum p_k=1$. **Not** $E$ |
| $\theta$ | LMSR state; each fill **leg** does $\theta_k\leftarrow\theta_k+q_\ell$ on $k\in S_\ell$ |
| $S$ | Frozen atom set at fill (mask or `skellam_masks`). Settlement SHALL NOT re-integrate a UI interval |
| $i^*(x)$ | Nearest listing node (`crates/math::outcome::interval_index`). Gaussian / lognormal $[a,b]$ SHALL expand to the inclusive node range $\{i^*(a),\ldots,i^*(b)\}$ (product §8.1.2.1). Outside $\Omega$ clamps to $0$ or $n-1$. No partial node |
| $q$ / shares | Ticket size minted at fill. One share’s **face** is $1$ USDC if it hits ($\rho=1$), not the USDC paid to buy it |
| $p_S$ | Current LMSR probability of $S$. Marginal price of an infinitesimal share. **Not** the payout. Cash multiple $\approx 1/p_S$ is display only (product §1.2.2) |
| $P_S(q)$ | $\partial C_S/\partial q\in(0,1)$. Fill price after size $q$. SHALL NOT be used as settlement face |
| face | $q\cdot n_{\mathrm{hit}}/n_{\mathrm{parts}}$ at atom $c=\mathrm{cell}(x^*)$ |
| $C_S(q)$ | USDC cost to **buy** $q$ shares of $S$. For small $q$, $\approx p_S q$. Not the share count |
| $\phi$ | Platform fee rate; fee $=\phi\cdot C_S(q)$ |
| $E(x)$ | Running sum of faces on atom $x$; `grid.exposure` |
| $L_{\max}$ | $\sup_x E(x)$ (display / auction signal only) |
| $L$ | $E(c)$ at $c=\mathrm{cell}(x^*)$ only — not $\sum_k E_k$ |
| $C_M$ | **Removed.** Not a product object. On-chain leftover field is always $0$ |
| Prediction market (预测市场) | The object created after review. Product copy SHALL say **prediction market** / 预测市场. SHALL NOT say 盘, 开盘, 关盘, or “board” as the product noun. On-chain leftover names (`Board`, `board_phase`) are implementation only |
| Open the prediction market (开通预测市场) | Reviewer-signed `create_*` succeeded, the lobby shows the prediction market, traders MAY fill. Not “listing” / “hang the catalog” / 开盘 |
| Open the risk auction (开放风险拍卖) | Risk LPs MAY quote published layers for payout coverage (`risk_open_book`). Not “open the risk book” |
| $C_R^{\mathrm{final}}$ | Locked, drawable Risk LP capacity at settlement; may be $0$ |
| $C_P^{\mathrm{pool}}$ | Single protocol USDC vault for the platform adjustment fund |
| $C_P^{\mathrm{board}}$ | Per-board cap on draws from the pool, locked at listing |
| $C_P^{\mathrm{alloc}}$ | Debit from $C_P^{\mathrm{pool}}$ onto one board at settlement |
| $R_{\mathrm{net}}$ | Trading proceeds; fees never entered; premia deducted |
| $C_{\max}$ | $R_{\mathrm{net}}+C_R^{\mathrm{final}}+C_P^{\mathrm{alloc}}$ |
| $\rho$ | $\min(1,C_{\max}/L)$; one value for every winner |
| `close_ts` | **Every** prediction market. Prediction fills **and** new risk-auction quotes / fills stop when `now ≥ close_ts`. Kickoff / report / early YES SHALL NOT reopen trading |
| `risk_lock_ts` | Auction MAY lock earlier; SHALL NOT be after `close_ts`. Default equals `close_ts` |
| Receipted fill | ER (or L1) executed the ix, returned a signed receipt, and the receipt reached a replicated journal quorum |
| Pending fill | Gateway accepted a signed tx, persisted a receipt, and has not yet confirmed it. Not a fill. |
| Session | Time-limited, amount-limited delegated signer for `buy_set` / `sell_set` only |
| USDC | Circle official SPL USDC mint on the target cluster |

---

## 4. Functional requirements

### 4.1 Markets and priors

| ID | Requirement | Verify |
| --- | --- | --- |
| FR-MKT-01 | The system SHALL create a market only with a locked distribution family, that family's **prior parameters** (FR-MKT-13–22, §4.1.1), $\beta$, grid or atoms, committee (or public committee id), `close_ts`, `risk_lock_ts`, and a resolution rule. | Create ix rejects missing fields |
| FR-MKT-02 | The system SHALL NOT change the distribution family after creation. | Trade / admin ix cannot rewrite family |
| FR-MKT-03 | At creation $\theta=0$. Displayed prices SHALL equal the prior $f_0$ / $P_0$. | Quote vs stored `p0_mass` |
| FR-MKT-04 | $f_0$ / $P_0$ SHALL be written once and SHALL NOT be rewritten by oracles or operators. | No ix mutates `p0_mass` after create |
| FR-MKT-05 | Football: one match `score_scope` SHALL be one Skellam board sharing a 2D score PDF. Derived books (1X2, totals, spreads, exact score) SHALL be set projections, not separate $\theta$ stores. | Shared `theta[i][j]`; `buy_skellam_set` |
| FR-MKT-06 | Football SHALL trade pre-match and in-play until `close_ts` (default: that scope’s full-time whistle). Kickoff SHALL NOT stop trading. After `close_ts` there are no fills. | Clock vs `close_ts` only |
| FR-MKT-07 | CPI / macro listings SHALL call `create_gaussian_market`. $x^*$ is the first official print (`FIRST_PRINT`; later revisions SHALL NOT settle). Prior parameters SHALL follow FR-MKT-15–17 and §4.1.1 — not integer cell indices. | Create + resolve + `p0_mass` vs $\mathcal{N}(\mu,\sigma^2)$ on $\Omega$ |
| FR-MKT-08 | Election winner, `TOP_N`, and vote-share SHALL all use `create_dirichlet_market` with `layout` atoms / top-$n$ / simplex. There SHALL NOT be a separate vote-share family. | Three layouts + resolve |
| FR-MKT-09 | Daily price listings SHALL call `create_lognormal_market` and lock `price_rule`. Prior SHALL be $\log X\sim\mathcal{N}(\mu,\sigma^2)$ on $\Omega>0$ (FR-MKT-18). | Create + `price_rule` + resolve |
| FR-MKT-10 | Binary events SHALL use `create_bernoulli_market` (YES/NO). YES MAY finalize early only when the defined event has occurred, not because it “looks likely”. | Early-YES guard |
| FR-MKT-11 | Listing SHALL open that board’s risk-auction book immediately. | Auction book exists after create |
| FR-MKT-12 | On-chain create SHALL be the five family instructions only: `create_skellam_market`, `create_gaussian_market`, `create_lognormal_market`, `create_dirichlet_market`, `create_bernoulli_market`. Listing names are metadata. | IDL whitelist |
| FR-MKT-13 | Prior parameters SHALL be specified in the **outcome unit** of the series (CPI / macro: percentage points or index points; football: expected goals; price: the `price_rule` unit). They SHALL NOT be cell indices $i\in\{0,\ldots,n-1\}$. $\Omega$, $\mu$, $\sigma$, $\lambda$ written as integers-on-the-grid is a defect. | Compose + `IntervalArgs` / `SkellamArgs` vs unit values |
| FR-MKT-14 | Create / compose SHALL encode unit scalars as Q64 thousandths (`milli`): value $v$ is stored as $\mathrm{Q64}(\lfloor 1000v\rceil/1000)$. Example: $\mu=2.4\Rightarrow 2400$, $\sigma=0.35\Rightarrow 350$, $x_{\min}=-2\Rightarrow -2000$, $x_{\max}=12\Rightarrow 12000$, $\lambda_H=1.4\Rightarrow 1400$. Integer-cell mode (`milli=false`) is allowed only for tests that deliberately use $\Omega=[0,n]$. | Byte compare: milli $\mu=2400$ $\neq$ `q_int(2)` |
| FR-MKT-15 | A Gaussian board SHALL write truncated $\mathcal{N}(\mu,\sigma^2)$ on $\Omega=[x_{\min},x_{\max}]$, then renormalize so $\sum_k p_{0,k}=1$. Require $x_{\max}>x_{\min}$ and $\sigma>0$. CPI / macro $n_{\mathrm{grid}}$ SHALL default to $256$ (product §4.8 / §5.1.1). $n=512$ / $1024$ MAY be used only when user intervals are comparable to the default step or $\Omega$ is wide vs $\sigma$. $n<32$ SHALL be rejected or hard-warned because $\sigma$ then collapses onto $1$–$2$ cells. The protocol floor $n=8$ is not a product Gaussian default. Grid $x_i=x_{\min}+i(x_{\max}-x_{\min})/(n-1)$. | `truncated_normal` vs stored `p0_mass` |
| FR-MKT-16 | CPI / macro $\mu$ SHALL be the survey (or model) **median in percentage points**; $\sigma$ SHALL be that survey's **dispersion in percentage points**, not a made-up cell width. The protocol SHALL ship a reference listing: `US_CPI_YOY` / `FIRST_PRINT` / $\Omega=[-2,12]$ / $\mathcal{N}(2.4,0.35)$ / $n=256$. A MoM series SHALL use a tighter $\Omega$ (reference: $[-1,2]$, $\mathcal{N}(0.2,0.15)$). Wide $\sigma$ vs $\Omega$ MAY be used only when there is no survey (approaches uniform after truncation). | Reference preset + `p0_mass` peak near $2.4$ |
| FR-MKT-17 | Mass outside $\Omega$ SHALL be dropped and the remainder renormalized. If $\mu\notin\Omega$, or $\sigma$ is smaller than one grid step, or $\sigma>(x_{\max}-x_{\min})/2$, or $\Omega$ cuts the $\sim 3\sigma$ tails, create MAY still accept, but `GET /v1/prior` and the create desk SHALL emit the matching warning. Trading SHALL NOT rewrite $P_0$ to “fix” a bad prior. | Preview warnings + no `p0` mutate ix |
| FR-MKT-18 | A lognormal board SHALL require $\Omega\subset(0,\infty)$ and interpret $\mu,\sigma$ on $\log x$ ($\log X\sim\mathcal{N}(\mu,\sigma^2)$), then project onto the log grid and renormalize. Reference: $\Omega=[10\,000,250\,000]$, $\mu=\log(\mathrm{spot})$, $\sigma=0.25$ (widen $\sigma$ with time to expiry). $x_{\min}\le 0$ SHALL be rejected. | `truncated_lognormal` + reject $x_{\min}\le 0$ |
| FR-MKT-19 | A Skellam board SHALL write $P_0(i,j)$ from $\lambda_H,\lambda_A>0$ in **goals** (independent Poisson default; Dixon–Coles optional), $k_{\max}=10$, $121$ cells, overflow labeled `10+`. $\lambda$ uses the same milli encoding ($1.4\to 1400$). After truncation the $121$ cells SHALL sum to $1$. | `independent_poisson_2d` + 1X2 projection |
| FR-MKT-20 | Dirichlet SHALL write $\alpha_i>0$; the uninformative default is $\alpha_i=1$ (uniform on the atoms). Bernoulli is the $K=2$ case (default $50/50$). | `dirichlet` / `binary` |
| FR-MKT-21 | Create / compose SHALL reject: $\sigma\le 0$; $x_{\max}\le x_{\min}$; lognormal $x_{\min}\le 0$; $\lambda_H\le 0$ or $\lambda_A\le 0$; $\alpha_i\le 0$; $n$ outside the family bound (Gaussian / lognormal: $8..1024$; Skellam fixed $121$). | Negative compose / ix tests |
| FR-MKT-22 | On-chain `p0_mass[]` SHALL equal `crates/math` `prior::*` on the same Q64 arguments. `GET /v1/prior` SHALL use that same function (no second TypeScript LMSR / PDF). At $\theta=0$, every quote $p_S$ SHALL equal $\sum_{k\in S}p_{0,k}$ (FR-MKT-03). | math crate = API = first quote |

### 4.1.1 Prior parameterization (normative)

Listing a board **is** writing $P_0$. Family, $\Omega$, and $(\mu,\sigma)$ or $(\lambda_H,\lambda_A)$ or $\alpha$ are not decorative form fields: they are the opening book. Product construction stays in product §§4.6–4.10; this section is the acceptance rule.

**Encoding.** All continuous / intensity scalars travel as Q64 thousandths (FR-MKT-14). The web create desk and CLI SHALL collect floats in the series unit and send `milli=true`. Reviewers SHALL treat a Gaussian create with $\mu=4,\sigma=2,n=8,\Omega=[0,8]$ as a test stub, not a CPI listing.

**Gaussian / CPI (product §4.8).**

| Input | Meaning | CPI YoY reference | CPI MoM reference |
| --- | --- | --- | --- |
| `series_id` / `release_id` | Metadata identity | `US_CPI_YOY` / vintage | `US_CPI_MOM` / vintage |
| `unit` | Official series unit | percentage points | percentage points |
| `print_rule` | What $x^*$ is | `FIRST_PRINT` | `FIRST_PRINT` |
| $\Omega=[x_{\min},x_{\max}]$ | Support; tails cut then renormalized | $[-2,12]$ | $[-1,2]$ |
| $n_{\mathrm{grid}}$ | Linear grid on $\Omega$ | $256$ | $256$ |
| $\mu$ | Prior mean | survey median, e.g. $2.4$ | e.g. $0.2$ |
| $\sigma$ | Prior std | survey dispersion, e.g. $0.35$ | e.g. $0.15$ |
| $f_0$ | Written once | truncated $\mathcal{N}(\mu,\sigma^2)$ on $\Omega$ | same |

Worked YoY check (acceptance): `milli` $(-2000,12000,2400,350)$, $n=256$. Peak $x$ SHALL lie near $2.4$. Interval masses on the product templates SHALL be computed from that $P_0$, not from a second model — e.g. $P(X<2)$, $P(2\le X<2.5)$, $P(2.5\le X<3)$, $P(X\ge 3)$. A $32$-cell grid on $[-2,12]$ has step $\approx 0.45>\sigma=0.35$ and SHALL warn (FR-MKT-17); it SHALL NOT be the CPI default.

Do not list “will CPI print above $2.5\%$” as a second market. That is one interval $S$ on this board (product §4.8.2).

**Skellam (product §4.6).** $\lambda_H,\lambda_A$ are pre-match expected goals from a model or the creator, not cell numbers. Default preview projects 1X2 and totals from the same $P_0(i,j)$.

**Lognormal (product §4.10).** $\mu$ is $\log(\mathrm{spot})$ at list (or a stated model), $\sigma$ is log-vol to expiry, $\Omega$ must cover a reasonable tail and stay strictly positive.

**Dirichlet / Bernoulli.** Uninformative $\alpha=1$. A “50/50 binary CPI” SHALL NOT replace a Gaussian CPI board.

**After list.** $\theta=0$ (FR-MKT-03). No oracle, keeper, or committee instruction may rewrite `p0_mass` (FR-MKT-04). A bad prior is a listing error; the fix is a new board, not a silent $P_0$ patch.

### 4.2 Wallet, deposit, session

| ID | Requirement | Verify |
| --- | --- | --- |
| FR-WAL-01 | The client SHALL connect via Wallet Standard (web) or the equivalent in-wallet / TWA path (mobile). Required wallets: Phantom, Solflare, Backpack. | Manual + adapter tests |
| FR-WAL-02 | After connect, the client SHALL run SIWS; the BFF MAY issue a JWT for query / push only. JWT SHALL NOT authorize on-chain spends. | JWT cannot `buy_set` |
| FR-WAL-03 | The user SHALL `vault.deposit` USDC on L1 with the **main wallet** before trading. Unconfirmed deposits SHALL NOT increase ER spendable balance. | Balance after finalized deposit only |
| FR-WAL-04 | Opening, renewing, or revoking a Session SHALL require the main wallet. | Session ix signer |
| FR-WAL-05 | A Session SHALL encode expiry, remaining USDC, allowed instructions (`buy_set` / `sell_set` / `buy_skellam_set` / `sell_skellam_set` only), and an optional market whitelist. | On-chain session account |
| FR-WAL-06 | In-board set buys and sells SHALL be signed by the Session. Withdraw, create, risk `bid`, and resolution SHALL require the main wallet (or KMS for keepers / reporters). | Negative tests |
| FR-WAL-07 | The UI SHALL expose revoke-session separately from disconnect-wallet. Disconnecting SHALL NOT be treated as on-chain revoke. | UX + chain state |
| FR-WAL-08 | The system SHALL NOT store mnemonic phrases. Session secrets SHALL NOT be stored in plaintext `localStorage`. | Review + scanner |
| FR-WAL-09 | Trading Gateway SHALL forward client-signed txs and SHALL NOT accept, log, or persist Session private keys, mnemonics, or keypair JSON. | Scanner + code review |

### 4.3 Trading (LMSR)

| ID | Requirement | Verify |
| --- | --- | --- |
| FR-TRD-01 | `buy_set` / `sell_set` / `buy_skellam_set` / `sell_skellam_set` SHALL execute only on ER while the market is delegated and `now < close_ts`. This is **every family**. After `close_ts` there are no prediction-market fills. Compose SHALL refuse those ops (`403`) when the indexed `close_ts` has elapsed. The chain SHALL reject with `Closed`. L1 is allowed only before Delegate (tests). | Status + clock + compose |
| FR-TRD-02 | On **every** family and line, fill price SHALL be LMSR $p_S$ / $C_S(q)$. A fill of $q$ SHALL mint $q$ shares (ordinary face $1$ USDC each if $c\in S$ and $\rho=1$). $p_S$ SHALL NOT be the payout and SHALL NOT rescale the share count. Quarter AH is the only split-face case (FR-SET-09). Coverage / $\hat\rho$ SHALL be displayed and SHALL NOT be baked into the quote. | CPI / 1X2 / YES: $p_S=0.7$, $q=1$ costs $\approx 0.7$, hit pays $1$ if $\rho=1$ |
| FR-TRD-13 | The product sells interval **probability**. Quote SHALL be $p_S=\sum_{k\in S}p_k$ (continuum: $\int_I f$). $\partial C_S/\partial q$ SHALL be treated as **price**, never as hit payoff. Ordinary hit payoff SHALL be $\lfloor\rho\cdot q\rfloor$, not $q/p_S$. Share size SHALL NOT be defined as $1/p_S$. Linear $q\cdot p_S$ is an approximation; the ledger SHALL charge $C_S(q)$. UI MAY show an implied multiple $\approx 1/p_S$ labeled as return on cash, not as face. Product §1.2.2. | $p_S=0.7$, $q=10$ → pay $\approx 7$, hit $10$ not $10/0.7$; $C'(q)\in(0,1)$ |
| FR-TRD-03 | Buying the same $S$ again SHALL raise $p_S$ and $C_S$ (LMSR). | Monotonicity test |
| FR-TRD-04 | Listing SHALL lock `fee_bps` and `fee_timing`. **At fill** (`fee_timing=0`): each buy charges $\phi\cdot C_S(q)$ onto the platform fee ledger. **At claim** (`fee_timing=1`): the buy charges only $C_S(q)$; when a winner claims, $\phi$ of that payout is taken onto the fee ledger (miss / VOID: $0$). Fees SHALL NOT enter $C_{\max}$, $R_{\mathrm{net}}$, or $C_P^{\mathrm{pool}}$. The market `platform` MAY `claim_fees` at any time. | Vault ledgers |
| FR-TRD-05 | The system SHALL NOT reject a valid order because $L'_{\max}$ exceeds locked $C_R$. | Case $L_{\max}$ huge, balance OK |
| FR-TRD-06 | The system SHALL reject only: insufficient USDC available, illegal set / $q$, market not TRADING, Session unauthorized, or nonce replay. | Negative tests |
| FR-TRD-07 | After a fill the system SHALL update $\theta$, $E$, $L_{\max}$, and broadcast the new **implied PDF** $p_k=\mathrm{implied\_probs}(p0,\theta,\beta)$, not $E$. | Event + indexer |
| FR-TRD-08 | Low coverage SHALL trigger a strong UI warning and SHALL still allow the order. | UI + chain accept |
| FR-TRD-09 | A fill is complete only when the receipt is durable (L1: gateway receipt store ACK’d then confirmed; ER: FR-DUR-01 journal quorum). Until then the client SHALL show `pending` and retry the same `nonce`. Gateway SHALL persist the receipt **before** returning `pending` and SHALL NOT wait for RPC confirm before that ACK. | Submit returns `pending`; same nonce is idempotent; store survives process restart |
| FR-TRD-10 | Quote Engine preview SHALL be read-only and SHALL NOT be the ledger. | Preview ≠ settle |
| FR-TRD-11 | Skellam fills SHALL use one shared $\theta_{ij}$. Typed lines SHALL go through `buy_skellam_set` (expand $S$, then `crates/math::lmsr_update`). Custom unions MAY use `buy_set`. $L_{\max}$ SHALL be $\max_{ij}E_{ij}$. Quarter lines SHALL be two half-fills of $q/2$ on the same book. Programs SHALL NOT implement a second LMSR. | Home buy raises exact 2-1; over + AH stack on intersection; $p_{1}+p_{X}+p_{2}=1$ |
| FR-TRD-12 | Gaussian / lognormal custom intervals and preset bins SHALL map $[a,b]$ (or a one-sided cut) with `interval_index` on **both** ends, then $S=\{i_a,\ldots,i_b\}$ inclusive (log axis for lognormal). Quote, fill, and `outcome_cell` SHALL use that same function. $a,b$ inside one Voronoi band SHALL buy the whole node (singleton $S$). Values outside $\Omega$ SHALL clamp. Fills SHALL NOT integrate $\int_a^b f$ and SHALL NOT mint a fractional share of a node. Empty $S$ SHALL reject. Product §8.1.2.1. | Nodes $6.0{+}0.1k$ to $10$: $[5.5,7.8]\to\{6.0,\ldots,7.8\}$; $[6.12,6.18]\to\{6.1\}$; $x^*=7.85\notin S$ |

### 4.4 Risk auction

| ID | Requirement | Verify |
| --- | --- | --- |
| FR-RSK-01 | Layers SHALL be those published at listing. LPs SHALL NOT invent attachments. | Bid ix checks layer id |
| FR-RSK-02 | Fills SHALL consume the lowest unit premium first within a layer, subject to capacity and concentration $\gamma$. | Matching test |
| FR-RSK-03 | An LP’s locked collateral SHALL be $\ge D_i$. Unlocked promises SHALL NOT count in $C_R$. | Vault vs book |
| FR-RSK-04 | After fill, that LP’s $D_i$ SHALL NOT increase because later traders raise $L_{\max}$. | Immutable $D_i$ |
| FR-RSK-05 | `risk_lock_ts` SHALL be required and SHALL satisfy `risk_lock_ts \le close_ts`. New auction quotes and fills SHALL stop when `now ≥ close_ts` **or** `now ≥ risk_lock_ts`. After trading close the auction SHALL NOT continue. | Clock |
| FR-RSK-06 | Auction SHALL NOT block prediction trading. | Parallel books |
| FR-RSK-07 | Layer payout SHALL be $H_{A,D}(L)=\min((L-A)^+,D)$. | Settlement vector |
| FR-RSK-08 | One collateral lock SHALL underwrite one board only. | PDA / accounting |

### 4.5 Halt, resolution, settlement

| ID | Requirement | Verify |
| --- | --- | --- |
| FR-HAL-01 | At `close_ts` Keeper SHALL stop prediction fills and the risk auction, then Commit / Undelegate $\theta$, $E$, `trades_root` to L1. | Clock + accounts |
| FR-RES-01 | $x^*$ SHALL be written only by committee (or authorized reporter) `submit_result`. No oracle or feed SHALL write $x^*$. | No auto-oracle ix |
| FR-RES-02 | After a proposal, a challenge window SHALL run. No challenge → finalize. Challenge → $M/N$ vote. | State machine |
| FR-RES-03 | Failed vote SHALL extend or `RESOLUTION_FAILED`. Failed finalization SHALL refund users, return LP collateral, and return unused premium. | Refund balances |
| FR-RES-04 | Football SHALL report a score pair; CPI the first official print; election the defined winner / TOP_N set / shares; price the `price_rule` scalar; binary YES or NO. | Type-specific accounts |
| FR-RES-05 | `evidence_hash` SHALL be an opaque digest. The program SHALL NOT parse oracles, price feeds, or sports APIs. | No feed accounts on ixs |
| FR-SET-01 | Settlement SHALL use $L=E(c)$ with $c=\mathrm{cell}(x^*)$, not $L_{\max}$ and not $\sum_k E_k$. | Fixture $E\neq L_{\max}$; one-cell read |
| FR-SET-02 | $C_{\max}$ SHALL equal $R_{\mathrm{net}}+C_R^{\mathrm{final}}+C_P^{\mathrm{alloc}}$. $C_R$ and $C_P^{\mathrm{alloc}}$ MAY each be $0$. | Ledger identity |
| FR-SET-03 | After $x^*$ is final the board SHALL compute **one** recovery rate **before any user payout**: $\rho=\min(1,C_{\max}/L)$ (or $\rho=1$ if $L=0$). If $C_{\max}\ge L$ then $\rho=1$ (full face). If $C_{\max}<L$ then $\rho=C_{\max}/L$ (actual ratio). Every winning ticket SHALL receive $\lfloor\rho\cdot\mathrm{face}\rfloor$. FIFO or entry-order haircuts SHALL NOT be used. VOID / `RESOLUTION_FAILED` SHALL refund `cost_paid` and SHALL NOT write a payout $\rho$. Sequence: FR-SET-11. | All winners same $\rho$; $C_{\max}\ge L\Rightarrow\rho=1$ |
| FR-SET-04 | Surplus $S=\max(R_{\mathrm{net}}-L,0)$ SHALL be paid only if $\rho=1$. If $C_R^{\mathrm{final}}=0$, all $S$ SHALL go to the platform (`pay_surplus_platform`). If $C_R^{\mathrm{final}}>0$, split at listing-locked $\alpha_R$ (`alpha_r_bps`, default $7000$): $S_R=\alpha_R S$ to Risk LPs pro-rata by `profit_share_bps` $\times$ filled (`pay_surplus_lp`), $S_P=(1-\alpha_R)S$ to the platform. If $\rho<1$ then $S=0$ and those claims SHALL reject (`NoSurplus`). Drawing $H$ / $C_P$ is a shortfall stack (FR-SET-07), not surplus. Fees SHALL NOT enter $S$. Product §1.2.1. | Surplus cases + claim ixs |
| FR-SET-05 | Dust from $\lfloor\rho\cdot\mathrm{face}\rfloor$ SHALL go to reserves, not to a preferred user. | Remainder account |
| FR-SET-06 | There SHALL be no `admin_withdraw`. Outflows are: user unused margin, settlement payout, LP draw, $C_P$ allocation, surplus split, platform fee claim, VOID / failed-resolution refunds. | Instruction whitelist |
| FR-SET-07 | There SHALL be one platform adjustment fund pool $C_P^{\mathrm{pool}}$ (one USDC vault). Draw order SHALL be $R_{\mathrm{net}}$, then $C_R$ by leftover shortfall, then $C_P^{\mathrm{alloc}}=\min((L-R_{\mathrm{net}}-C_R)^+,C_P^{\mathrm{board}},C_P^{\mathrm{pool}})$, which SHALL debit the pool. Boards SHALL NOT hold a private $C_P$ balance. The pool SHALL NOT be an unlimited guarantee. | Pool balance + two-board contention |
| FR-SET-08 | If $L\le R_{\mathrm{net}}$, Risk LP $H$ SHALL be $0$ and $C_P$ SHALL NOT be drawn. | Own-funds fixture |
| FR-SET-09 | Ticket face SHALL be $q\cdot n_{\mathrm{hit}}/n_{\mathrm{parts}}$. Ordinary sets: one part. Quarter lines: two parts of $q/2$. Fill and settle SHALL use the same `skellam_masks`. A quarter OR-mask paying $\rho\cdot q$ is forbidden. | Half-win $q/2$; $E(c)=\sum\mathrm{face}$ |
| FR-SET-10 | The displayed market distribution SHALL be $p_k$ from $p0,\theta,\beta$ (`implied_probs`). $E$ SHALL NOT be shown as the PDF. `submit_result` SHALL NOT rewrite $p$ or $E$. | PDF sums to 1; $E\neq p$ fixture |
| FR-SET-11 | Settlement is a **gate**. After $x^*$ is finalized the program SHALL, in order: (1) lock $L=E(c)$ (FR-SET-01); (2) lock $C_{\max}$ (FR-SET-02, FR-SET-07); (3) write $\rho$ (FR-SET-03) on the board; (4) only then accept `vault.payout` / claim. A payout instruction SHALL reject if $\rho$ is not yet written. Live $\hat\rho$ / coverage SHALL NOT be used as this $\rho$. | No payout before written $\rho$; reject ix |

### 4.5.1 Recovery rate (normative)

Payout is not “send face and hope the pot lasts”. One board-wide $\rho$ is computed **first**, then every winner is paid at that rate:

$$
\rho=\min\bigl(1,C_{\max}/L\bigr),\qquad L=E(c),\quad C_{\max}=R_{\mathrm{net}}+C_R^{\mathrm{final}}+C_P^{\mathrm{alloc}}.
$$

- **Enough:** $C_{\max}\ge L$ $\Rightarrow$ $\rho=1$ $\Rightarrow$ $\lfloor\mathrm{face}\rfloor$ USDC.
- **Short:** $C_{\max}<L$ $\Rightarrow$ $\rho=C_{\max}/L$ $\Rightarrow$ $\lfloor\rho\cdot\mathrm{face}\rfloor$ USDC, same $\rho$ for every winner.
- **Empty liability:** $L=0$ $\Rightarrow$ $\rho=1$.
- **Abnormal end:** VOID / `RESOLUTION_FAILED` $\Rightarrow$ no $\rho$; refund `cost_paid`.

Product §1.2.1 / §2 ⑦ / §8.1.8. Code: `crates/math` `recovery_rate`. `submit_result` SHALL NOT write $\rho$; only `begin_settle` after `finalize`.

### 4.5.2 Surplus after full payout (normative)

Surplus is **not** “whatever is left in the vault after claims”. It is computed **in the same `begin_settle` gate** as $\rho$:

$$
S=\max(R_{\mathrm{net}}-L,0)\quad\text{only if }\rho=1;\qquad \rho<1\Rightarrow S=0.
$$

- No filled risk capital: $S_R=0$, $S_P=S$.
- Filled risk capital: $S_R=\alpha_R S$, $S_P=S-S_R$, default $\alpha_R=0.7$. Each LP claim weight is `profit_share_bps` $\times$ filled on that quote.
- Floor dust from $\lfloor\rho\cdot\mathrm{face}\rfloor$ stays in reserves (FR-SET-05), not in $S$.
- $\phi$ remains on the fee ledger until `claim_fees`.

Product §1.2.1 / §8.6. Code: `surplus`, `surplus_parts`, `pay_surplus_lp_inner`, `pay_surplus_platform_inner`.

### 4.6 Durability and recovery

| ID | Requirement | Verify |
| --- | --- | --- |
| FR-DUR-01 | A receipted fill SHALL persist across a single-node crash. $\theta$ SHALL be reconstructible as $\mathrm{Update}(\theta_0,\mathrm{trade}_1,\ldots,\mathrm{trade}_t)$. | Kill ER / indexer / PG, replay |
| FR-DUR-02 | L1 Commit SHALL be a checkpoint, not the definition of “fill exists”. | Fills between commits survive |
| FR-DUR-03 | The fill journal SHALL have at least two durable copies that do not share a process (ER replica log and object-store append log as specified). | Ops checklist |
| FR-DUR-04 | Postgres / Redis SHALL NOT be the ledger. Recovery order: L1 checkpoint → replay journal → rebuild index. Off-chain PG (listings, fill journal, projections) SHALL be written by Rust services through DDD repositories and sqlx transactions (`docs/architecture/ddd-sqlx.md`). The Next.js client SHALL NOT open Postgres. | Runbook drill |
| FR-DUR-05 | Recovery SHALL NOT delete receipted fills to “align” state. Mismatch SHALL halt the board and alert. | Fault injection |
| FR-DUR-06 | ER SHALL Commit on an interval (time or N fills), not only at `close_ts`. | Commit cadence |

### 4.7 Client and ops surfaces

Next.js is the only user client (CR-01). Every capability in §2 that a human performs SHALL have a screen. A route name alone is not a requirement: the page SHALL do the job of that role. Chain rules stay in FR-MKT / FR-WAL / FR-TRD / FR-RSK / FR-RES / FR-SET; this subsection is the **web acceptance list**.

**Keeper split (locked).** R-KEEP write path is the CLI / KMS (FR-CLI-01, FR-UI-28): `close`, Commit, Undelegate, keeper alerts. That *is* the keeper screen. `/ops` is read-only for R-OPS / R-PLAT. The committee desk MAY call `resolve_open`; it SHALL NOT expose keeper halt / Commit buttons.

| Role | Page | Job |
| --- | --- | --- |
| R-TRADER | `/` | Browse / search / page every indexed board by title, tags, family |
| R-TRADER | `/m/[id]` | Trade ticket (FR-UI-32): live implied PDF (FR-UI-39), pre-bet, typed lines, coverage, pending nonce; after close public $x^*$ / $\rho$ (FR-UI-33); **comments after the board is indexed** (FR-UI-42) |
| R-TRADER | `/portfolio` | Cash ticket (FR-UI-35): deposit / withdraw Circle USDC; every fill; settlement tickets (FR-UI-33) |
| R-TRADER | chrome | Connect / SIWS; open / renew / revoke Session; in-app inbox |
| R-LP | `/auctions` | Browse every open risk book |
| R-LP | `/auction/[id]` | Auction ticket (FR-UI-37): identity, layer stack, $H$ if drawn, standing ladder, quote |
| R-LP | `/lp` | Locked $D_i$, expected $H$, premium, surplus / unlock claim |
| R-CREATOR | `/create` | Market **application** (SIWS): human title + catalog tags (FR-UI-36), distribution family, **write $P_0$** (FR-MKT-13–22, §4.1.1) via the prior ticket (FR-UI-34). Submit does **not** open trading (FR-UI-43). Duplicate → `409` (FR-UI-45). SHALL NOT collect comments (FR-UI-42) |
| R-REVIEW | `/review` | Review queue: approve, then **reviewer** signs on-chain create + open; reject, mark duplicate, set geo-IP blocks, audit log (FR-UI-43–45) |
| R-CREATOR | `/tags` | Maintain the tag vocabulary: add a tag; delete only when unused (FR-UI-36) |
| R-CMTE | `/committee`, `/resolve/[id]` | Open window, `submit_result`, evidence, challenge, $M/N$ vote |
| R-OPS | `/ops` | Read-only: index lag, coverage, Vault identity, $C_P$ pool, keeper heartbeat. Never Vault withdraw |
| R-PLAT | `/ops` ($C_P$) | View $C_P^{\mathrm{pool}}$ and per-board caps / allocations. No withdraw |
| R-KEEP | CLI | `close` / Commit / Undelegate / alerts. Not a web write path |

| ID | Requirement | Verify |
| --- | --- | --- |
| FR-UI-01 | Next.js SHALL provide `/`, `/m/[id]`, `/auctions`, `/auction/[id]`, `/portfolio`, `/lp`, `/resolve/[id]`, `/committee`, `/create`, `/review`, `/tags`, `/ops`. | Routes |
| FR-UI-02 | Mobile SHALL be the same site (PWA / in-wallet browser / official TWA). | Same origin / build |
| FR-UI-03 | The client SHALL show PDF (or 11×11 football heat), $p_S$, $C_S(q)$, coverage, $\hat\rho$, and fee separately. | UI review |
| FR-UI-04 | Copy SHALL state that fills are public on-chain. The product SHALL NOT promise on-chain anonymity. | Copy review |
| FR-UI-05 | The public desk (`GET /v1/markets/{id}/info`, lobby, `/m/[id]`) SHALL show traders (distinct owners with $q>0$), stake ($\sum$ `cost_paid`), $L_{\max}$, $C_R$, $C_{\max}$, coverage, and the trading-implied PDF $p_k=\mathrm{implied\_probs}(p0,\theta,\beta)$. $E$ SHALL be labeled as face / liability and SHALL NOT be drawn as the PDF. Highest-risk tape: FR-UI-46. | `/info` + UI review |
| FR-UI-07 | The lobby catalog (`/` and `GET /v1/markets`) SHALL list every **approved / open** indexed board with the **market name** as the primary row heading (never pubkey-only). `PENDING_REVIEW` and `REJECTED` applications SHALL NOT appear as tradable rows. It SHALL also show tags, status, and when posted: trading event, description, `close_ts`. Search on name / event / description / tags / market / family / status. Optional `tag=` filter (legacy `category=` aliases `tag=`). A market MAY match if **any** of its tags equals the filter. Page+limit: default 20, max 100. A board without a posted title SHALL still show a **generic family-default name** (e.g. `Gaussian prediction market`), never a create-form example such as `US CPI YoY — 2026-03`. Full card: FR-UI-38. Geo-IP: FR-UI-44. | `/` + `/v1/markets` review |
| FR-UI-08 | `/committee` SHALL list indexed boards and accept a family-correct $x^*$ (`submit_result` / `resolve_open`). Session keys SHALL be rejected. No oracle write path. This is the **report slice** of FR-UI-13. | Committee UI review |
| FR-UI-09 | `/portfolio` SHALL list the connected wallet’s tickets (`GET /v1/owners/{owner}/positions`): every board the user filled, cost paid, claimed payout, and a prompt when a board has settled / refunded. Search and pagination apply. Filters SHALL include open / won / lost / unclaimed. The same page SHALL show Vault balance, Session remaining, and deposit / withdraw (FR-UI-16). | Portfolio + API review |
| FR-UI-10 | `/m/[id]` SHALL offer `sell_set` / `sell_skellam_set` on inventory the wallet holds, using the same Quote Engine as buy. Session MAY sign; coverage warning still applies (FR-TRD-08, FR-UI-30). | Board sell + quote |
| FR-UI-11 | After a board is SETTLED, REFUND, VOID, or `RESOLUTION_FAILED`, `/portfolio` SHALL let the owner claim (`vault.payout` / refund). The UI SHALL show paid, net vs `cost_paid`, and a persistent prompt until claimed. Session SHALL be rejected. | Claim / refund ix |
| FR-UI-12 | `/create` SHALL let any **SIWS-logged-in** wallet submit a listing **application** by **distribution family** (Skellam / Gaussian / lognormal / Dirichlet / Bernoulli), not by product category. Main wallet only. Required fields: FR-UI-24. Prior parameters are FR-MKT-13–22 (core), not a UI convenience. Submit SHALL NOT open trading. On-chain `create_*` runs only after `R-REVIEW` approve (FR-UI-43). Interaction: FR-UI-31, FR-UI-34. | Application + family + `p0_mass` |
| FR-UI-13 | `/committee` SHALL support the full resolution path: open window, `submit_result`, challenge, $M/N$ vote. Family-correct $x^*$ (FR-RES-04). No oracle write path. Session rejected. Evidence, reporter vs member, and tallies: FR-UI-26. | Committee state machine |
| FR-UI-14 | `/auction/[id]` SHALL let a Risk LP quote a published layer (capacity, premium, profit share) with the main wallet. Session SHALL NOT bid (FR-WAL-06). The published book is FR-UI-22. After-fill LP claims are FR-UI-23. | Auction desk |
| FR-UI-15 | The chrome SHALL let the connected wallet open, renew, and revoke a Session (expiry, remaining USDC, allowed ixs). Revoke SHALL be a distinct control from disconnect. Disconnecting SHALL NOT revoke on-chain (FR-WAL-07). Session ixs: main wallet only. Remaining and expiry SHALL stay visible while a Session is live. | Session chrome + chain state |
| FR-UI-16 | `/portfolio` SHALL expose `vault.deposit` and `vault.withdraw` (main wallet). Unconfirmed deposits SHALL NOT raise spendable balance (FR-WAL-03). Copy SHALL state Circle USDC only; the product SHALL NOT offer an in-protocol swap. Interaction: FR-UI-35. | Deposit / withdraw ix |
| FR-UI-17 | A Skellam board on `/m/[id]` SHALL offer typed lines — 1X2, handicap (integer / half / quarter), totals, BTTS, exact score, and a custom 121-cell mask — each expanded to $S$ then quoted by the same engine (`buy_skellam_set` / `sell_skellam_set` or `buy_set` for a custom union). Quarter AH SHALL preview as two half-fills of $q/2$ (FR-SET-09). Overflow cells SHALL be labeled `10+` (or $k_{\max}^+$). The heat is not a substitute for the templates. | Football board + quote |
| FR-UI-18 | After a gateway ACK the client SHALL show `pending` on that ticket and retry the **same** `nonce` until `confirmed` or a terminal reject (FR-TRD-09). Refresh SHALL NOT mint a new nonce for an in-flight submit. | Pending UX + idempotent nonce |
| FR-UI-19 | `/m/[id]` SHALL show a rules strip: family, `close_ts` countdown, `risk_lock_ts` if the auction is open, resolution rule, `source_url` / `cert_source` when locked, committee roster or public committee id, overflow `10+` on Skellam. Live score, if shown, SHALL be labeled display-only and SHALL NOT rewrite $P_0$, $\theta$, or `close_ts`. | Rules strip review |
| FR-UI-20 | After halt or settlement, `/m/[id]` SHALL disclose the public result: proposed and/or final $x^*$, $\rho$, $L=E(c)$, and $C_{\max}=R_{\mathrm{net}}+C_R^{\mathrm{final}}+C_P^{\mathrm{alloc}}$. VOID and `RESOLUTION_FAILED` SHALL use refund copy, not a fake $0$-$0$. A connected wallet SHALL see whether each of its $S$ hit. Interaction: FR-UI-33. | Settled board review |
| FR-UI-21 | The chrome SHALL provide an in-app inbox (drawer or `/inbox`) for: unclaimed settlement / refund, challenge opened, Session expiry, pending→confirmed. Items are `{kind, market}` from `GET /v1/notify` plus local session alerts. | Inbox |
| FR-UI-22 | `/auctions` SHALL list every open risk book (search + page) with listing title and tags when posted. `/auction/[id]` SHALL show **published** layers only: layer id, attachment $A$, remaining $D$, unit premium, concentration $\gamma$, `risk_lock_ts` countdown when indexed, and standing quotes. LPs SHALL NOT invent a layer (FR-RSK-01). Interaction: FR-UI-37. | Auction catalog + book |
| FR-UI-23 | `/lp` SHALL list this wallet’s risk quotes and locks (`GET /v1/owners/{owner}/risk`): $D_i$, premium, expected $H$, surplus share. After settlement it SHALL let the LP claim premium / surplus / unused collateral with the main wallet. Session SHALL be rejected. | LP book + claim ix |
| FR-UI-24 | `/create` SHALL collect, per family: **listing title**, **catalog tags**, $\beta$, grid or atoms, $C_P^{\mathrm{board}}$, committee roster or public committee id, `close_ts`, `risk_lock_ts` ($\le$ `close_ts`; default equal), resolution rule, published auction layers, topic / tag (on-chain series key, not the human name). Family-specific: Skellam $\lambda_H,\lambda_A,k_{\max}$ and overflow `10+` preview; Gaussian $\Omega=[x_{\min},x_{\max}]$, $\mu,\sigma$ in the series unit (CPI: percentage points, not cell indices); lognormal $\Omega>0$, $\mu$ on $\log x$, $\sigma$, and `price_rule`; Dirichlet `layout` (atoms / top-$n$ / simplex) and $\alpha$; Bernoulli prior and `early_resolve`. A pubkey without a live SIWS session SHALL be rejected. An allow-list of “authorized creators” SHALL NOT be required. Identity: FR-UI-36. Prior: FR-UI-34. Review: FR-UI-43. | Create form + application |
| FR-UI-25 | **Removed.** The product SHALL NOT collect or inject $C_M$. | — |
| FR-UI-26 | `/committee` / `/resolve/[id]` SHALL distinguish authorized reporter (propose only) from committee member (challenge / vote). The desk SHALL accept an evidence object, store it off-chain, and send only `evidence_hash` on-chain (FR-RES-05). It SHALL show the challenge-window countdown and the live $M/N$ tally, including extend and `RESOLUTION_FAILED`. | Evidence + tally UI |
| FR-UI-27 | `/ops` SHALL be read-only: indexer lag, board coverage, Vault token identity (no withdraw control), keeper heartbeat, $C_P^{\mathrm{pool}}$, and per-board $C_P^{\mathrm{board}}$ / $C_P^{\mathrm{alloc}}$ (`GET /v1/ops/status`, `GET /v1/pool`). It SHALL NOT offer `admin_withdraw` or any Vault outflow (FR-SET-06). | Ops dashboard |
| FR-UI-28 | The web app SHALL NOT expose keeper write instructions (`close`, Commit, Undelegate, keeper-only halt). Those SHALL run from the CLI / KMS (FR-CLI-01). `/ops` shows heartbeat only. `resolve_open` stays on the committee desk (FR-UI-13). | Negative: no keeper buttons |
| FR-UI-29 | The shell nav SHALL include Lobby, Portfolio, Committee, Create, Auctions, Ops. Create SHALL be available to any SIWS session (FR-UI-43). `/review` SHALL be shown only to `R-REVIEW`. Ops MAY be hidden unless the connected pubkey is an operator. `/auctions` SHALL stay public (approved books only). | Nav review |
| FR-UI-30 | Low coverage SHALL show a strong warning on **buy and sell** and SHALL still allow submit (FR-TRD-08). The warning SHALL NOT change $p_S$ / $C_S$. | Coverage warning + chain accept |
| FR-UI-31 | `/create` SHALL be an **application ticket**, not a flat admin form. The desk SHALL: (1) pick a distribution family first (cards) — catalog tags are identity, not the family picker; (2) collect listing title + catalog tags (FR-UI-36), optional blocked regions (FR-UI-44), and show them on the live ticket with family, $x^*$ meaning, cells, $\beta$, $C_P$ tap cap, close window, and derived market PDA; (3) on submit persist a `PENDING_REVIEW` application (SIWS required) — it SHALL NOT open trading or the risk auction yet; (4) after accept show “pending review”, not a live market; (5) keep protocol $C_P^{\mathrm{pool}}$ as a **separate** action after the board is **open**. Session SHALL be rejected (FR-WAL-06). Required fields remain FR-UI-24. Duplicate: FR-UI-45. | Create desk review |
| FR-UI-32 | `/m/[id]` SHALL be a **trade ticket** while the prediction market is trading: implied PDF (or 11×11 heat) per FR-UI-39, typed Skellam lines or interval $S$, live $p_S$ / $C_S(q)$ / Pay / hit / miss / net / book EV (FR-UI-06), coverage warning that does not rewrite the quote (FR-UI-30), and `pending` on ACK with the **same** `nonce` until confirmed (FR-UI-18). Session MAY sign buy / sell only while `now < close_ts` and the market is not settled. Refresh SHALL NOT mint a new nonce for an in-flight fill. When `now ≥ close_ts` or after settlement, buy / sell SHALL be **disabled** and `trade()` SHALL refuse. Labels MAY stay visible for review; they SHALL NOT be a write path (FR-UI-20, FR-UI-33). | Trade desk review |
| FR-UI-33 | Settlement SHALL have two surfaces (product §1.2.3). On `/m/[id]`, after halt / SETTLED / VOID / `RESOLUTION_FAILED`, the **public result** card SHALL show proposed and/or final $x^*$ (from the indexed resolution record when present), $\rho$, $L=E(c)$, $C_{\max}=R_{\mathrm{net}}+C_R^{\mathrm{final}}+C_P^{\mathrm{alloc}}$, and refund copy — never a fake $0$-$0$. A connected wallet SHALL see its last $S$ on that market. On `/portfolio`, claimable tickets SHALL render as **settlement tickets**: cost paid, shares, $\lfloor\rho\cdot\mathrm{face}\rfloor$ if $S$ contains $x^*$ else $0$, or refund `cost_paid` on VOID / failed resolution, with Claim / Refund. Copy SHALL state the payout is $q\times\mathrm{hit}\times\rho$, not $p_S$ and not $q/p_S$. Session SHALL be rejected. The prompt SHALL persist until claimed (FR-UI-11). | Settlement desk review |
| FR-UI-34 | `/create` SHALL present a **prior ticket** that implements FR-MKT-13–22 / §4.1.1: collect $\Omega,\mu,\sigma$ or $\lambda$ or $\alpha$ in the series unit; offer the US CPI YoY / MoM reference listings; live-preview $P_0$ via `GET /v1/prior` (bars + interval or 1X2 masses); surface FR-MKT-17 warnings; send `milli=true`. The desk SHALL NOT expose raw cell-index $\mu,\sigma$ as the CPI path. | Prior ticket + `/v1/prior` + milli compose |
| FR-UI-35 | `/portfolio` SHALL present a **cash ticket**, not two raw amount buttons. The desk SHALL: (1) separate Deposit ticket vs Withdraw ticket; (2) show a live confirmed ledger — wallet ATA, vault `available`, `reserved`, free $=\mathrm{available}-\mathrm{reserved}$, Session cap labeled **not a balance**; (3) on deposit move wallet ATA $\to$ vault `available`, show `pending` until confirmed, and SHALL NOT increment spendable before confirm (FR-WAL-03); (4) on withdraw move only **free** unused margin $\to$ wallet ATA and refuse / disable amounts above free (reserved stays locked); (5) copy Circle SPL USDC only, no in-protocol swap, SOL is fees only; (6) main wallet only — Session SHALL be rejected (FR-WAL-06, NFR-17). `GET /v1/owners/{owner}/vault` SHALL project the same L1 `UserVault` + ATA (`source=l1`). The desk SHALL prefer confirmed L1; the API is the same ledger, not a stub. | Cash ticket + `/vault` + ix |
| FR-UI-36 | `/create` SHALL collect a human **listing title** (market name), **one or more catalog tags**, **trading event**, and **description**. Tags are a **maintained English catalog** (e.g. `football` + `epl`, or `football` + `world cup`); a market MAY belong to several tags. `/tags` SHALL list every catalog tag with a usage count, accept `POST /v1/tags` to add, and `DELETE /v1/tags/{name}` only when unused. Attaching a tag on a listing SHALL register it if new. Tags SHALL NOT replace distribution family and SHALL NOT be confused with the on-chain series `tag`. These fields SHALL persist via `POST /v1/listings` (`tags[]`; legacy `category` is accepted as a single tag) into **PostgreSQL** (`listing.tags` + `catalog_tag`, sqlx, DDD `Context` transaction). Memory is a cache only. A process crash or API restart SHALL reload names from PG. Local test uses the machine PostgreSQL (`infra/local-pg.sql`, default `postgres://cpm:cpm@127.0.0.1:5432/cpm`). `LISTINGS_PATH` JSON is a one-time import, not the store. The browser MAY hydrate an empty API from its local cache. They SHALL appear on `/`, `/m/[id]`, `/auctions`, `/auction/[id]`, `/committee`, `/resolve/[id]`, `/portfolio`, `/lp`, and `/ops`. Topic / tag remain the 32-byte on-chain series key and SHALL NOT be presented as the public name. Title, at least one tag, event, and description SHALL be required to submit. The on-chain `Market` account MAY omit these strings (id_hash only). | Identity ticket + listings file |
| FR-UI-37 | `/auction/[id]` SHALL be an **auction ticket**, not a four-stat summary plus a raw form. The desk SHALL: (1) show listing identity (title, tags, family); (2) draw the published **layer stack** with attachment $A$ and thickness $T$; (3) for the selected layer show $H$ if $L\le A$, if $L$ is mid-layer, and if $L\ge A+T$ using $H=\min(D,T,\max(L-A,0))$ on the $D$ being quoted; (4) show the standing ladder (lowest unit premium, then earlier timestamp); (5) keep the quote ticket (capacity $D$, premium, profit share, rank, lock-is-reserve copy). When `now ≥ close_ts` (or `risk_lock_ts` if earlier), the quote ticket SHALL disable **Quote layer** and `bid()` SHALL refuse. Session SHALL be rejected (FR-WAL-06). Published-only remains FR-UI-22 / FR-RSK-01. | Auction desk review |
| FR-UI-38 | Every market surface (`/`, `/m/[id]`, `/auction/[id]`) SHALL expose a **market card**: name, tags, description, trading event, trading close (`close_ts`), abnormal end (VOID / `RESOLUTION_FAILED` refund — never a fake $0$-$0$), close-extension copy (`close_ts` is locked; only report/challenge may extend), committee-report-open time (`close_ts`), current status, committee final $x^*$ when finalized, liability $L=E(c)$ after settle else $L_{\max}$, capital pool $C_{\max}=R_{\mathrm{net}}+C_R+C_P^{\mathrm{alloc}}$, and payable $\min(L,C_{\max})$. The lobby row SHALL at least show the name. | Market card review |
| FR-UI-39 | `/m/[id]` SHALL show the **trading-implied PDF** $p_k=\mathrm{implied\_probs}(p0,\theta,\beta)$ from the indexed projection — **not** computed in the browser, **not** a fill histogram, **not** $E$. Cadence: indexer poll $400\,\mathrm{ms}$ (floor $200\,\mathrm{ms}$); `GET /v1/markets/{id}/ws` ticks $250\,\mathrm{ms}$ and SHALL push on connect and whenever $\theta$ changes (payload: `slot`, $p$ bps, $E$ for hover, coverage / $\hat\rho$ / $L_{\max}$ / $C_{\max}$). The desk SHALL subscribe to that WS; if the socket is down it SHALL poll `GET /v1/markets/{id}/info` at $\le 2\,\mathrm{s}$. After `board_phase\ge 1` the chart SHALL freeze and unsubscribe. Display: bars or Skellam $11\times 11$ heat, height/heat relative to the peak $p_k$, hover in percent, selected mass $p_S$, bin $n>32$ cells, stamp `Live book` / `Polling book` / `Frozen PDF` plus snapshot slot. $E$ MAY appear only in the tooltip labeled face. The quote ticket SHALL refresh when the snapshot slot moves. | PDF desk + WS / poll |
| FR-UI-40 | Live coverage and $\hat\rho$ SHALL be shown as $C_{\max}/L_{\max}$ only when $L_{\max}>0$. If $L_{\max}=0$ the desk SHALL print `—` and SHALL NOT print $100\%$ or a low-coverage warning. Settlement $\rho=\min(1,C_{\max}/L)$ with $L=0\Rightarrow 1$ remains a settlement identity, not a live-book coverage. | Coverage dash when $L_{\max}=0$ |
| FR-UI-41 | `/portfolio` tickets, `/lp` quotes, `/ops` $C_P$ rows, `/resolve/[id]`, committee result pane, and inbox links SHALL show the **market name** (FR-UI-36), not pubkey-only. `GET /v1/owners/{owner}/positions` SHALL include title and the frozen $S$ (`mask` or Skellam kind). Compose `buy_*` and `POST /v1/tickets` SHALL persist $S$ to PostgreSQL `fill_journal` (same DDD transaction port). `TICKETS_PATH` JSON is a one-time import. Claim SHALL use that journal (or recover $n\le 16$ / typed Skellam from `set_hash`) and SHALL NOT require this browser's localStorage. `/lp` SHALL reload after Draw / Premium / Surplus / Unlock. | Ticket identity + fill journal |
| FR-UI-42 | `/m/[id]` SHALL expose a **flat comment thread** only after the board is **approved and indexed** (`market_proj`). `/create` SHALL NOT collect comments. `GET` / `POST /v1/markets/{id}/comments` SHALL 404 if the market is not indexed or not open. Posting SHALL send the connected wallet pubkey as `author` (same grade as `POST /v1/listings`; SIWS is not required) and a body of 1–2000 trimmed characters. Comments SHALL persist in PostgreSQL `market_comment` (sqlx, DDD `Context` transaction). Memory is a cache. Next.js SHALL NOT open Postgres. Comments SHALL NOT be on-chain and SHALL NOT enter $C_{\max}$, $C_P$, or the fee ledger. No nested replies. Page+limit: default 20, max 100, oldest first. Geo-IP: FR-UI-44. | Board comments + `/comments` |
| FR-UI-43 | The product SHALL have a **system review desk** `/review` for role `R-REVIEW`. Any SIWS wallet MAY `POST /v1/listings/applications` (create ticket + compose spec with `close_in`). Status starts `PENDING_REVIEW`. The reviewer SHALL approve, reject, or mark-duplicate, with a written reason stored in `review_log`. **批准并开通预测市场** SHALL then be signed by the **reviewer wallet as on-chain `owner`**: `create_*` + 开放风险拍卖 + optional `set_tap`; attach `market`; set status `OPEN`. If create already landed, **继续开通预测市场** writes the live market into the lobby without a second create. `close_ts` is the absolute time locked at submit. The applicant SHALL NOT sign `create_*`. **Reject** sets `REJECTED` and SHALL NOT open trading. Pending / rejected applications SHALL NOT appear on `/` as tradable markets. The reviewer is not the committee and SHALL NOT withdraw Vault. | `/review` + application API |
| FR-UI-44 | A listing application MAY lock **blocked regions** (ISO 3166-1 alpha-2, optional ISO 3166-2 subdivision). The Market API SHALL resolve the client IP to a region (GeoIP) and SHALL hide or `403` that board on lobby, `info`, `quote`, `preview`, `compose`, trade, auction, and comments when the region is blocked. The reviewer MAY add or confirm the list at review. Next.js SHALL NOT open Postgres; it SHALL call `/v1/*`. Geo-IP is access policy, not $\rho$ / $C_P$. | Geo-IP + `blocked_regions` |
| FR-UI-45 | Submit SHALL compute a duplicate key = family + normalized title + trading event (trim; ASCII case-fold). If another application is `PENDING_REVIEW` or `OPEN` with the same key, the API SHALL return `409`. The reviewer MAY reject as duplicate. On-chain `topic`/`tag` uniqueness remains a separate series-key check. | Duplicate listing |
| FR-UI-46 | During trading the system SHALL publish a **highest-risk-payout** object per prediction market: $L_{\max}=\max_k E_k$ and the **thickest overlap** $\{x:E(x)=L_{\max}\}$ (contiguous plateau), labeled in outcome units (Gaussian / lognormal: print interval on $\Omega$; Skellam: score cell; Bernoulli: YES/NO; Dirichlet: atom). $E(x)$ is overlap depth of bought intervals, not ticket-volume rank. Info / list / WS SHALL include `peak_risk` (`lo`/`hi` = plateau). The lobby and `/ops` SHALL list every live market’s peak on a $2\,\mathrm{s}$ poll. Display SHALL NOT sum $E$ across a bought band or treat the PDF peak as the risk peak. After `board_phase≥1` the public number is $L=E(c)$. | Tape + `/info.peak_risk` |
| FR-CLI-01 | CLI SHALL support create-*, close/undelegate, buy-set, session open/renew/revoke, withdraw, risk bid, resolve, keeper, index status, `market pdf` / `market info`. Gateway buys SHALL poll `pending` then `confirmed` on the same `nonce`. Keeper writes in this list are the R-KEEP screen (FR-UI-28). | Command list |
| FR-IDX-01 | Indexer SHALL follow ER + L1 and rebuild from the journal after lag. Reads SHALL be shed if lag exceeds the threshold. | Lag metric |

### 4.7.1 Interaction desks (normative)

A route that “has the buttons” is not enough. Each human write path SHALL present a **ticket**: what is locked now, what happens if the event hits, what the user signs, and which key may sign. Auction (FR-UI-22) and committee (FR-UI-26) already follow this. Create / trade / settle SHALL match.

**Create (`/create`, FR-UI-31, FR-UI-34, FR-UI-36).** Family cards first — family is the math, not the catalog. The **identity ticket** is the public name: listing title + catalog tags (several allowed — e.g. `football` + `epl`). `/tags` maintains the English vocabulary; unused tags MAY be deleted. Topic / tag stay the on-chain series key. Writing $P_0$ is the listing (FR-MKT-13–22, §4.1.1). The **prior ticket** is that write: for CPI, $\Omega$ and $\mathcal{N}(\mu,\sigma^2)$ in percentage points — survey median $\to\mu$, survey dispersion $\to\sigma$, $n=256$ — with a live $P_0$ preview and interval masses; not two raw cell integers. The application ticket then previews title, tags, $x^*$ meaning, $P_0$ summary, cells, $\beta$, $C_P$ tap, close, derived PDA. Submit is one primary button per family (`Create {family} prediction market`). Submit persists a `PENDING_REVIEW` application (`POST /v1/listings/applications` + compose spec), not a live market. Duplicate key → `409` (FR-UI-45). The applicant SHALL NOT sign `create_*`. After **reviewer approve** (FR-UI-43) the **reviewer wallet** is owner: persist title/tags (`POST /v1/listings`), **open trading** and **open the risk auction**, attach `market`, set `OPEN`. The create desk then shows the name and market key and deep-links to the trade desk, the risk auction, and the committee desk. Funding $C_P^{\mathrm{pool}}$ stays secondary — it is not the application. The create desk SHALL NOT collect comments; discussion starts on `/m/[id]` after the market is live (FR-UI-42).

**Review (`/review`, FR-UI-43–45).** `R-REVIEW` only. Queue of `PENDING_REVIEW` applications: identity, prior, compose spec, blocked regions, duplicate hits. Actions: **批准并开通预测市场** (reviewer signs `create_*` as owner — 大厅可见、交易者可买, and 开放风险拍卖), 拒绝 (reason), 标为重复. If chain fails after off-chain approve, retry **继续开通预测市场**. Every action writes `review_log`. Not committee $x^*$, not `/ops` withdraw.

**Trade (`/m/[id]`, FR-UI-32, FR-UI-39, FR-UI-40, FR-UI-42).** Left: implied PDF (or Skellam heat) and typed lines. The chart is a **live book snapshot**: server `implied_probs`, WS on $\theta$ change, $2\,\mathrm{s}$ poll fallback, freeze after close — never a browser LMSR loop. Stamp slot and live/poll/frozen. Hover is percent; $E$ is face in the tooltip. Right: the pre-bet ticket (Pay, fee, hit / miss, net, book EV), refreshed when the snapshot slot moves. Coverage / $\hat\rho$ are `—` while $L_{\max}=0$. Low coverage is a banner that still allows submit **while trading is open**. After `close_ts` the ticket reads **Trading closed** and buy / sell are disabled on every family. After gateway ACK the ticket reads `pending` and retries the same nonce. Session may sign only before close. **Comments** sit below the trade ticket: only after `market_proj` has the prediction market; connected wallet pubkey as `author`; `GET`/`POST /v1/markets/{id}/comments`; Postgres `market_comment`; 404 if not indexed; 1–2000 trimmed characters; flat, no replies; never $C_{\max}$ / $C_P$ / fees.

**Settle (`/m/[id]` public result + `/portfolio` settlement tickets, FR-UI-33, FR-UI-41).** After close the market shows $x^*$, $\rho$, $L$, $C_{\max}$. VOID / `RESOLUTION_FAILED` use refund copy. `/portfolio` lifts unclaimed tickets into settlement tickets (name, cost, shares, $\lfloor\rho\cdot\mathrm{face}\rfloor$ or refund) with Claim / Refund. Frozen $S$ comes from the fill journal (`TICKETS_PATH`), not this browser's localStorage.

**Cash (`/portfolio` cash ticket, FR-UI-35).** Deposit and withdraw are separate tickets on the same ledger. Deposit: wallet ATA $\to$ vault `available`; the ticket reads `pending` and spendable does not move until confirm. Withdraw: only free $=\mathrm{available}-\mathrm{reserved}$ may leave; reserved is risk-lock, not unused margin. Circle SPL USDC only; no swap; Session cannot sign. The Session remaining figure is a cap, not a second vault. `GET /v1/owners/{owner}/vault` reads L1, not a zero stub.

**Market card (FR-UI-38, FR-UI-46).** Name, tags, description, trading event, `close_ts`, abnormal end, report-open, status, final $x^*$, $L$, $C_{\max}$, payable, and during trading the **highest risk payout** (thickest overlap interval + $L_{\max}$). The lobby lists the name as the row heading and a live tape of those peaks.

**Auction (`/auction/[id]`, FR-UI-22, FR-UI-37).** Identity first (title, tags). Then the published layer stack ($A$, $T$), $H$ if this layer is drawn at three $L$ marks, the standing ladder, and the quote ticket ($D$, premium, rank). Four headline numbers alone are not the desk.

Auction and committee desks already specified: published-layer quote ticket (FR-UI-22) and phase-gated report / challenge / $M/N$ (FR-UI-26).

---

## 5. External interface requirements

| ID | Interface | Requirement |
| --- | --- | --- |
| IR-01 | Solana L1 | Vault, create, Delegate/Commit, resolution, settle; dedicated RPC, not a public free endpoint for users |
| IR-02 | MagicBlock ER | `buy_set` / `sell_set` / optional risk fills; `<10` ms in-chain target |
| IR-03 | Wallet Adapter | Web: `@solana/wallet-adapter-react`. Mobile: deep link / injected provider / MWA on TWA |
| IR-04 | USDC mint | Circle official SPL USDC; `mint == USDC_MINT` on every funds ix |
| IR-05 | — | No on-chain oracle or price-feed dependency |
| IR-06 | Object store | Committee evidence and append-only fill journal; L1 stores hashes / `trades_root` |
| IR-07 | BFF / Market API | Metadata, positions, snapshots, `GET /v1/markets` catalog (search + page), `GET /v1/owners/{owner}/positions` tickets, `GET /v1/owners/{owner}/vault` cash ledger, `GET /v1/owners/{owner}/risk` LP book, `GET /v1/markets/{id}/info` desk, `GET /v1/markets/{id}/pdf` cells, `GET /v1/markets/{id}/ws` implied-PDF push (IR-08), `GET /v1/markets/{id}/preview` pre-bet ticket, `GET /v1/markets/{id}/layers` published auction book, `GET /v1/markets/{id}/resolution` committee record, `GET`/`POST /v1/markets/{id}/comments` board comments (FR-UI-42; 404 if not indexed), `GET /v1/auctions` auction catalog, `POST /v1/listings/applications` + `GET`/`POST /v1/review` listing review (FR-UI-43–45; SIWS), `POST /v1/listings` + `GET /v1/listings/{id}` listing identity (after approve), `GET/POST /v1/tags` + `DELETE /v1/tags/{name}` catalog tags, `POST /v1/tickets` fill journal, `GET /v1/ops/status` ops snapshot, `GET /v1/pool` $C_P$ pool, `GET /v1/prior` create-time $P_0$ preview; no hot-path fill |
| IR-08 | Quote WSS | PDF / book push. Localnet floor is the indexer poll ($400\,\mathrm{ms}$, min $200\,\mathrm{ms}$) plus a $250\,\mathrm{ms}$ $\theta$-change tick. ER-era emit after a projected fill remains $<50\,\mathrm{ms}$ (NFR-04). |
| IR-09 | Trading Gateway | Client-signed txs; ACK is `pending` (NFR-13). ER execution target remains NFR-01. Gateway is not a signer. |
| IR-10 | IDL | One Anchor IDL for `packages/sdk` and `crates/client`; no hand-rolled discriminators |

---

## 6. Non-functional requirements

Reliability (crash / replay), security (keys, session authority, payload bounds), and performance (ACK vs confirm) SHALL be treated as first-class requirements. NFR-* below are MUST, same as FR-*.

| ID | Requirement | Target |
| --- | --- | --- |
| NFR-01 | ER `buy_set` execution | $<10$ ms (excludes wallet popup) |
| NFR-02 | ER gas | 0 |
| NFR-03 | Quote / position API | $<150$ ms |
| NFR-04 | PDF WebSocket | Emit after a projected $\theta$ change $<50\,\mathrm{ms}$. Localnet indexer lag ($400\,\mathrm{ms}$) is a separate floor (FR-UI-39). |
| NFR-05 | Halt Commit / Undelegate | seconds |
| NFR-06 | Grid size | $N=256\sim1024$ (1D); football $11\times11$. Solana `create_account` / one `resize` $\le10\,240$ bytes. `Grid::space(n)=75+64n` (Borsh, no pad): $n=128\to8267$ (one create), $n=256\to16459$ (create + 1 grow), $n=1024\to65611$ (create + 6 grows). Create writes $P_0$ only when `space\le10240`; else empty vecs, then `grow_grid` + `write_grid_mass` (`PRIOR_CHUNK=256`) + `seal_grid`. Families that MAY exceed the cap: Gaussian, lognormal, and Dirichlet **simplex with $k\le4$** (e.g. $k=2$ bins=$158$ $n=159$; $k=4$ bins=$10$ $n=286$). Dirichlet **atoms** over the cap SHALL reject (`extra.a–d` hold at most 4 $\alpha$). Simplex masses are streamed (`simplex_chunk_raw`); do not rebuild the full simplex on the BPF heap. Interval $P_0$ is the same truncated $\mathcal{N}$: range-reduced `exp` and a $3\sigma$ index window. `grow_grid` only resizes. 继续开通预测市场 / CLI **always** finish from on-chain `Market.n` until `p0.len()=n`, `z\neq0`, and `data_len\ge space`. Grow / mass / seal / fill txs request 256 KiB heap. Fill SHALL return `GridNotReady` until sealed. LMSR fills stay on-chain (same `exp`); quote is already off-chain. |
| NFR-07 | Consensus math | Q64.64; no IEEE float; `crates/math` shared by chain, Quote, WASM |
| NFR-08 | Index rebuild | From journal / chain; RPO for PG query copy $\le1$ min (not ledger) |
| NFR-09 | Monthly drill | Kill Indexer, Keeper, PG primary, one ER; Vault identity and receipted fills unchanged |
| NFR-10 | Observability | ER latency, fill success, index lag, coverage, Vault balance, Keeper heartbeat |
| NFR-11 | Availability (Keeper) | Active-standby; idempotent `close` |
| NFR-12 | Precision | Same test vectors on chain, Quote, WASM |
| NFR-13 | Gateway submit ACK | Return `pending` without waiting for RPC confirm. Target $<50$ ms after the signed tx is accepted (local disk write included; not ER fill time) |
| NFR-14 | Gateway receipt durability | A receipt the gateway already ACK’d as `pending` SHALL survive a gateway process crash. The store is **not** the ledger (FR-DUR-04). RPO $=0$ for ACK’d receipts on that host |
| NFR-15 | Gateway / Session secrets | The system SHALL NOT persist or log private keys, mnemonics, or Session secret material. A signed transaction MAY be stored only to re-forward the same `nonce`. Logs SHALL NOT print full `tx_b64` |
| NFR-16 | Gateway idempotency | The same `(owner, market, nonce)` SHALL return the existing receipt. After `confirmed`, a new signed tx for that nonce SHALL NOT be forwarded again |
| NFR-17 | Session is not a vault | `remaining_usdc` is a Session cap. The L1 user vault debit is authoritative. Session SHALL NOT authorize `withdraw`, `create_*`, risk `bid`, or resolution |
| NFR-18 | Gateway rate limit | Submit SHALL be rate-limited per owner (default $20$ / $10$ s). Excess SHALL return HTTP $429$ |
| NFR-19 | Gateway payload bound | Signed tx body SHALL be rejected above $4$ KiB. JSON body limit $16$ KiB |
| NFR-20 | Confirm path vs hot path | RPC send / confirm SHALL run off the submit ACK path (background). Confirm latency SHALL NOT block the `pending` response |

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
| CR-11 | TEE, if used, SHALL attest ER integrity only. TEE SHALL NOT replace the committee or L1 Vault. |
| CR-12 | PDF / $E$ / $L_{\max}$ SHALL remain public. Private ER encryption SHALL NOT be used for the main book. |

---

## 8. Data and security requirements

| ID | Requirement |
| --- | --- |
| DR-01 | Public book ($f$, fills, $E$, $x^*$, $\rho$, payouts) SHALL be public and recomputable. |
| DR-02 | Secrets (upgrade keys, keeper keys, KMS, DB, RPC tokens) SHALL use least privilege and KMS/HSM. |
| DR-03 | PII SHALL be encrypted, off-chain, and not written to L1. |
| DR-04 | TLS in transit; KMS-backed encryption at rest for PG, object store, backups. |
| DR-05 | Evidence objects: private bucket, L1 hash only. |
| DR-06 | Logs MAY include truncated pubkey; SHALL NOT include phone, government id, or raw IP by default. |
| DR-07 | A stolen index DB SHALL NOT enable an extra USDC transfer. |
| DR-08 | Gateway receipt files SHALL NOT contain private keys or mnemonics. Signed tx bytes, if stored, are not spend authority. |
| DR-09 | Gateway receipt directory SHALL be host-local and not world-readable by default. |

---

## 9. Protocol invariants (test in CI)

| ID | Invariant |
| --- | --- |
| INV-01 | $\sum_k p_k=1$ (or $\int f=1$) after every fill |
| INV-02 | Buying $S$ does not decrease $p_S$ |
| INV-03 | If $\rho=1$, winners are paid ticket face; else $\sum$ paid $=\lfloor\rho\cdot$ faces$\rfloor$ with dust to reserves |
| INV-04 | $\sum$ USDC paid to winners $\le C_{\max}$ |
| INV-05 | Vault token balance equals the accounting sum |
| INV-06 | Same $\rho$ for every winner on a board |
| INV-07 | Fees never sit in the user payout numerator |
| INV-08 | After any mix of fills, $\sum_j\mathrm{face}_j(c)=E(c)$ |
| INV-09 | $p_k$ is not $E_k$; $\sum p_k=1$ |
| INV-10 | Session is not vault authority: `withdraw` / `create_*` / `bid` / `submit_result` fail when signed only by Session |

---

## 10. SHALL NOT (out of scope)

The system SHALL NOT:

| ID | Forbidden |
| --- | --- |
| XX-01 | Bake expected $\rho$ into LMSR price |
| XX-13 | Treat $E$ as the PDF; sum $E$ across atoms as $L$; pay $\rho\cdot q$ on a quarter OR-mask |
| XX-02 | Haircut by arrival time / FIFO |
| XX-03 | Hard-reject trading because $L_{\max}$ exceeds locked $C_R$ fails |
| XX-04 | Let one LP lock underwrite multiple boards |
| XX-05 | Use a parameterized AMM (only $\mu,\sigma$ or $\lambda_H,\lambda_A$) |
| XX-06 | Run a full on-chain CVaR portfolio optimizer |
| XX-07 | Ship committee token governance or a large on-chain court |
| XX-08 | Accept SOL or other SPL tokens as margin / collateral / payout |
| XX-09 | Auto-swap arbitrary coins into USDC inside the protocol |
| XX-10 | Use multi-collateral or a house stablecoin |
| XX-11 | Build Flutter / RN / a store trading app or a “read-only store package” |
| XX-12 | Use IAP / Play Billing or in-store circumvention copy |
| XX-14 | Persist or log Session / user private keys, mnemonics, or keypair JSON in the gateway, BFF, or indexer |

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
8. FR-TRD-09 / NFR-13–16: gateway submit returns `pending` after a durable write; that receipt survives a gateway process restart; same `(owner, market, nonce)` is idempotent; no private keys in receipt files.

---

## 12. Traceability (summary)

| SRS group | Product spec | Architecture |
| --- | --- | --- |
| FR-MKT-* | §§4, 4.6–4.10 (priors), 11, 16; SRS §4.1.1 | Tech §5–6, §6.2 $P_0$ |
| FR-WAL-* | §14.5, 16 | Tech §2 |
| FR-TRD-* | §§4–5, 7–8, 16 | Tech §6.3 |
| FR-RSK-* | §6, 16 | Tech §6.5 |
| FR-HAL/RES/SET-* | §§8–11, 16 | Tech §6.6–6.7, Sys §3 |
| FR-DUR-* | §16 fill durability | Sys §10 |
| FR-UI-* | §3 roles, §4.6 football templates, §8.1 desk, §14.5–14.6 client | Tech §2; Sys §2 |
| NFR-* | §14.4 | Sys §4, 6 |
| CR-* | §16–17 | Tech §1–2, 5, 10 |
| DR-* | — | Sys §9, 12 |
| XX-* | §17 | — |

---

## 13. Change control

- New behavior requires an SRS ID and a product-spec update in the same change.
- “Should we add X later?” without an SRS ID is not a requirement.
- Stack changes (e.g. leaving Next.js or Anchor) require CR-* amendment and SDK updates in the same change.

**v1.4:** Gateway `pending`-first durable receipts; NFR-13–20 (reliability, secrets, rate limit, ACK vs confirm); DR-08–09; FR-TRD-09 / FR-WAL-09 / FR-CLI-01 tightened; XX-14.

**v1.5:** FR-UI-05 public desk — traders, stake, $L_{\max}$, $C_R$, implied PDF on `GET /v1/markets/{id}/info` and `/m/[id]`.

**v1.6:** FR-UI-06 pre-bet ticket — Pay, fee, hit / miss cashflows, net if hit, book EV on `GET /v1/markets/{id}/preview` and `/m/[id]`.

**v1.7:** FR-UI-07 lobby catalog — search by market / family / status and paginated `GET /v1/markets`.

**v1.8:** FR-UI-08 `/committee` desk — list boards, family-correct $x^*$, `submit_result` / `resolve_open`, no Session / oracle.

**v1.9:** FR-UI-09 portfolio tickets — all boards a wallet filled, claimed PnL, settle/refund prompts.

**v1.10:** §4.7 is the web acceptance list. Role→page matrix. FR-UI-01 adds `/create`. FR-UI-10–14: sell, claim/refund, create-by-family, committee challenge/vote, LP auction desk. v1.0–1.4 only named routes (FR-UI-01–04); v1.5–1.9 backfilled pages after they were requested.

**v1.11:** Completes the web acceptance list against §2. Role→page adds `/auctions`, `/lp`, `/ops`, chrome Session / inbox. FR-UI-01 routes updated. FR-UI-09/11/12/13/14 tightened. FR-UI-15–30: Session chrome, vault cash, Skellam typed lines, pending nonce, rules strip, public $x^*$/$\rho$, inbox, auction catalog + published book, LP claims, create field list, inject $C_M$, committee evidence/tally, `/ops` + $C_P$, keeper writes CLI-only, shell nav, coverage warning on buy. IR-07 adds `/auctions`, `/layers`, `/risk`, `/ops/status`, `/pool`. Keeper split: CLI is the R-KEEP screen; web has no halt / Commit buttons.

**v1.12:** Interaction desks. §4.7.1 is the ticket rule for every human write path. FR-UI-31 listing ticket on `/create` (family cards, live capital ticket, post-list links). FR-UI-32 trade ticket on `/m/[id]` (pre-bet + pending nonce + coverage banner). FR-UI-33 settlement tickets: public $x^*$/$\rho$/$L$/$C_{\max}$ on the board, claim/refund tickets on `/portfolio`. IR-07 adds `GET /v1/markets/{id}/resolution`. Auction and committee desks stay FR-UI-22 / FR-UI-26.

**v1.13:** Prior ticket (FR-UI-34). `/create` takes Gaussian $\Omega,\mu,\sigma$ in the series unit (CPI: percentage points; preset $\mathcal{N}(2.4,0.35)$ on $[-2,12]$), live $P_0$ via `GET /v1/prior`, interval masses, and tail / grid warnings. Compose writes Q64 thousandths (`milli`). FR-UI-24 and §4.7.1 updated. IR-07 adds `/v1/prior`.

**v1.14:** Prior parameterization is a **market** rule, not only a desk. §4.1.1 is normative. FR-MKT-13–22: outcome-unit scalars (not cell indices), Q64 thousandths, truncated Gaussian on $\Omega$ with CPI $\mu$=survey median / $\sigma$=dispersion, reference `US_CPI_YOY` $\mathcal{N}(2.4,0.35)$ on $[-2,12]$ $n=256$, lognormal $\Omega>0$ / $\mu$ on $\log x$, Skellam $\lambda$ in goals, Dirichlet/Bernoulli $\alpha$, reject illegal params, `p0_mass` equals `crates/math`. FR-MKT-01/07/09 and FR-UI-12/34 point here. Product §§4.6–4.10 remain the construction source.

**v1.15:** Cash ticket (FR-UI-35). `/portfolio` deposit / withdraw is a ledger ticket: Deposit vs Withdraw, confirmed `available` / `reserved` / free, Session cap is not a balance, `pending` until confirm (FR-WAL-03), Circle USDC only, Session rejected. FR-UI-16 and §4.7.1 updated. IR-07 adds `GET /v1/owners/{owner}/vault`.

**v1.16:** Listing identity (FR-UI-36) and auction ticket (FR-UI-37). `/create` requires a human title and catalog category, persisted `POST /v1/listings`, shown on lobby / board / auctions. `/auction/[id]` is a layer-stack ticket with $H$ if drawn and a standing ladder — not four stats plus a raw form. FR-UI-07/22/24/31 and §4.7.1 updated. IR-07 adds `/v1/listings`.

**v1.17:** Market card (FR-UI-38). Listings require name, category, trading event, description. Lobby row heading is the market name (never pubkey-only). `/m/[id]` and `/auction/[id]` show close, abnormal refund, report-open, status, final $x^*$, $L$, $C_{\max}$, payable. `close_ts` does not move; only report/challenge may extend. FR-UI-07/36 updated.

**v1.18:** Settlement gate. FR-SET-03 states the two $\rho$ cases ($C_{\max}\ge L\Rightarrow 1$, else $C_{\max}/L$). FR-SET-11 forbids `vault.payout` until $\rho$ is written. §4.5.1 is normative. Product §2 ⑦ / §8.1.8 match. VOID / failed resolution still refund, not a fake $\rho$.

**v1.19:** Implied-PDF desk (FR-UI-39). `/m/[id]` shows $p_k=\mathrm{implied\_probs}$ from the indexed book — not a browser LMSR loop. Cadence: indexer $400\,\mathrm{ms}$, WS on connect + $\theta$ change ($250\,\mathrm{ms}$ tick), $2\,\mathrm{s}$ poll fallback, freeze after close. Display: percent hover, selected $p_S$, bin $n>32$, live/poll/frozen + slot. IR-07/08 and NFR-04 state the localnet floor vs ER emit. Product §8.1.7 / §14.6 match.

**v1.20:** Listings file (`LISTINGS_PATH`) so names survive API restart (FR-UI-36). `GET /v1/owners/{owner}/vault` reads L1 (FR-UI-35). Live coverage is `—` while $L_{\max}=0$ (FR-UI-40) — not a fake $100\%$.

**v1.21:** Secondary lists show market names (FR-UI-41). Fill journal (`TICKETS_PATH`) + compose/`POST /v1/tickets` so claim does not need this browser's mask. `/lp` reloads after a write. Untitled boards use a generic family name, never the create CPI example (FR-UI-07). Public result prints written ρ, not Q64 raw.

**v1.22:** Board comments (FR-UI-42). Discussion is on `/m/[id]` only after the board is indexed. `/create` SHALL NOT collect comments. Wallet pubkey as `author` (listings grade; no SIWS). Postgres `market_comment`. `GET`/`POST /v1/markets/{id}/comments` 404 if unknown. Flat thread; not on-chain; not $C_{\max}$ / $C_P$ / fees. IR-07 and §4.7.1 updated. Product §1.1 locks the same rule.

**v1.23:** Listing review (FR-UI-43–45). Any SIWS wallet may apply; a system reviewer (`R-REVIEW`, `/review`) must approve before the board opens. Duplicates `409`. Geo-IP region blocks. Lobby lists only `OPEN` boards. On-chain create runs on approve, not on first submit. Product §1.1 / §4.6.1 / §14.6 updated.

**v1.24:** After approve, the **reviewer** (not the applicant) is the on-chain owner and signs `create_*` / `fund_cm` / `risk_open_book`. The application stores a compose spec (`close_in`); `close_ts` is set at open so the queue does not burn the clock. `/create` is submit-only. `/review` Approve and open. Status `OPEN` once `market` is attached. Product §1.1 / §1.2 updated.

**v1.25:** $C_M$ / seed reserve / inject removed from the product. $C_{\max}=R_{\mathrm{net}}+C_R^{\mathrm{final}}+C_P^{\mathrm{alloc}}$. Risk layers attach at $(k-1)D_{\mathrm{unit}}$ (no retained seed layer). Create and `/m/[id]` no longer collect or inject creator capital. `fund_cm` only opens the board vault at amount $0$. Product §1.1 / §1.2 / §6.6 / §9 updated.

**v1.26:** Product copy: **open the prediction market** (开通预测市场) and **open the risk auction** (开放风险拍卖). Do not say 盘 / 开盘 / 关盘 / “board” as the product noun, or “listing” / “open the risk book” / “hang the catalog” for those steps. `/review` buttons: 批准并开通预测市场 / 继续开通预测市场. Lobby: Live markets. `/auctions`: Risk auctions. Product §1.1 / §1.2 / §4.6.1 / §14.6 updated.

**v1.27:** After `close_ts`, **no prediction-market fills on any family**. `/m/[id]` disables buy / sell; compose refuses `buy_*` / `sell_*` (`403`); chain `Closed`. Bernoulli `close_ts` is the trading deadline (not “slightly after for verification”). Kickoff / report / early YES do not reopen trading. Product §1.1 / §4.11 / §16. FR-TRD-01 / FR-UI-32.

**v1.28:** Highest risk payout during trading is the **thickest overlap** $\{x:E(x)=L_{\max}\}$ of bought intervals, labeled on $\Omega$ (Gaussian print plateau, not ticket-volume rank). Live tape on `/` and `/ops`. FR-UI-46. Product §4.5.

**v1.29:** Gaussian / lognormal $n=256$ is the create default. Grid larger than 10 240 bytes is created at the Solana cap then finished with `grow_grid` (NFR-06). Trading stays blocked until $P_0$ is written.

**v1.30:** `Grid::space` equals Anchor `try_serialize` length. **继续开通预测市场** retries `grow_grid` against chain `Market.n` / `p0.len` (a create that landed with a failed grow is no longer left untradeable). Grow / create txs set `request_heap_frame(128KiB)`.

**v1.31:** Core settlement rule locked in Product §1.2.1 (简体中文): $\rho$ is written only in `begin_settle` after finalize — not at `submit_result`; surplus $S$ only if $\rho=1$, default $70/30$ when $C_R>0$, else all $S$ to the platform. FR-SET-04 / §4.5.2.

**v1.32:** Default Gaussian $n=256$ is creatable on-chain. One ix cannot compute and write 256 $Q64$ $\exp$ masses under the 1.4M CU cap. `grow_grid` only resizes; `write_grid_mass` / `seal_grid` write the same `truncated_normal` prior (chunk size at that landing was 32; current `PRIOR_CHUNK=256`, v1.33). NFR-06.

**v1.33:** Prior kernel: range-reduced $e^x=2^{x/\ln 2}\cdot e^r$ and only $\sim 3\sigma$ nodes call `exp`. `write_grid_mass` chunk is `PRIOR_CHUNK=256`. LMSR fills remain on-chain (FR-MKT / quote already off-chain).

**v1.34:** NFR-06: empty-then-grow is not Gaussian / lognormal only. Dirichlet **simplex** with $k\le4$ MAY exceed 10 240 bytes (`write_grid_mass` streams `simplex_chunk_raw`, chunk 256). Dirichlet atoms over the cap still reject. Changelog chunk 32 in v1.32 is historical.

**v1.35:** 1-D interval mapping is nearest-node, not $\int_a^b$. FR-TRD-12 / product §8.1.2.1: $i^*$ clamps outside $\Omega$; $S$ is the inclusive node range; no partial atom. Example $[5.5,7.8]$ on a $0.1$ grid from $6$ → $\{6.0,\ldots,7.8\}$.

**v1.36:** Product §1.2.2 / FR-TRD-13: selling interval probability $p_S$; $\partial C/\partial q$ is price; ordinary face is $1$ USDC; $1/p_S$ is cash multiple only, never share size or hit payout $q/p_S$.

**v1.37:** Product §1.2.3: claim amount is $q\times\mathrm{hit}\times\rho$, shown on the quote ticket / public result / portfolio. Product §5.1.1 / FR-MKT-15: Gaussian default $n=256$ is enough vs $\sigma$ on the CPI $\Omega$; $512$/$1024$ are finer options, $n=8$ is not a product default. FR-UI-33 copy forbids $p_S$ or $q/p_S$ as the claim number.
