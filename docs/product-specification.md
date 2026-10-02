# Product Specification — Continuous PDF Prediction Market

**Continuous probability prediction market + risk-capital auction + layered tail coverage + pro-rata payout**

| Item | Content |
| --- | --- |
| Version | 1.14 |
| Status | Product specification (features are written as fully delivered; no MVP / later-phase split) |
| Key decisions | Soft solvency: do not reject trades when $L_{\max}$ exceeds capital. Settle on $L=E(c)$ at $c=\mathrm{cell}(x^*)$ (product §8.1). **Settlement gate:** write $\rho=\min(1,C_{\max}/L)$ *before* any user payout — $\rho=1$ if $C_{\max}\ge L$, else the actual ratio $C_{\max}/L$ (SRS FR-SET-03 / FR-SET-11). Do not mix implied PDF $p_k$, exposure $E$, and ticket face. One global pro-rata $\rho$ on **face** (no FIFO). $C_{\max}=R_{\mathrm{net}}+C_R^{\mathrm{final}}+C_P^{\mathrm{alloc}}$. Trading is the primary payout source; $C_R$ is optional. Fees go to the platform fee ledger and do not enter this prediction market’s $C_{\max}$ or $C_P$; the platform may claim them at any time. At `close_ts` both prediction fills and the risk auction stop (`risk_lock_ts \le close_ts`). Price is pure probability; coverage is displayed only. Markets are created by **distribution family**. Football is Skellam. $x^*$ only via `submit_result`. |

---

## 1. Product Definition

This protocol is a prediction market based on a **continuous probability density function (PDF)**.

Unlike traditional YES / NO prediction markets, it trades the full probability distribution of a continuous random variable, not a single binary event. Typical underlyings include:

- BTC / ETH expiry or close price
- CPI, interest rates, FX, commodity prices
- Other continuous economic indicators
- Football and other score-type events: one prediction market per match, see section 4.6
- Macro prints such as CPI, elections, a same-day BTC price, whether an event occurs by a deadline: see sections 4.7–4.11

The market maintains a probability density curve $f(x)$ that always satisfies:

$$
f(x)\ge 0,\qquad \int_{\Omega} f(x)\,dx=1
$$

The curve evolves in a fixed order and is never rewritten by an oracle in the middle:

1. **Initialized at creation.** Write the prior $f_0(x)$ (uniform, or a parameterized family chosen by the creator). At this point $\theta(x)=0$, so the quoted price is the prior.
2. **Updated continuously during trading.** Each interval buy or sell only changes the state $\theta(x)$, then LMSR produces a new $f(x)$. Buying an interval raises density on that stretch; the rest is renormalized downward.
3. **Frozen after close.** Trading stops and $f(x)$ no longer changes. The committee only reports the realized outcome $x^*$; it does not go back and rewrite the distribution.

Users buy an arbitrary interval $I=[a,b]$, mapped to a frozen atom set $S$ (§8.1.2.1). The continuum contract is the Arrow–Debreu indicator; on-chain the same idea is $1_{c\in S}$ with $c=i^*(x^*)$:

$$
g_S(x^*)=1_{c\in S}
$$

The market **price** of that claim equals the current probability mass (continuum: $\int_a^b f$; chain: $p_S$). That number is what an infinitesimal share **costs**, not what it **pays**. One share pays $1$ USDC if it hits ($\rho=1$), so the cash multiple on a small ticket is $\approx 1/p_S>1$ when $p_S<1$. Do not write the share size as $1/p_S$ (product §1.2.2 / §8.1.0).

The protocol consists of two linked markets:

| Market | What is discovered | Description |
| --- | --- | --- |
| Market A: prediction market | $P(X\in I)$ | The probability that the outcome falls in a given interval |
| Market B: risk-capital market | The price of tail risk | How much capital it costs to underwrite the gap if final payout exceeds the board’s own funds |

The product is therefore not “a prediction market + a generic LP pool”, but:

$$
\text{Probability Discovery}+\text{Risk Capital Discovery}
$$

The core innovation: turn the tail liability produced by the prediction market into a financial object that can be priced, auctioned, layered, and underwritten.

### 1.1 Locked business rules

These are protocol rules, not UI hints. Programs, Quote, and the desk SHALL follow them.

| Rule | What happens |
| --- | --- |
| After trading close, no fills | **Every** prediction market (all five families). When `now ≥ close_ts`, `buy_set` / `sell_set` / `buy_skellam_set` / `sell_skellam_set` SHALL NOT execute. Kickoff, live score, committee report, and Bernoulli early YES finalize SHALL NOT reopen trading. The trade ticket SHALL disable buy / sell. Compose SHALL refuse those ops (`403`) when the indexed `close_ts` has elapsed. The chain SHALL reject with `Closed`. |
| Trading close stops the risk auction | When `now ≥ close_ts`, prediction fills **and** new risk-auction quotes / fills SHALL stop. Already-locked $C_R$ stays locked until settlement or VOID. `risk_lock_ts` MAY be earlier than `close_ts` (freeze capital while trading continues). `risk_lock_ts` SHALL NOT be after `close_ts`. Default at listing: `risk_lock_ts = close_ts`. |
| Fees never enter $C_P$ | Listing locks `fee_bps` and **when** the fee is taken. **At fill** (default): buy pays $C_S+\phi C_S$; $\phi C_S$ hits the platform fee ledger immediately. **At claim**: buy pays only $C_S$; when a winner claims, $\phi$ of that payout is taken (miss / VOID: $0$). The platform MAY `claim_fees` into its UserVault **at any time**. Fees never enter $C_{\max}$ or $C_P^{\mathrm{pool}}$. |
| $C_P$ is a capped tap | One protocol pool $C_P^{\mathrm{pool}}$. Listing only locks a tap cap $C_P^{\mathrm{board}}$ (MAY be $0$). At settlement, if $L>R_{\mathrm{net}}$, draw $C_P^{\mathrm{alloc}}=\min((L-R_{\mathrm{net}}-C_R)^+,C_P^{\mathrm{board}},C_P^{\mathrm{pool}})$. If $L\le R_{\mathrm{net}}$, do not draw $C_P$. The pool is not an unlimited guarantee. |
| Soft solvency | Do not reject fills because $L_{\max}$ exceeds capital. After $x^*$ is final, write $\rho=\min(1,C_{\max}/L)$ **before** any user payout. One $\rho$ for every winner; no FIFO. |
| Surplus vs fees | Surplus $S$ exists only when $\rho=1$. $S_P$ is a residual claim (`pay_surplus_platform`), not a fee. Topping up $C_P$ is an explicit `fund_pool`, never an automatic fee transfer. |
| Board comments after the market is live | Comments exist only on an **already indexed** market (`/m/[id]`). `/create` writes the public card (title / tags / event / description) and SHALL NOT collect comments — there is no live market yet. A connected wallet posts as `author` pubkey (same grade as listings; SIWS is not required). The thread is a flat off-chain catalog (`market_comment`): not on-chain, not nested, not $C_{\max}$, not $C_P$, not the fee ledger. Unknown / unindexed market: `404`. |
| The created object is a prediction market | Users create a **prediction market** (预测市场). Product copy SHALL NOT say 盘, 开盘, 关盘, or “board” as the product noun. One football match has one prediction market — 1X2 / handicap / totals are contracts on that market, not separate 盘. On-chain leftover names (`Board`, `board_phase`) are implementation only. |
| Anyone logged in may apply; review **opens trading** | After SIWS, any wallet MAY submit a **market application**. That is not live. `close_ts` is an **absolute** unix time locked at submit (whistle / first print). Solana `Clock` only reads *now* at the create instruction — it cannot learn the event time. If `now ≥ close_ts` at approve, create SHALL fail; the reviewer rejects or the applicant resubmits. The reviewer wallet signs `create_*` (pays rent). **Open the prediction market** (开通预测市场) means the prediction market exists on-chain, the lobby shows it, and traders MAY fill. **Open the risk auction** (开放风险拍卖) means Risk LPs MAY quote published layers for payout coverage. Both start together on approve. Do not call the created object 盘 / “board”, and do not say “listing” or “open the risk book” in product copy. Fees $\phi$ accrue on that prediction market’s fee ledger; `claim_fees` goes to the protocol `platform` (`PLATFORM_PUBKEY`). Compose `create_*` and lobby ingest SHALL require an approved application — a chain account that skipped review SHALL NOT appear as a live market. Rejected applications never become markets. |
| Duplicate listings are refused | Two applications SHALL NOT cover the same event. Duplicate key = distribution family + normalized title + trading event (trim / case-fold ASCII). If another row is `PENDING_REVIEW` or `OPEN`, submit returns `409`. The reviewer MAY reject an application as duplicate. |
| Geo-IP region block | A listing MAY lock blocked countries / regions (ISO 3166-1 alpha-2, optional subdivision). Enforcement is the client IP via GeoIP on lobby, info, quote, compose, trade, auction, and comments. A blocked visitor SHALL NOT see the prediction market as tradable (`403` / hidden). The reviewer MAY add or confirm the block list at review. This is access policy, not settlement math. |

SRS: FR-TRD-01, FR-TRD-04, FR-RSK-05, FR-HAL-01, FR-SET-06–08, FR-UI-32, FR-UI-37, FR-UI-42–45.

### 1.2 End-to-End Business Flow

A prediction market’s lifecycle follows the path below. During the trading period the risk-capital auction runs in parallel. At `close_ts` both prediction fills and the risk auction stop; the prediction market then only waits for the outcome and pays out.

```text
① Choose the distribution family
② Initialize parameters and **apply** (SIWS), locking absolute close_ts. Reviewer approves and **opens trading** (create_*). The risk auction opens at the same time.
③ Trade (LMSR: buying the same outcome makes it more expensive + fees; LPs may quote at the same time)
④ At close_ts, cut off prediction fills **and** the risk auction; freeze f / P
⑤ Wait for the event
⑥ The committee (or an authorized reporter) writes the outcome on-chain. No oracle writes $x^*$.
⑦ Payout (commercial stack: trading first, then optional C_R, then optional C_P)
      ├─ Trading covers L               → full payout; risk capital not drawn
      ├─ Those own funds fall short, but L ≤ C_max → draw Risk LP, then C_P if still short; still full payout
      └─ L > C_max                      → pay all winners at the same ratio ρ = C_max / L
⑧ Only on full payout: surplus is split. If risk capital filled this prediction market, α_R / α_P; if no risk capital entered, residual goes to the platform. Fees stay on the fee ledger until claim_fees — they never enter C_P.
```

**① Choose the probability-distribution type.** Identify the underlying first, then lock the family. It cannot be swapped later.

| Distribution family | Create instruction | Example underlyings (metadata only) |
| --- | --- | --- |
| Skellam (2D score grid) | `create_skellam_market` | Football / other scores; 1X2 and totals are projections |
| Gaussian | `create_gaussian_market` | CPI, inflation, other 1-D prints |
| Lognormal | `create_lognormal_market` | Same-day BTC / ETH price |
| Dirichlet | `create_dirichlet_market` | Election winner, `TOP_N`, vote share (`layout`) |
| Bernoulli | `create_bernoulli_market` | Deadline YES / NO |

**② Initialize and open trading.** The applicant writes prior parameters (e.g. $\lambda_H,\lambda_A$ or $\mu,\sigma$ or $\alpha_i$), the grid or atoms, $\beta$, the resolution source, and an **absolute** `close_ts`. After review approve, the reviewer signs `create_*` — that is **open the prediction market** (开通预测市场): it is live, the lobby shows it, traders MAY fill. The same step **opens the risk auction** (开放风险拍卖): Risk LPs MAY quote published layers. Fees stay on the prediction market until the protocol platform claims them. At this point $\theta=0$, so prices equal $f_0$ / $P_0$. Status is `OPEN` once the market key is attached.

**③ Trade.** The user buys some outcome set $S$ (home win, exactly 1 goal, CPI in a bin, YES…). Each fill pays two amounts:

1. **Contract cost** $C_S(q)$: enters this prediction market’s Vault, used for expiry payout
2. **Fee** $\phi\cdot C_S(q)$: goes to the platform immediately and does not enter the payout pool

The same outcome gets more expensive the more it is bought. Under LMSR, if at open $P(S)=0.2$, a first tiny order has a marginal price of about $0.2$; after it fills, that stretch of density is pulled up, and the next order for the same outcome has a marginal price strictly above $0.2$. Further buys keep lifting it. This is not a queue markup; the curve is rewritten by the fill itself. Selling (reducing a position) presses that stretch down and the price falls back.

During the trading period $L_{\max}$ may exceed available funds. **Orders are not rejected, and user positions are not force-liquidated.** Coverage is display-only. Risk LPs quote published layers and lock collateral in the same window.

**④ Cutoff.** When `now ≥ close_ts`, prediction-side fills stop, the distribution is frozen, **and the risk-capital auction stops**. New quotes and auction fills SHALL be rejected. Already-locked $C_R$ stays until settlement or VOID. `risk_lock_ts` MAY close the auction earlier; it SHALL NOT be after `close_ts`.

**⑤ Wait for the event.** Football waits for full time; CPI waits for the official print; a price board waits until `observe_ts`; a binary event waits until the deadline or an early occurrence.

**⑥ Submit the result.** The chain does not grow $x^*$ by itself. A `submit_result` transaction must write the settlement value into the market account. The reporter is a committee member or an authorized bot. The payload is a score, a published print, a winner, a price, or YES/NO — not “which line won”. An optional `evidence_hash` may be stored; the program does not parse it. No payout before finalization.

**⑦ Settlement gate — compute $\rho$, then pay.** This protocol has no futures-style liquidation. After $x^*$ is final, settlement **first** locks $L=E(c)$ and $C_{\max}=R_{\mathrm{net}}+C_R^{\mathrm{final}}+C_P^{\mathrm{alloc}}$, **then** writes one board-wide recovery rate

$$
\rho=\min\bigl(1,C_{\max}/L\bigr)
$$

($\rho=1$ if $L=0$). **No** `vault.payout` / claim SHALL run until that $\rho$ is written. Two cases only:

| Funds vs liability | $\rho$ | What winners receive |
| --- | --- | --- |
| $C_{\max}\ge L$ (enough) | $\rho=1$ | Full face $\lfloor\mathrm{face}\rfloor$ USDC |
| $C_{\max}<L$ (short) | $\rho=C_{\max}/L$ | Same actual ratio on every winning ticket: $\lfloor\rho\cdot\mathrm{face}\rfloor$ |

$R_{\mathrm{net}}$ is trading revenue; fees never entered it. $C_R$ may be zero; then the prediction market pays from trading, plus any allocated platform adjustment $C_P$. How the pot is drawn when $\rho=1$ still uses the stack below. The commercial stack is section 9. VOID / `RESOLUTION_FAILED` **do not** compute $\rho$ — they refund `cost_paid`.

| Informal wording | Exact condition | What the user receives |
| --- | --- | --- |
| Own funds cover (no risk draw) | $L \le R_{\mathrm{net}}$ | $\rho=1$; $H_i=0$; $C_P$ not drawn |
| Draw risk / adjustment, still full | $R_{\mathrm{net}} < L \le C_{\max}$ | Still $\rho=1$; draw Risk LP by leftover shortfall, then $C_P$ if needed |
| Exceeds maximum payable | $L > C_{\max}$ | $\rho=C_{\max}/L$; all winners at the same ratio; first-come-first-served is forbidden |

Only the third case is a haircut. How $\rho$ is computed is in section 8.1.8 / 8.3: the denominator is the sum of face values that hit the realized outcome, not trading-period $L_{\max}$. SRS: FR-SET-03, FR-SET-11.

**⑧ Surplus allocation.** Surplus exists only when $\rho=1$ (users have already been paid at face value):

$$
S=\max(R_{\mathrm{net}}-L,0)
$$

If this prediction market has filled risk capital ($C_R^{\mathrm{final}}>0$), $S$ is split at the listing lock: $\alpha_R S$ to those Risk LPs (then by each LP’s $\alpha_i$), $\alpha_P S$ to the platform, $\alpha_R+\alpha_P=1$. If no risk capital entered, $S_R=0$ and $S$ goes to the platform. When $\rho<1$, $S=0$. $\phi$ stays on the platform fee ledger until `claim_fees`; it SHALL NOT enter $C_P$. $S_P$ is a separate surplus claim. The platform MAY later `fund_pool` from its own vault; that is a top-up, not a fee sweep.

### 1.2.1 核心业务规则：赔付比率与剩余分发（锁定）

本节是商业规则，不是实现备忘。Vault `begin_settle` / `payout` / `draw_lp` / `pay_surplus_*` / `claim_fees`、Quote、UI 公开结果卡，都必须按这里执行。数学内核：`crates/math` 的 `recovery_rate`、`surplus`、`surplus_parts`、`payout_floor`、`c_p_alloc`、`layer_loss`。

**委员会提交 \(x^*\) 并不计算 \(\rho\)。** `submit_result` 只写入拟议结果。挑战期结束、`finalize` 锁定 \(x^*\) 之后，`begin_settle` 才一次性锁死 \(L\)、\(C_{\max}\)、\(\rho\) 和剩余。没写出 \(\rho\) 之前，任何赢家领取都必须拒绝。交易期展示的 \(\hat\rho\) / 覆盖率 **不是** 这笔结算 \(\rho\)。VOID / `RESOLUTION_FAILED` **不算** \(\rho\)，退回买入时付的本金 `cost_paid`。

**先锁责任和资金，再写一个全场 \(\rho\)。**

| 符号 | 定义 | 不是 |
| --- | --- | --- |
| \(c=\mathrm{cell}(x^*)\) | 委员会锁定的实现结果落到的那个原子 | 交易期最热的点 |
| \(L=E(c)\) | 压在 \(c\) 上的票面合计（`grid.exposure[c]`） | \(L_{\max}\)、\(\sum_k E_k\)、买价 |
| \(R_{\mathrm{net}}\) | 成交收入 − 应付风险保费 | 手续费（手续费另本账） |
| \(C_R^{\mathrm{final}}\) | 风险拍卖里已锁定、结算时可抽的资金；可为 \(0\) | 未成交的报价承诺 |
| \(C_P^{\mathrm{alloc}}\) | 仅当 \(L>R_{\mathrm{net}}\) 时：\(\min((L-R_{\mathrm{net}}-C_R)^+,\,C_P^{\mathrm{board}},\,C_P^{\mathrm{pool}})\)；否则 \(0\) | 无限担保、手续费自动转入 |
| \(C_{\max}\) | \(R_{\mathrm{net}}+C_R^{\mathrm{final}}+C_P^{\mathrm{alloc}}\) | 含 \(C_M\)（已废除） |

$$
\rho=\min\bigl(1,\,C_{\max}/L\bigr)
\qquad(L=0\Rightarrow\rho=1)
$$

| 资金对责任 | \(\rho\) | 赢家领取 | 未中 |
| --- | --- | --- | --- |
| \(C_{\max}\ge L\)（够） | \(1\) | 面值 \(\lfloor\mathrm{face}\rfloor\) USDC | \(0\) |
| \(C_{\max}<L\)（不够） | \(C_{\max}/L\) | **同一个** \(\rho\)：\(\lfloor\rho\cdot\mathrm{face}\rfloor\)，禁止先到先得 | \(0\) |

普通合约 \(\mathrm{face}=q\)（一股命中面值 1 USDC）。足球四分让球按两条半线：全中 \(q\)、中一条 \(q/2\)。地板抹掉的尘埃进准备金，不补给某个人。

**满付时钱从哪出（补缺口，不是分余利）：** 先用 \(R_{\mathrm{net}}\)。若 \(L\le R_{\mathrm{net}}\)，风险层损失 \(H=0\)，不抽 \(C_P\)。若仍缺，按层抽 \(H=\min((L-A)^+,D_i)\)，且不超过尚未补上的缺口。再不够才抽 \(C_P^{\mathrm{alloc}}\)。

**赔付后还有剩，怎么分。** 剩余 **只在 \(\rho=1\)**（赢家已按面值满付）时存在：

$$
S=\max(R_{\mathrm{net}}-L,\,0)
$$

\(\rho<1\) 时 \(S=0\)，没有“多余”可分。

| 条件 | \(S_R\)（风险资金方） | \(S_P\)（平台） |
| --- | --- | --- |
| \(C_R^{\mathrm{final}}=0\)（没有风险资金进来） | \(0\) | 全部 \(S\) |
| \(C_R^{\mathrm{final}}>0\) | \(\alpha_R S\) | \(\alpha_P S=(1-\alpha_R)S\) |

开通预测市场时锁定 \(\alpha_R\)（`alpha_r_bps`）。缺省 **7000 bps：70% 给风险资金方、30% 给平台**。资金方那一份再按各笔成交报价的权重 \(w_i=\) `profit_share_bps` \(\times\) 成交量摊，指令 `pay_surplus_lp`；平台一份 `pay_surplus_platform`。手续费 \(\phi\) 走 `claim_fees`，**不进** \(C_{\max}\)，也不进 \(S\)。平台日后 `fund_pool` 是自己往调节池注资，不是把手续费扫进 \(C_P\)。

### 1.2.2 核心业务规则：卖的是概率，份额面值是 1（锁定）

本节锁定「PDF 预测市场卖什么」。Quote、成交、公开预览、结算卡都必须用同一套名字。数学内核：`crates/math` 的 `interval_prob` / `buy_cost` / `lmsr_cost` / `ticket_face`。

**卖的是区间概率，不是把 PDF 误解成别的东西。** 开通预测市场时写入先验 \(f_0\)（CPI 高斯是 \(\mu,\sigma\) 用百分点，例如中位数 \(5\%\)、离散 \(1\%\)，不是「总概率等于 \(5\%\)」）。用户买 \([a,b]\)（先按 §8.1.2.1 吸成节点集合 \(S\)）时，报价是这段概率质量：

\[
p_I=\int_a^b f_\theta(x)\,dx\qquad\text{（连续模型）},\qquad
p_S=\sum_{k\in S}p_k\qquad\text{（链上）}.
\]

\(p_S\in(0,1)\)。成交只改 \(\theta\)，再归一化得到新的 \(f_\theta\)；下一笔用新的 \(p_S\)。委员会只报 \(x^*\)，不改分布。

**对数量求导得到的是价格，不是到期赔付。** LMSR 成本 \(C_S(q)\) 对 \(q\) 的导数是边际价：

\[
P_S(q)=\frac{\partial C_S}{\partial q}=\frac{p_S e^{q/\beta}}{1-p_S+p_S e^{q/\beta}}\in(0,1),\qquad P_S(0)=p_S.
\]

到期是 Arrow–Debreu：**中了赔 1 USDC，没中赔 0**（再乘全场 \(\rho\)）。现金倍数 \(\approx 1/p_S>1\) 是「付 \(p\) 拿 \(1\)」的回报，**不是**把份额定义成 \(1/p_S\)，也不是把 \(C'(q)\) 当成结算。

| 说法 | 产品里是 | 禁止 |
| --- | --- | --- |
| 价格 \(=\int_a^b f\) | \(p_S\)，买一丁点份额要付的钱 | 把 \(p_S\) 当中了拿多少 |
| 价格在 \((0,1)\) | 边际价 \(P_S(q)\) | 把导数写成 payoff |
| 赔付 \(>1\) | 付约 \(p_S\)、中了拿 \(1\)，倍数 \(\approx 1/p_S\) | 每份面值写成 \(1/p_S\) 再乘份数（变成 \(1/p_S^2\)） |
| 份数 \(\times\) 价格 | 小单 \(\approx q\cdot p_S\) | 大单仍按 \(q\cdot p_S\) 收费（必须用 \(C_S(q)\)） |
| 份数 \(\times\) payoff | 中了 \(\lfloor\rho\cdot q\rfloor\) | \(\lfloor\rho\cdot q/p_S\rfloor\) |
| 分布随交易变 | \(\theta\) 更新后再报价 | 成交后改 \(p_0\) / 改 \(\mu,\sigma\) |

精确应付：

\[
C_S(q)=\beta\ln\bigl((1-p_S)+p_S e^{q/\beta}\bigr)
\]

用户付 \(C_S(q)+\phi C_S(q)\)，得到 \(q\) 份冻在 \(S\) 上的券。小单 \(C_S(q)\approx q\cdot p_S\)，买得越多 \(p_S\) 被抬高，线性 \(q\cdot p_S\) 不够。

例：\(p_S=0.7\)，买 \(q=10\)。大约先付 7 USDC（再加费用和滑点）。仓位仍是 10 份。\(i^*(x^*)\in S\) 且 \(\rho=1\) 拿 **10**，不是 \(10/0.7\)。VOID 退买入本金，不走 \(\rho\)。

### 1.2.3 核心业务规则：用户怎么知道自己该领多少（锁定）

结算后该拿多少 **不是**「价格的倒数」，也 **不是**用户自己算 \(\rho\)。一张票是冻结集合 \(S\) 上的 \(q\) 股。普通合约一股命中面值 **1 USDC**（足球四分让球见 §1.2.1）。委员会只报 \(x^*\)；`finalize` 之后 `begin_settle` 才锁死全场同一个 \(\rho\)。领取指令用链上已写的 \(\rho\) 和面值算 \(\lfloor\rho\cdot\mathrm{face}\rfloor\)，打进 UserVault。

**三步读数（Quote、公开结果卡、`/portfolio` 结算票必须同一套）：**

| 何时 | 用户看见什么 | 那是不是最终到手 |
| --- | --- | --- |
| 下单前 | 报价票：命中面值 \(q\)（普通合约）、未中 \(0\)；展示用的 \(\hat\rho\) / 覆盖率 | 否。\(\hat\rho\) 只是警告，不是结算 \(\rho\) |
| 结果锁定后 | 公开结果卡：\(x^*\)、命中格子 \(c\)、\(L=E(c)\)、\(C_{\max}\)、已写入的 \(\rho\) | \(\rho\) 已锁。自己的票还要看 \(c\in S\) 与否 |
| 点领取时 | 结算票：中了 \(\lfloor\rho\cdot q\rfloor\)，没中 \(0\)；VOID / 决议失败退 `cost_paid` | 是。Vault `payout` 按此入账，用户不必手算 |

**自己这张票：** 仓位记下 \(q\) 和 \(S\)（区间掩码，或足球 `--kind` / \(a,b\)）。\(c\) 是离 \(x^*\) 最近的节点（§8.1.2.1）。\(c\in S\) 则 \(\mathrm{face}=q\)，否则 \(0\)。实发 \(\lfloor\rho\cdot\mathrm{face}\rfloor\)；费用若在领取时扣，再减去 \(\phi\)。资金够则 \(\rho=1\)（满面值）；不够则所有赢家同一个 \(\rho=C_{\max}/L\)，没有先到先得。

一句话：该领多少 \(=\) **我的 \(q\) × 是否命中 × 全场 \(\rho\)**。不是 \(p_S\)，不是 \(q/p_S\)。

### 1.3 Business Flow Diagram

The full figure is `business-flow.png` (same folder).

![End-to-end business flow](business-flow.png)

```mermaid
flowchart TB
  subgraph P1["① Create the market"]
    A1[Choose event type and lock the distribution family] --> A2[Initialize parameters / β / grid<br/>open the auction book]
    A2 --> A3[Delegate to ER<br/>enter TRADING]
    A3 --> A4[Open price = prior θ = 0]
  end

  subgraph P2["② Trading period · dual tracks in parallel"]
    direction LR
    subgraph T["Prediction side · Trader"]
      T1[Buy outcome set S<br/>pay Cq + fee φ·Cq] --> T2[LMSR: same outcome gets more expensive<br/>Cq into Vault, fee to the platform]
      T2 --> T3[Update E and Lmax<br/>no reject, no liquidation]
    end
    subgraph R["Risk side · Risk LP"]
      R1[Quote published layers<br/>capacity / premium / profit share] --> R2[Lock collateral ≥ D<br/>fill from lowest unit premium]
      R2 --> R3[C_R rises, coverage improves<br/>liability cap remains D]
    end
  end

  A4 --> T1
  A4 --> R1

  subgraph P3["③ Cutoff and freeze"]
    C1[close_ts stops prediction trading AND the risk auction<br/>already-locked C_R freezes; write back to L1]
  end

  T3 --> C1
  R3 --> C1

  subgraph P4["④ Committee submits the event result"]
    D1[Wait for the event] --> D2[submit_result on-chain<br/>score / print / winner / price / YES-NO]
    D2 --> D3{Challenge window:<br/>anyone object?}
    D3 -->|No| D4[finalize locks x*]
    D3 -->|Yes| D5[M/N vote]
    D5 -->|Pass| D4
    D5 -->|Fail| D6[RESOLUTION_FAILED<br/>refund users / collateral / unused premium]
  end

  C1 --> D1

  subgraph P5["⑤ Settlement and payout"]
    E1[Compute L = E x* and C_max] --> E2{Compare L with funds}
    E2 -->|L ≤ R_net| E3[Full payout ρ=1<br/>H=0, C_P not drawn, compute S]
    E2 -->|Need C_R and/or C_P, L ≤ C_max| E4[Draw Risk LP then C_P<br/>still full ρ=1]
    E2 -->|L > C_max| E5[ρ = C_max / L<br/>same ratio, S = 0]
    E3 --> E6[α_R to Risk LP<br/>α_P to the platform]
    E4 --> E6
    E5 --> E7[No surplus]
    E6 --> E8[CLOSED]
    E7 --> E8
  end

  D4 --> E1
```

---

## 2. Problems to Solve

Traditional prediction markets have two structural defects:

1. **Insufficient expressiveness.** YES / NO can trade only one event. They cannot trade “which interval the price will fall into, and how wide the distribution is”.
2. **Scale is capped by own funds.** If the protocol requires $L_{\max}$ to stay inside a creator reserve, the market cannot grow. If it allows naked shorting and then looks for a rescue after liquidation, user payout has no bound agreed in advance.

Goals of this protocol:

- Initialize the probability distribution first, then let trading continuously rewrite it
- Trade the full distribution with a continuous PDF
- Use Risk LPs to underwrite the tail in layers
- **Allow trading to continue even when risk capital has not yet caught up**
- **At expiry, pay only the liability on the realized outcome; if funds fall short, haircut all winners at the same ratio**
- Cap “who pays at most how much” on already-locked capital, not on after-the-fact top-ups

One-sentence principle:

> Orders are not rejected for insufficient funds. At expiry, only the liability on the realized $x^*$ is paid. Maximum payable capital is whatever is actually in place at settlement. If that is not enough, pay pro-rata — not by arrival order.

---

## 3. Roles

### 3.1 Trader

Buys a probability interval $I$ and receives **shares** $q$ equal to the size bought. If $x^*\in I$, the claim is those shares; the USDC paid is $\rho\cdot q$ (section 8). Misses pay 0.

Traders have no market-making obligation and do not provide risk capital.

### 3.2 Market

Maintains and executes:

- The PDF $f(x)$ and state $\theta(x)$
- Interval quotes and fills
- User positions and total exposure $E(x)$
- Market revenue, reserves, and current theoretical maximum liability $L_{\max}$
- Risk-capital demand and coverage display
- Expiry settlement and haircuts

### 3.3 Risk LP

A Risk LP is not a traditional AMM liquidity provider. It is a **tail underwriter**.

In a specified risk layer $[A,A+D]$, they commit to pay at most $D$ and lock collateral of at least $D$. Revenue comes from:

- Risk Premium
- Optional Profit Share (only residual profit after full payout and after reserves meet the bar)

They bear the Tail Loss when that layer is hit. Later fills do not increase their committed liability.

### 3.4 Risk Auction

Auctions, in layers, the tail risk that exceeds the board’s own funds. Risk LPs quote layer, size, premium, collateral, and profit share. The market selects a set of quotes under constraints, aiming to fill the required coverage and to keep total premium as low as possible.

In v1.1 the risk auction is a **continuous capital-top-up mechanism**, not a hard gate on open or on placing an order. When risk capital is insufficient, trading still proceeds; coverage falls and expiry may haircut.

### 3.5 Resolver

The person who **writes the final outcome $x^*$ as an on-chain transaction**. Without that transaction, no feed can enter the contract.

- A committee member or authorized reporting bot calls `submit_result`
- May attach an opaque `evidence_hash` (the program does not parse it)
- Finalization happens only after the challenge window or after a vote passes

### 3.6 Platform

The protocol operator of the fee pot, surplus share $S_P$, and the **platform adjustment fund pool** $C_P^{\mathrm{pool}}$.

- Receives $\phi\cdot C_S(q)$ on every fill onto the **platform fee ledger**. That money is platform income. It does **not** enter this board’s $C_{\max}$ or $C_P^{\mathrm{pool}}$. The platform MAY `claim_fees` into its UserVault at any time.
- Receives $S_P$ only when $\rho=1$.
- Holds **one** protocol-level pool $C_P^{\mathrm{pool}}$ funded by `fund_pool` (and optional later top-ups). Fees SHALL NOT be swept into the pool. Boards do not hold their own $C_P$ balance.
- A short board may draw $C_P^{\mathrm{alloc}}$ from that pool at settlement, never more than the pool and never more than the board cap.
- The pool is a capped commercial reserve, not an unlimited guarantee and not a second LMSR price.

---

## 4. Continuous PDF Market

The prediction market’s primary state is $f(x)$. The product cadence is: **initialize first, then keep deforming with every fill**.

```text
Create the market
   │  Choose Ω, β, prior f_0(x)
   │  θ(x) = 0
   ▼
f(x) = f_0(x)
   │
   ▼
Each buy / sell of interval I, quantity q
   │  θ_new(x) = θ_old(x) + q · 1_I(x)
   │  Recompute Z[θ], obtain the new f(x)
   │  Interval price = current ∫_I f(x) dx
   ▼
The next trade continues to rewrite f(x)
   │
   ▼
Close: freeze f(x) and positions, wait only for x*
```

$f(x)$ **is not specified by an oracle and is not rewritten at settlement**. The committee only supplies $x^*$, used to compute $L=E(x^*)$. Price discovery comes entirely from trading.

### 4.1 Market State

Market state is a density $f(x)$ defined on the interval $\Omega=[x_{\min},x_{\max}]$, jointly determined by the prior $f_0(x)$ and the accumulated state $\theta(x)$.

### 4.2 Initial Distribution $f_0$

$f_0$ is written once at genesis and is never officially rewritten afterward. It is the starting point of the curve (or discrete mass), not the final distribution. **The underlying type determines the family**: football, CPI, elections, prices, and binary events each use a different state space and $f_0$; see 4.7.1. When information is missing, fall back only to the maximum-entropy / unbiased form *inside that family* (widen the Gaussian, set all Dirichlet $\alpha_i=1$, flatten Poisson $\lambda$, etc.). Do not switch families.

**1. Dirichlet / mutually exclusive categories**

Used when “exactly one option occurs”: 1X2, who is elected, whether a measure passes. The true outcome model is Categorical; Dirichlet is the prior on the probability simplex and supplies initial weights $(\alpha_1,\ldots,\alpha_K)$. After normalization:

$$
p_i=\frac{\alpha_i}{\sum_k\alpha_k},\qquad \sum_i p_i=1
$$

When $\alpha_i=1$, this degenerates to a uniform categorical prior. Thereafter trading rewrites $\theta_i$ on $K$ atoms, equivalent to discrete LMSR; no continuous grid is needed.

**2. Gaussian / continuous numeric**

Used for temperature, inflation, prices, FX, and other quantities that live on the reals (or a truncated interval):

$$
f_0(x)=\mathcal{N}(\mu_0,\sigma_0^2)
$$

Truncate and renormalize on $\Omega=[x_{\min},x_{\max}]$. When $\sigma_0$ is large, this approaches a uniform unbiased prior.

**3. Poisson / counts**

Used for goals, incidents, post counts, and other non-negative integers:

$$
P_0(K=k)=\frac{\lambda_0^k e^{-\lambda_0}}{k!},\qquad k=0,1,2,\ldots
$$

On-chain, truncate to $\{0,1,\ldots,K_{\max}\}$ and renormalize.

**4. Skellam / difference of two counts (primary sports scheme)**

If home and away goals are approximately independent, $X\sim\mathrm{Poisson}(\lambda_H)$, $Y\sim\mathrm{Poisson}(\lambda_A)$, then the margin

$$
D=X-Y\sim\mathrm{Skellam}(\lambda_H,\lambda_A)
$$

$$
P(D=d)=e^{-(\lambda_H+\lambda_A)}\left(\frac{\lambda_H}{\lambda_A}\right)^{d/2}I_{|d|}\bigl(2\sqrt{\lambda_H\lambda_A}\bigr)
$$

The same pair $(\lambda_H,\lambda_A)$ simultaneously yields:

| Derived line | Formula |
| --- | --- |
| Home | $P(D>0)$ |
| Draw | $P(D=0)$ |
| Away | $P(D<0)$ |
| Handicap / margin interval | $P(D\in[a,b])$ |
| Total goals | $X+Y\sim\mathrm{Poisson}(\lambda_H+\lambda_A)$ |
| Correct score | $P(X=i,Y=j)$ (independent Poisson or bivariate Poisson) |

Skellam is therefore not a fourth unrelated prior. It is the **generative bridge between a Poisson count market and a Dirichlet categorical market**: first initialize two attack intensities, then project many lines.

Independent Poisson underestimates draws and ignores attack–defense correlation. When needed, apply a Dixon–Coles low-score correction, or a bivariate Poisson with covariance; the margin can still keep Skellam form.

#### One match, one prediction market

The product shape is: **one football match has exactly one prediction market**. Everyone on that market buys different contracts — home / draw / away, home handicap, totals, correct score. It looks like several questions, but underneath it is the same joint score distribution $P(X=i,Y=j)$, the same pot of funds, the same Risk LPs, and the same settlement score.

It is not a separate, unrelated market for each play type. Every buy is a different set of cells on this one score table.

The underlying state is a truncated score grid, e.g. $i,j\in\{0,1,\ldots,10\}$, $11\times 11=121$ atoms. At genesis, write independent Poisson (or Dixon–Coles / bivariate Poisson):

$$
P_0(X=i,Y=j)=\frac{\lambda_H^i e^{-\lambda_H}}{i!}\cdot\frac{\lambda_A^j e^{-\lambda_A}}{j!}
$$

(Renormalize after truncation.) Users always buy a cell set $S$. Payoff is $1_{(X,Y)\in S}$, price is $\sum_{(i,j)\in S}P(i,j)$. Common lines are just preset $S$:

| Line | Set $S$ |
| --- | --- |
| Home | $\{(i,j):i>j\}$ |
| Draw | $\{(i,j):i=j\}$ |
| Away | $\{(i,j):i<j\}$ |
| Home handicap $h$ | $\{(i,j):i-j>h\}$ (half / quarter lines split into two sets by rule) |
| Total goals over $k$ | $\{(i,j):i+j>k\}$ |
| Both teams to score | $\{(i,j):i\ge 1,j\ge 1\}$ |
| Home goals over $k$ | $\{(i,j):i>k\}$ |
| Correct score $i$-$j$ | Single cell $\{(i,j)\}$ |
| Margin in $[a,b]$ | $\{(i,j):a\le i-j\le b\}$ |

Trading any line updates $\theta_{ij}\leftarrow\theta_{ij}+q$ on the cells in $S$, then LMSR recomputes the entire $P(i,j)$. Therefore:

- Someone buys “home”: home cells rise, draw / away are pressed down, handicap and totals prices move with them
- Someone buys “over 2.5”: high-score cells rise, 0-0 / 1-0 / 0-1 fall; the draw and cheap low-score home wins become cheaper together
- All derived lines stay **arbitrage-free consistent**: “home 70% while $P(D>0)=55\%$” cannot occur

Settlement still looks only at the realized score $(x^*,y^*)$. Positions whose set contains that cell count toward $L$ at face value; if funds fall short, one global $\rho$. Risk LPs underwrite this match’s total exposure, not a mutually unrecognized pot per line.

Do not mix these two implementations:

| Approach | Meaning | Fit |
| --- | --- | --- |
| **Recommended: shared score grid** | One underlying PDF per match; many lines are different projections | Sports; needs cross-line consistency |
| Independent boards | Separate LMSR books for 1X2, totals, etc. | Simpler to implement, but prices can contradict |
| Update only $(\lambda_H,\lambda_A)$ | Parameterized AMM; state is two numbers | Tiny state; but “buy home” does not invert $\lambda$ uniquely, and cross-line behavior is stiff |

The sports line uses the shared score grid: Skellam / Poisson is only for initialization. After $f_0$ is written, trading freely shapes the table; it is not locked in a two-parameter family forever. Do not also build a parameterized AMM that only updates $(\lambda_H,\lambda_A)$.

In the continuous or truncated case it must hold that:

$$
\int_{\Omega}f_0(x)\,dx=1
$$

In the discrete case, replace with $\sum_k P_0(k)=1$.

### 4.3 Continuous Cost Function

The market maintains a continuous state function $\theta(x)$, initially $\theta(x)=0$. The cost function is:

$$
C[\theta]=\beta\log\int_{\Omega}f_0(x)\,e^{\theta(x)/\beta}\,dx=\beta\log Z[\theta]
$$

where:

- $f_0(x)$: initial distribution
- $\theta(x)$: accumulated state from historical trades
- $\beta$: liquidity / sensitivity parameter; larger $\beta$ makes the curve harder for a single trade to move
- $Z[\theta]=\int_{\Omega}f_0(x)\,e^{\theta(x)/\beta}\,dx$: partition function

The corresponding market PDF:

$$
f_{\theta}(x)=\frac{f_0(x)\,e^{\theta(x)/\beta}}{Z[\theta]}
$$

### 4.4 How Trades Rewrite $f(x)$

After open, the distribution changes only with fills. Continuum notation below is the model; the chain buys a **frozen node set** $S$, not a length-weighted slice of $[a,b]$ (mapping: §8.1.2.1). When a user buys interval $I=[a,b]$ in quantity $q$:

$$
\theta'(x)=\theta(x)+q\cdot 1_I(x)
$$

If historical trade $j$ bought $I_j$ in quantity $q_j$, then:

$$
\theta(x)=\sum_j q_j\cdot 1_{I_j}(x)
$$

Buying $I$ pulls density up on that interval; because $Z[\theta]$ renormalizes, density outside the interval is pressed down. Selling is equivalent to $q<0$ (implementation must separately check sellable position).

Let the current interval probability be $p_I=\int_I f_{\theta}(x)\,dx$ in the continuum, and $p_S=\sum_{k\in S}p_k$ on the grid. Quote, fill, and settle use $p_S$, not a fresh $\int_a^b$.

Then that trade has a closed-form cost:

$$
C_I(q)=\beta\log\bigl((1-p_I)+p_I e^{q/\beta}\bigr)
$$

On the grid this is $C_S(q)$ with $p_S$ in place of $p_I$.

Marginal price:

$$
P_I(q)=\frac{p_I e^{q/\beta}}{1-p_I+p_I e^{q/\beta}}
$$

Hence $P_I(0)=p_I$. **This derivative is the marginal fill price, not the settlement payout** (product §1.2.2). One share still pays $1$ USDC if it hits. The fill price is quoted as pure probability; expected haircut is not baked into the price. Coverage is displayed separately; see section 8.5.

The same outcome gets more expensive the more it is bought. Suppose at open $p_S=0.2$ (e.g. football “exactly 1 goal” currently 20%):

- First small order: marginal price about $0.2$, cost near $0.2q$
- After the fill, density on $S$ rises and $p_S$ becomes $0.26$
- The next buy of $S$ starts from $0.26$, higher than the first
- Keep buying the same $S$ and the price rises monotonically along $P_S(q)$ toward 1

User actually pays $=C_S(q)+\phi\cdot C_S(q)$. $\phi$ is the protocol fee rate, locked at market creation.

**Share unit (locked).** One share is a claim on **1 USDC of face** if the bought set contains $x^*$, else $0$. The USDC spent at fill is the LMSR cost, not the face.

Example — CPI board, user buys the print interval $[0.3,0.4]$ (mapped to atom set $S$). Current $p_S=0.7$:

- A **tiny** buy of $1$ share costs about $0.7$ USDC (marginal price $P_S(0)=p_S$). A **large** $q$ costs the closed form $C_S(q)$, so average USDC per share is $C_S(q)/q\ge p_S$ (impact). Plus fee $\phi C_S(q)$.
- The ticket is credited **$q$ shares**, not “$0.7q$ shares”.
- If the official CPI print maps into $S$: face $=q$ USDC, paid $\lfloor\rho\cdot q\rfloor$. If $C_{\max}$ covers $L$, $\rho=1$ and $q$ shares pay $q$ USDC (profit $\approx q-C_S(q)$ before fee).
- If the print misses $S$: face $=0$; the $C_S(q)$ already paid stays in the pot (it is $R_{\mathrm{net}}$).

Do not redeem “the $0.7$ you paid” and do not treat $p_S$ as the payout. $p_S$ is only the **price of a share**.

**Pre-bet (read-only).** Before `buy_set`, the Quote Engine / `GET /v1/markets/{id}/preview` shows the ticket a user is about to take: Pay $=C_S(q)+\phi C_S(q)$, fee, face if $S$ hits ($\lfloor\hat\rho\cdot q\rfloor$ while live), $0$ if it misses, net if hit, and an optional book EV that treats $p_S$ as the viewer’s own probability. None of these replace $p_S$. Coverage / $\hat\rho$ stay display-only.

**This is the same rule on every board and every line** — not a CPI special case. Home / draw / away, handicap, totals, BTTS, exact score, election winner / TOP_N / vote-share band, BTC interval, YES/NO, custom mask: each is some $S$, quoted at $p_S$, minted as $q$ shares, paid $\lfloor\rho\cdot\mathrm{face}\rfloor$ if $c\in S$ (quarter AH is the only split-face exception, section 8.1.5). Section 8.1.

### 4.5 Positions, Shares, and Liabilities

A fill of size $q$ on interval (or set) $I$ **credits $q$ shares** on that ticket. $q$ is the claim unit, not the USDC paid at fill (that USDC is $C_I(q)+\phi\cdot C_I(q)$).

A position is $(S_j,q_j)$: $q_j$ shares on a **frozen atom set** $S_j$ (section 8.1). Ordinary face is $q_j$ if $x^*\in S_j$, else $0$. Quarter lines split into two legs of $q_j/2$. All **legs** stack into an exposure curve:

$$
E(x)=\sum_{\ell} q_\ell\,1_{S_\ell}(x)
$$

If the final outcome maps to atom $c$, the market owes $L=E(c)$ — not $\sum_x E(x)$ and not $\sum_j q_j$ over tickets that merely “look like winners”. $L_{\max}=\sup_x E(x)$ is a trading-period monitor only.

**Highest risk payout (every family, including Gaussian).** Users buy intervals, not cells. $E(x)$ **is** the overlap depth: how much face would pay if the print is $x$. The interval to list is the **thickest overlap**, $\{x:E(x)=L_{\max}\}$ — the contiguous plateau at $\max E$, mapped back onto $\Omega$ (node midpoints). That is not a volume ranking of tickets, and it is not “enumerate every possible $[a,b]$”. A fill of $q$ on $I$ adds $q$ to every atom in $S$; overlapping tickets stack on the intersection. Do not sum $E$ across a bought band (that recounts the same $q$). Football labels the score cell; Bernoulli labels YES / NO. A live tape (`/`, `/ops`) SHALL refresh this row per prediction market on the indexer cadence (UI poll $2\,\mathrm{s}$). The PDF peak $p_k$ is a different object. After settlement the tape yields to $L=E(c)$.

On a football board, exposure lives on score cells: $E(i,j)$. Settlement uses $L=E(x^*,y^*)$. The implied PDF $p_{ij}$ is a different object (section 8.1). Listing steps are in 4.6.

### 4.6 How a Skellam (Football Score) Board Is Created

A match calls the create instruction once and produces **one prediction market**. 1X2, handicap, totals, and correct score are preset contracts on that market; they do not each call `create_market`.

#### 4.6.1 Who Creates It and When

Any SIWS-logged-in wallet MAY apply to open a market (off-chain; not on-chain). A **system reviewer** must approve, then **that reviewer wallet** **opens trading** (`create_*`) and **opens the risk auction** (`risk_open_book`) as owner. Only then is the market live (lobby + fills + risk auction). The applicant SHALL NOT sign create. Recommended: apply no later than a few hours before kickoff, so Risk LPs have time to lock collateral after the reviewer opens the risk auction. Duplicate events are refused. Region blocks are IP/GeoIP, not a second settlement rule.

Trading covers pre-match and in-play: fills are allowed from listing until `close_ts`. `close_ts` defaults to the full-time whistle for that `score_scope` (end of regulation, or end of extra time), not kickoff. Kickoff does not stop trading. During in-play the underlying is still the **final score** $(X,Y)$, not “the next goal”. Goals and red cards do not rewrite $P_0$; only traders buying and selling continue to change $\theta$. Live score is display and risk hint only.

#### 4.6.2 Creation Parameters

`create_skellam_market` writes the following fields in one shot. The listing name “football” is not an instruction.

**Match identity**

| Field | Description |
| --- | --- |
| `match_id` | External event ID (e.g. Sportradar / an in-house catalog) |
| `home` / `away` | Home and away teams |
| `kickoff_ts` | Kickoff time |
| `league` / `season` | Display and risk grouping |

**Settlement convention (must be locked before open)**

| Field | Rule | Description |
| --- | --- | --- |
| `score_scope` | `REGULATION_90` / `INCLUDE_ET` / `INCLUDE_PENS` | Must pick exactly one and lock it. League regulation boards use the first; cup extra-time boards and boards that include penalties are each a separate listing. One board must not mix conventions |
| `own_goals` | Count as goals | An own goal is a goal against the scoring side and a goal for the opponent |
| `abandon_policy` | `VOID` | Abandoned / postponed and not completed inside the window: void and refund |

The same `match_id` may have boards with different `score_scope` at the same time (e.g. one regulation, one including extra time). Funds and PDFs are independent. On one board, 1X2, handicap, and totals must share that board’s `score_scope`.

**Score grid**

| Field | Rule |
| --- | --- |
| `k_max` | `10` |
| Atom count | $(k_{\max}+1)^2=121$ |
| Overflow | If a realized score on one side is $>k_{\max}$, it falls into that side’s overflow bucket. E.g. 12-1 is recorded as $(10^+,1)$, the same settlement cell as $(10,1)$. At creation the UI must show “10+” |

**Prior $P_0$ (written once)**

| Field | Description |
| --- | --- |
| `prior_family` | `independent_poisson` (default) / `dixon_coles` / `uniform` |
| `lambda_home` / `lambda_away` | Pre-match expected goals, from a model or the creator |
| `dc_rho` | Dixon–Coles only; corrects 0-0 / 1-0 / 0-1 / 1-1 |
| `beta` | LMSR liquidity. Larger = harder for a single trade to move the whole table |

$$
P_0(i,j)=\frac{\lambda_H^i e^{-\lambda_H}}{i!}\cdot\frac{\lambda_A^j e^{-\lambda_A}}{j!},\quad i,j\in\{0,\ldots,k_{\max}\}
$$

After truncation, divide by $\sum_{i,j}P_0(i,j)$ so the 121 cells sum to 1. `uniform` gives each cell $1/121$, used only when there is no pre-match information at all.

**Capital and resolution source**

| Field | Description |
| --- | --- |
| `resolution_source` | `committee` (required) |
| `resolvers` | Committee roster or the public committee |
| `authorized_reporters` | Optional; sports-bot public keys; they may only propose; disputes still go to the committee |
| `close_ts` | Defaults to the full-time whistle for that convention; an in-play prediction market must not stop trading before kickoff. After `close_ts` there are no fills. |
| `risk_lock_ts` | Moment risk capital stops being accepted; SHALL NOT be after `close_ts`. Default equals `close_ts`. After `close_ts` the auction is closed even if this field was set later in an old listing |
| `report_window` | Post-match report / challenge window |

Football $x^*$ is an integer pair $(x^*,y^*)$. See 10.6.

#### 4.6.3 Preset Contracts (several questions on one prediction market)

Creation opens a set of templates. Each template is only a name for a set $S$; it does not open a separate pot. The following must be open:

| Contract group | Options the user sees | Set rule |
| --- | --- | --- |
| 1X2 | Home / Draw / Away | $i>j$ / $i=j$ / $i<j$ |
| Handicap | Integer, half, and quarter lines, e.g. $-0.5,-0.75,-1,-1.5$ | See below |
| Totals | e.g. $1.5,2.5,3.5,4.5$ | Over: $i+j>k$; under: $i+j\le k$ |
| Both teams to score | Yes / No | $i\ge 1,j\ge 1$ or its complement |
| Correct score | $0$-$0$ through $k_{\max}$-$k_{\max}$, plus overflow display | Single cell |
| Custom set | Optional | Any union of cells, for professional users |

Handicap settlement:

- **Integer / half** (line in half-goals, e.g. $-3=-1.5$): a single set. Home handicap wins iff $2(i-j)+\mathrm{halves}>0$. Integer push ($=0$) is not in $S$. Home $-1.5$ is $i-j\ge 2$, a subset of home.
- **Quarter** (e.g. $-0.75$): one notional position $q$ splits into two adjacent half-line fills of $q/2$ each. E.g. home $-0.75$ = $q/2$ of $-1.0$ then $q/2$ of $-0.5$ on the **same** $\theta$. Both win → credit $q$; one wins → credit $q/2$; both lose → $0$. The fee is $C_{S_1}(q/2)+C_{S_2}(q/2)$ after the first half has already moved the book. On-chain this is `buy_skellam_set(HomeHandicapQuarter)` (one ticket, two LMSR updates), not a second market.

The frontend renders these templates as multiple lines. They are not separate pots and do not have separate $\theta$.

#### 4.6.3.1 Shared-board LMSR (normative)

There is one $11\times 11$ state: $p0_{ij}$, $\theta_{ij}$, $E_{ij}$. Every football fill — 1X2, handicap, totals, BTTS, exact score, or a custom union — is the same LMSR on a cell set $S$:

$$
Z=\sum_{i,j}p0_{ij}\,e^{\theta_{ij}/\beta},\qquad
p_S=\frac{\sum_{(i,j)\in S}p0_{ij}\,e^{\theta_{ij}/\beta}}{Z}
$$

$$
C_S(q)=\beta\ln\bigl((1-p_S)+p_S e^{q/\beta}\bigr)
$$

Then only cells in $S$ update: $\theta_{ij}\leftarrow\theta_{ij}+q$, $E_{ij}\leftarrow E_{ij}+q$. $L_{\max}=\max_{ij}E_{ij}$ (cell max, not “per product line”).

Consequences that implementation SHALL preserve:

- Buying home ($S=\{i>j\}$) raises $p_{\mathrm{home}}$ and also raises exact 2-1, because $(2,1)\in S$.
- Buying over 2.5 and then home $-1.5$ **stacks** $q$ on the intersection (e.g. 3-0 gets both fills); disjoint cells get only one.
- After any mix of lines, $p_{\mathrm{home}}+p_{\mathrm{draw}}+p_{\mathrm{away}}=1$ still holds (one partition of the same table).
- Numerics live in `crates/math` (`football` masks + `lmsr_update`). Programs SHALL NOT reimplement a second LMSR.

On-chain:

| Intent | Instruction |
| --- | --- |
| Typed line (1X2, totals, AH, BTTS, exact, quarter) | `buy_skellam_set` / `sell_skellam_set` — expand the template to $S$, then `lmsr_update` |
| Custom cell union | `buy_set` / `sell_set` with a 121-bit mask, same LMSR |
| Non-Skellam family | `buy_set` / `sell_set` only |

#### 4.6.4 Listing Steps

```text
1. Check the match is not already listed (unique match_id + score_scope)
2. Write identity, convention, grid, prior parameters, resolution source, close_ts
3. Generate P_0(i,j) from λ or uniform; θ_{ij}=0
4. Open contract templates (1X2 / handicap lines / totals lines / scores …)
5. Open this match’s Risk Auction order book (with no LP quotes, Coverage starts at 0)
7. Delegate the score grid to the ER
8. State → TRADING; frontend shows current prices for each template
```

At open, each line’s price is the sum of $P_0$ on the corresponding set, e.g.:

$$
p_{\text{home}}=\sum_{i>j}P_0(i,j),\quad
p_{\text{over 2.5}}=\sum_{i+j\ge 3}P_0(i,j),\quad
p_{1-0}=P_0(1,0)
$$

Thereafter it changes only with fills; $\lambda$ is not rewritten.

#### 4.6.5 Trading Period (pre-match + in-play)

- The user picks an already-open contract (or a custom $S$), enters $q$, and pays $C_S(q)$
- The ER updates $\theta_{ij}\leftarrow\theta_{ij}+q$ on cells in $S$, recomputes the whole $P$, and refreshes every line quote together
- Update $E(i,j)$, $L_{\max}$, and coverage; do not reject for insufficient funds
- After kickoff, trading continues until `close_ts` (full-time whistle). Live score and time remaining are display only and do not automatically rewrite the prior
- After `now >= close_ts`, reject new orders and Commit back to L1

#### 4.6.6 Full Time and Settlement

1. After official full time, the Resolver reports $(x^*,y^*)$ (regulation score if `score_scope=REGULATION_90`)
2. After the challenge window, finalize; overflow maps into the $k_{\max}$ bucket
3. Every position with $S\ni(x^*,y^*)$ counts toward $L=E(x^*,y^*)$
4. Compute one $\rho$ from this match’s $C_{\max}$; winners on 1X2, handicap, totals, and correct score are paid at the same ratio
5. Profit share to this match’s Risk LPs only after $\rho=1$ and reserves meet the bar

Example: final 2-1. Home, home $-0.5$, over 2.5, BTTS, and correct score 2-1 all hit; draw, away, under 2.5, and 1-0 miss. Only the former are paid; if money is short, those winners share one $\rho$.

#### 4.6.7 Void

On any of the following, enter `VOID`, refund unsettled funds, unlock Risk LPs, and do not compute $\rho$:

- The match is abandoned or cancelled and is not completed inside the agreed window
- Home/away swap or eligibility change makes the underlying invalid
- The resolution source fails and the backup still has no valid score (take the `RESOLUTION_FAILED` refund path)

Convention disputes (whether a stoppage-time goal counts) are resolved only through challenge / committee. `score_scope` is not rewritten after settlement.

#### 4.6.8 Creation Example

```text
match:            EPL 2026-04-12  Arsenal vs Chelsea
score_scope:      REGULATION_90
k_max:            10
prior:            independent_poisson
lambda_home:      1.70
lambda_away:      1.15
beta:             (configured for target depth)
templates:        1X2, AH [-0.5,-0.75,-1,-1.5], OU [1.5,2.5,3.5], BTTS, CS
resolution:       committee (3/5) or sports_feed + challenge
close_ts:         regulation full-time whistle
risk_lock_ts:     close_ts
```

After open, users see multiple lines; accounts and risk see the same `market_id`.

### 4.7 How Each Market Type Is Created (Master Table)

These four types differ from football not only in lines and resolution source, but in **the probability distribution itself**. Do not put a football score grid on CPI, and do not treat an election’s $K$ atoms as a Gaussian curve to integrate.

What is shared is only the outer protocol: LMSR rewrites $\theta$, one pot of funds per board, $L$ at expiry from the realized outcome, and the same $\rho$ if funds fall short. Inner state space, $f_0$ family, and normalization fork by underlying.

#### 4.7.1 Distribution Family Map

| Type | Outcome random variable | State space | $f_0$ / $P_0$ | What trading rewrites | Details |
| --- | --- | --- | --- | --- | --- |
| Football | Score $(X,Y)\in\mathbb{N}^2$ | 2D discrete grid | Independent Poisson; margin is Skellam | $\theta_{ij}$, $(k_{\max}+1)^2$ of them | 4.6 |
| CPI / macro | Continuous (or fine-grid) scalar $X\in\mathbb{R}$ | 1D interval $\Omega$ | Gaussian $\mathcal{N}(\mu,\sigma^2)$ | 1D $\theta(x)$ | 4.8 |
| Election winner | Categorical $C\in\{1,\ldots,K\}$ | $K$ mutually exclusive atoms | Dirichlet → Categorical | $K$-dimensional $\theta_i$ | 4.9 |
| Election top $n$ | $n$-person combinations | $\binom{K}{n}$ atoms | Dirichlet → Categorical | $\theta$ on combinations | 4.9 |
| Election vote share | Share vector $\sum s_i=1$ | Simplex grid | Dirichlet | $\theta$ on share regions | 4.9 |
| BTC daily price | Positive price $X>0$ | 1D (log axis common) | Lognormal, $\log X\sim\mathcal{N}(\mu,\sigma^2)$ | 1D $\theta(x)$ | 4.10 |
| Occurs by deadline | Binary $B\in\{\mathrm{YES},\mathrm{NO}\}$ | 2 atoms | Binary Dirichlet / Bernoulli | $\theta_{\mathrm{YES}},\theta_{\mathrm{NO}}$ | 4.11 |

Football is a **two-dimensional joint count distribution**. CPI is a **one-dimensional continuous density**. Elections are a **categorical distribution on the simplex**. BTC is also one-dimensional continuous, but supported on the positive half-line; the prior is lognormal, not a plain Gaussian (price cannot be negative; vol is approximately proportional). A binary event is the $K=2$ degeneration of an election, not “squash a CPI interval into two bins”.

Therefore market creation must first pick the correct `prior_family` and state container:

- Gaussian / lognormal: continuous grid; buys are interval integrals
- Dirichlet: discrete atoms; buys are a point or a union of points, with $\sum p_i=1$
- Poisson / Skellam: 2D score table; buys are cell sets; 1X2 is a projection of the joint, not a separately stored categorical

After initialization, trading still only changes $\theta$. **It will not turn a Gaussian board into a Dirichlet board.** The family is locked at creation.

**On-chain create is by distribution family, not by listing name.** CPI, an election, a BTC board, and a deadline event are metadata (`topic` / `tag`) on the matching family instruction. Implementers SHALL NOT add `create_macro_market` / `create_election_market` / `create_price_market` as extra program entrypoints.

| Family | Instruction | Listing examples (metadata only) | Finalization |
| --- | --- | --- | --- |
| Skellam / 2D score | `create_skellam_market` | One football `score_scope` | Score pair $(x^*,y^*)$ |
| Gaussian | `create_gaussian_market` | CPI / macro print | Official first print |
| Lognormal | `create_lognormal_market` | Same-day BTC / ETH price | Committee price by `price_rule` |
| Dirichlet | `create_dirichlet_market` | Election winner, `TOP_N`, vote share (`layout`) | Winner / top-$n$ set / share vector |
| Bernoulli | `create_bernoulli_market` | Deadline YES/NO | YES or NO |

### 4.8 How a CPI / Macro Numeric Market Is Created

The distribution is a **one-dimensional Gaussian** (truncated on $\Omega$), not football’s 2D Poisson and not an election categorical. One board corresponds to one official release under one convention, e.g. “US March 2026 CPI year-over-year”. Users trade which interval $X$ falls into.

#### 4.8.1 Creation Parameters

A CPI / macro listing calls `create_gaussian_market` and writes:

| Field | Description | Rule |
| --- | --- | --- |
| `series_id` | Indicator: `US_CPI_YOY` / `US_CPI_MOM` / `CN_CPI_YOY`, etc. | Required |
| `release_id` | Which vintage, e.g. `2026-03` | Required |
| `unit` | Percentage points or index points | Match the official series |
| `print_rule` | `FIRST_PRINT` | Only the first official print; later revisions do not change settlement |
| `seasonal` | Seasonally adjusted / not | Must be bound to `series_id`; no ambiguity |
| `x_min` / `x_max` | Domain, e.g. YoY $[-2\%,12\%]$ | Creator-specified |
| `n_grid` | Sample count on $\Omega$ | **$256$ default** (why 256 vs 512 / 1024: §5.1.1). Create + `grow_grid` + chunked `write_grid_mass` / `seal_grid`; one Solana account cannot hold 256 points in the first `create_account`, and one ix cannot compute 256 $\exp$ masses |
| `prior_family` | `normal` / `uniform` | `normal` |
| `mu` / `sigma` | Prior mean and std (percentage points) | Survey median can be $\mu$, survey dispersion $\sigma$ |
| `beta` | LMSR liquidity | Required |
| `resolution_source` | `committee` | Official agencies are not on-chain; the committee transcribes |
| `source_url` | Specified BLS / statistics-bureau release page | Locked into the rules |
| `close_ts` | Usually before the official release time | Cut off before the print |
| `report_window` | Post-print report window | Hours to 1 day |

Acceptance for these fields is SRS §4.1.1 / FR-MKT-13–22: parameters are percentage points (not cell indices), encoded as Q64 thousandths, $n_{\mathrm{grid}}=256$, $\mu$ = survey median, $\sigma$ = survey dispersion.

Prior (truncated and renormalized on $\Omega$):

$$
f_0(x)=\mathcal{N}(\mu,\sigma^2)
$$

#### 4.8.2 What Users Buy

The underlying is a one-dimensional continuous PDF. Preset contracts are only interval templates. Each template is snapped to listing **nodes** with §8.1.2.1; the ticket is the bitmask $S$, not $\int_a^b f$.

| Contract | Set |
| --- | --- |
| Custom interval | $I=[a,b]$ → inclusive nearest-node range |
| Preset bins | e.g. $<2$, $[2,2.5)$, $[2.5,3)$, $\ge 3$ (same snap) |
| Above / below $k$ | $(k,x_{\max}]$ / $[x_{\min},k]$ (same snap) |

Do not open a separate independent board for “will it print above 2.5%”; that is just one interval on this CPI board. If it must share liquidity with “the exact YoY print”, it must live on the same `market_id`.

#### 4.8.3 Listing Steps

```text
1. Only one board per series_id + release_id + print_rule
2. Write convention, Ω, grid, N(μ,σ), committee, close_ts
3. Generate f_0, θ_k = 0
4. Open interval templates
5. Open the Risk Auction order book
6. Delegate → TRADING
```

#### 4.8.4 Finalization and Void

The committee reports a scalar $x^*$ (the official first print), mapped to the nearest grid. A convention error (YoY reported as MoM, SA reported as NSA) is an invalid report; after challenge, re-report. An official delay extends `report_window`; do not settle on a forecast. If that vintage is cancelled, `VOID`.

Creation example:

```text
series:     US_CPI_YOY
release:    2026-03
print:      FIRST_PRINT
Ω:          [-2%, 12%]
prior:      N(2.4, 0.35)
templates:  custom intervals + 0.5pct bins + above 2% / 2.5% / 3%
close_ts:   before the official release time
resolution: committee, specified BLS press release
```

### 4.9 How an Election Market Is Created

The distribution is **Categorical under a Dirichlet prior**. State lives on a $K$-dimensional probability simplex. There is no “goal count” and no continuous density $f(x)$. One board corresponds to one election under one winner rule; the default question is “who wins”.

#### 4.9.1 Creation Parameters

An election winner / `TOP_N` listing calls `create_dirichlet_market` (`layout` = atoms or top-$n$ combinations) and writes:

| Field | Description | Rule |
| --- | --- | --- |
| `election_id` | Election identity, e.g. `US_2028_PRES` | Required |
| `contest_rule` | `PLURALITY` / `ABSOLUTE_MAJORITY` / `ELECTORAL_COLLEGE` / `TOP_N` | Locked; `TOP_N` must also supply `n` |
| `candidates[]` | Candidate id, name; last item recommended as `OTHER` | $K\ge 2$ |
| `alpha[]` | Dirichlet prior pseudo-counts | All $1$ when uninformative |
| `beta` | LMSR liquidity | Required |
| `resolution_source` | `committee` | Certified result, not a poll |
| `cert_source` | Specified certifying body / official page | Locked into the rules |
| `close_ts` | Voting deadline or before official counting starts | Creator-specified |
| `dropout_policy` | `MERGE_OTHER` | A dropout merges into `OTHER`; the whole board is not voided |

$$
p_{0,i}=\frac{\alpha_i}{\sum_k\alpha_k},\qquad \sum_i p_{0,i}=1
$$

`contest_rule` decides who $x^*$ is: plurality, must exceed half, or Electoral College winner. Polls and media “called the race” must not replace certification, unless the rules explicitly say “first Associated Press call” — that must be written and locked at creation.

#### 4.9.2 What Users Buy

| Contract | Set |
| --- | --- |
| Single winner | Atom $\{i\}$ (when `contest_rule` is a winner type) |
| Party / camp | Union of several candidates |
| Enters top $n$ | `contest_rule=TOP_N`: atoms are all $n$-person combinations; buying “A in top $n$” = union of combinations that contain A |

`TOP_N` and the winner board are two boards (same `election_id`, different `contest_rule`), because “enters the top two” atoms are combinations, not mutually exclusive single persons. E.g. 4 people, top 2: $\binom{4}{2}=6$ atoms. The committee reports the set of ids that enter the top $n$, mapped to a unique atom.

Do not make “A wins” and “B wins” two unrelated YES/NO boards, or $P(A)+P(B)$ can exceed 1. One winner board shares one simplex.

**Vote share** is the same family: `create_dirichlet_market` with `layout` = simplex. Candidate shares $(s_1,\ldots,s_K)$, $\sum s_i=1$, outcome space a grid on the simplex, $n=C(\mathrm{bins}+k-1,k-1)$. If `Grid::space(n)` exceeds $10\,240$ bytes, create writes empty $P_0$ and `write_grid_mass` streams unnormalized masses (`PRIOR_CHUNK=256`, `simplex_chunk_raw`); $k$ SHALL be $\le4$. Dirichlet atoms over that cap SHALL reject. Buying “A’s share $\in[a,b]$” is that band. Winner-board and vote-share-board funds are independent; both resolve from the same certified tally. The winner is derived from shares via `contest_rule`; the vote-share board pays the shares themselves. Both boards must be fully implemented; they are not half-finished versions of one board. They are **not** a fourth on-chain family.

#### 4.9.3 Listing Steps

```text
1. Only one board per election_id + contest_rule (winner and TOP_N may coexist)
2. Write candidates, alpha, rule, committee
3. p_0 = normalize(alpha), θ_i = 0
4. Open single / camp / combination templates
5. Open the Risk Auction order book
6. Delegate → TRADING
```

#### 4.9.4 Finalization and Void

Winner board: the committee reports `winner_id` (an id on the list, or `OTHER`). `TOP_N` board: report exactly $n$ ids, mapped to the corresponding combination atom. Vote-share board: report a share vector or raw votes (then normalize). The challenge window checks the certified result. Candidate dropout: by `dropout_policy`, merge that atom’s unsettled positions into `OTHER` or refund by rule; others keep trading. Election cancelled, or the result is indefinitely invalid: `VOID`.

Creation example:

```text
election:    US_2028_PRES
rule:        ELECTORAL_COLLEGE
candidates:  [A, B, OTHER]
alpha:       [3, 3, 1]
close_ts:    election day 12:00 UTC
resolution:  committee, Congress certification / specified wire service (pick one and lock at creation)
```

### 4.10 How a Same-Day BTC Price Market Is Created

The distribution is **one-dimensional lognormal** ($\log X\sim\mathcal{N}(\mu,\sigma^2)$). It is in the same continuous family as CPI, but supported on $X>0$; a log axis is recommended for the grid. Do not use a football score table, and do not use a symmetric Gaussian that can generate negative prices. One board corresponds to one price convention at one timestamp, e.g. “BTC/USD at 2026-04-12 00:00 UTC”. Settlement is the committee reporting the price under that convention. The contract stores the number; it does not fetch or recompute a feed.

#### 4.10.1 Creation Parameters

A same-day price listing calls `create_lognormal_market` and writes:

| Field | Description | Rule |
| --- | --- | --- |
| `symbol` | `BTC-USD` / `ETH-USD`, etc. | Required |
| `observe_ts` | Observation timestamp | Required, including timezone |
| `price_rule` | Human-readable, verifiable pricing convention | E.g. Coinbase last at `observe_ts` |
| `twap_window` | TWAP only, e.g. 300s before close | Optional |
| `x_min` / `x_max` | Price domain, e.g. $[10k,250k]$ | Must cover a reasonable tail |
| `n_grid` | Grid count | $256\sim 1024$ |
| `grid_space` | `linear` / `log` | Price boards should use `log` |
| `prior_family` | `normal` / `lognormal` / `uniform` | `lognormal` |
| `mu` / `sigma` | On $x$ or $\log x$ | Spot at listing time can be $\mu$ |
| `beta` | LMSR liquidity | Required |
| `resolution_source` | `committee` | Required |
| `close_ts` | Defaults to `observe_ts` | Observation is the cutoff |

Lognormal prior:

$$
\log X\sim\mathcal{N}(\mu,\sigma^2)
$$

Then truncate on $[x_{\min},x_{\max}]$, project onto the log grid, and renormalize.

#### 4.10.2 What Users Buy

| Contract | Set |
| --- | --- |
| Custom price interval | $[a,b]$ |
| Above / below $K$ | $(K,x_{\max}]$ / $[x_{\min},K]$ |
| Preset bins | e.g. every $5k$ or log-equal-width bins |

“Will BTC be above 100k that day” should not open a separate binary board unless the intent is *not* to share liquidity with the interval board. The default is one-sided interval on this price board.

#### 4.10.3 Listing Steps

```text
1. Only one board per symbol + observe_ts + price_rule
2. Write Ω, grid, prior, price_rule, committee
3. Generate f_0, θ = 0
4. Open interval / above-K templates
5. Open the Risk Auction order book
6. Delegate → TRADING; `close_ts` observes the price and **stops trading** on this prediction market
```

#### 4.10.4 Finalization and Void

Inside the report window, the committee (or an authorized reporter) calls `submit_result(price)` using the `price_rule` locked at listing. After clamp onto $\Omega$, drop into the nearest grid. Disputes follow the convention written at listing, not someone’s screen price.

Creation example:

```text
symbol:      BTC-USD
observe_ts:  2026-04-12 00:00:00 UTC
price_rule:  SPOT
Ω:           [10_000, 250_000]
grid:        log, N=512
prior:       lognormal(μ=log(spot), σ=0.25)   # widen σ with time to expiry
templates:   custom intervals + above 80k/100k/120k
price_rule:  Coinbase BTC-USD last at observe_ts
resolution:  committee
```

### 4.11 How a Deadline Event (Did It Happen) Market Is Created

The distribution is **Bernoulli / binary Dirichlet**: only $p$ and $1-p$. It is not slicing a CPI curve into two pieces, and it is not football 1X2 (1X2 is a projection of a joint score; there is no third “draw” here). One board corresponds to one locked proposition + one deadline.

#### 4.11.1 Creation Parameters

A deadline event listing calls `create_bernoulli_market` and writes:

| Field | Description | Rule |
| --- | --- | --- |
| `title` / `description` | Full proposition text | Required; semantics cannot change after creation |
| `yes_definition` | What fact counts as occurred | Must be verifiable |
| `deadline_ts` | Deadline timestamp | Required |
| `early_resolve` | May finalize early if it already occurred before the deadline | `true` |
| `evidence_urls` | Specified evidence sources | Recommended |
| `alpha_yes` / `alpha_no` | Prior pseudo-counts | `1, 1` (50 / 50) |
| `beta` | LMSR liquidity | Required |
| `resolution_source` | `committee` | Required |
| `close_ts` | Defaults to `deadline_ts` | Trading **stops** at `close_ts`. Verification and committee report happen **after** close. `close_ts` SHALL NOT be set after the event is known so as to allow trading on a known outcome. |

$$
p_{\mathrm{YES}}=\frac{\alpha_{\mathrm{YES}}}{\alpha_{\mathrm{YES}}+\alpha_{\mathrm{NO}}}
$$

The proposition must be a closed question that can be decided at the deadline. Counter-example: “will some project succeed” has no standard. Positive example: “by 2026-12-31 23:59 UTC, has the ETF application received formal SEC approval”.

#### 4.11.2 What Users Buy

| Contract | Meaning |
| --- | --- |
| YES | By the deadline (inclusive), the event has occurred as defined |
| NO | At the deadline it still has not occurred |

$p_{\mathrm{NO}}=1-p_{\mathrm{YES}}$. Buying YES in quantity $q$ is $\theta_{\mathrm{YES}}\leftarrow\theta_{\mathrm{YES}}+q$ on the YES atom. Do not split this into two independent boards.

If the same theme also has a numeric question (“first-day volume after approval”, “what is CPI”), list a 4.8 / 4.10 board separately. Do not stuff it into this YES/NO.

#### 4.11.3 Listing Steps

```text
1. Check the proposition, deadline, and evidence rules are complete
2. Write YES/NO atoms, prior, committee
3. Open prices = p_YES / p_NO
4. Open the Risk Auction order book
5. Delegate → TRADING
```

#### 4.11.4 Finalization and Void

- Before the deadline the committee confirms it occurred and `early_resolve=true`: immediately $x^*=\mathrm{YES}$, stop trading and settle
- At the deadline it has not occurred: $x^*=\mathrm{NO}$
- Contradictory evidence, or the definition is still undecidable: challenge / delay; if still impossible, `RESOLUTION_FAILED` or `VOID`
- Occurs only after the deadline: NO, no look-back

$L=E(x^*)$ lands on only one atom. $\rho$ is still computed from this board’s $C_{\max}$.

Creation example:

```text
proposition:  By 2026-12-31 23:59 UTC, a spot BTC ETF receives formal SEC approval
yes_if:       Specified SEC notice page shows final approval
deadline:     2026-12-31 23:59 UTC
early_resolve: true
prior:        50 / 50
resolution:   committee
```

---

## 5. Engineering Representation of the PDF

The chain does not integrate an arbitrary continuous curve. It places **nodes** on $\Omega$ and treats each node as one LMSR atom. $\theta$ is constant on that atom. Buying $[a,b]$ never cuts an atom by length (algorithm: §8.1.2.1).

### 5.1 Step PDF / Grid

1. Place $n$ nodes on $\Omega=[x_{\min},x_{\max}]$ (Gaussian default $n=256$, bound $8..1024$):

$$
x_i=x_{\min}+i\cdot\frac{x_{\max}-x_{\min}}{n-1},\qquad i=0,\ldots,n-1.
$$

   Lognormal uses the same formula on $\log x$. Prior mass $p_{0,i}$ is the truncated kernel **at node** $x_i$, then $\ell_1$-renormalized. This is not a Riemann integral of $f_0$ over a bin.

2. Atom $i$ owns the Voronoi band between adjacent midpoints (the first node also owns down to $x_{\min}$, the last up to $x_{\max}$). Any print in that band maps to $i$ via nearest node.

3. The partition function is a finite sum over nodes:

$$
Z[\theta]=\sum_{i=0}^{n-1}p_{0,i}\,e^{\theta_i/\beta},\qquad
p_i=\frac{p_{0,i}\,e^{\theta_i/\beta}}{Z[\theta]}.
$$

When a user draws $[a,b]$, snap both ends with `interval_index`, take the inclusive node range $S$, then add $q$ to $\theta_k$ and $E_k$ for every $k\in S$. Complexity is $O(|S|)$ on the locked shards, acceptable inside an Ephemeral Rollup.

#### 5.1.1 \(n=256/512/1024\) 格子够不够（锁定）

产品高斯默认 **\(n=256\)**。\(512\)、\(1024\) 更细，但不是因为 \(256\)「不够用」才要上。两端最多偏半步；规格要警告的是 \(n<32\)（步长大于 \(\sigma\)，先验塌到 \(1\)–\(2\) 格），不是 \(256\)。

以 CPI 参考盘 \(\Omega=[-2,12]\)、\(\sigma=0.35\) 为例（宽度 \(14\) 百分点）：

| \(n\) | 步长 \((x_{\max}-x_{\min})/(n-1)\) | 和 \(\sigma\) 比 | 什么时候用 |
| --- | --- | --- | --- |
| \(<32\) | \(n=32\) 时 \(\approx 0.45>\sigma\) | 先验塌掉 | 拒绝或强警告（SRS FR-MKT-15） |
| **\(256\)（默认）** | \(\approx 0.055\) | 一步远小于 \(\sigma\)，约 \(6\) 个节点盖住一个 \(\sigma\)；半步误差 \(\approx 0.027\) | CPI / 宏观默认。开通预测市场写这个 |
| \(512\) | \(\approx 0.027\) | 再细一倍 | 用户经常买极窄区间，或 \(\Omega\) 很宽而 \(\sigma\) 很小 |
| \(1024\)（上限） | \(\approx 0.014\) | 更细 | 同上，且分片更多、成交/收尾更慢 |

MoM 一类更窄的 \(\Omega\)（例如 \([-1,2]\)、\(\sigma=0.15\)）在 \(n=256\) 上步长 \(\approx 0.012\)，仍然小于 \(\sigma\)。只有「区间宽度接近步长」时才需要加密。链上硬范围是 \(8..1024\)；\(8\) 是账户下限，**不是**给用户开的高斯默认。

### 5.2 No Parameterized AMM

One could assume $f(x)$ always belongs to a parametric family, e.g. $\mathcal{N}(\mu,\sigma^2)$. Trades would no longer accumulate step $\theta_k$, but would update $(\mu,\sigma)$.

This protocol **does not** use a parameterized AMM. State is always $\theta$ on a grid or on discrete atoms, freely shaped by trading. A parameterized book can only lock the distribution on $(\mu,\sigma)$ or $(\lambda_H,\lambda_A)$; it cannot express multimodality or irregular shapes, and it is not built.

### 5.3 Fixed-Point Arithmetic

ER / on-chain computation uses fixed point (e.g. Q64.64). $\exp/\ln$ use lookup tables plus polynomial fits; no floats. Settlement amounts round down; dust remainder enters the reserve so that the sum actually paid never exceeds $C_{\max}$.

---

## 6. Risk Capital

### 6.1 Why Risk Capital Is Still Needed

Even after a hard $L_{\max}$ reject is no longer a condition, the market still needs external capital to absorb the tail. Otherwise every shortfall becomes a user haircut, the prediction market degenerates into “a lottery that may fail to pay”, and probability discovery is polluted by payout risk.

Risk capital’s role is to:

- Raise coverage so $\rho$ stays as close to 1 as possible
- Price tail risk
- Lock “who loses at most how much” on collateral in advance

### 6.2 Risk Layers

A layer is defined as $[A,A+D]$:

- $A$: Attachment, the point at which underwriting starts after the market’s retention
- $D$: Capacity of that layer

Risk LP payout under realized loss $L$:

$$
H_{A,D}(L)=\min\bigl(\max(L-A,0),D\bigr)
$$

Example: market retains $0\rightarrow 2M$, layer A is $2M\rightarrow 5M$, layer B is $5M\rightarrow 10M$.

$$
H_A(L)=\min((L-2M)^+,3M),\qquad
H_B(L)=\min((L-5M)^+,5M)
$$

Total risk-capital payout:

$$
H_{\text{Risk}}(L)=\sum_i H_i(L)
$$

Single Risk LP profit:

$$
\Pi_i=\text{Premium}_i-H_i(L)+\alpha_i S
$$

where $S$ is residual profit and exists only after users have been paid in full. See section 9.

### 6.3 Quotes and Auction

A Risk LP submits:

```text
Capacity / Attachment / Detachment / Premium / Collateral / Profit Share
```

Constraints:

- $\text{Collateral}_i\ge D_i$
- A single LP’s share does not exceed $\gamma$ (e.g. 10%), to limit concentration
- Collateral must be locked in advance; “promised 10M, account holds 1M” is not allowed

The coverage the market needs is $D_{\text{required}}$. The auction selects a set of quotes so that $\sum D_i\ge D_{\text{required}}$ (fill what can be filled; a shortfall does not block trading) and minimizes:

$$
\sum_i\text{Premium}_i+\lambda_1\text{Concentration}+\lambda_2\text{Counterparty}+\lambda_3\text{Liquidity}
$$

Fill rule: each layer is filled from lowest unit premium to highest, subject to capacity and concentration $\gamma$. See 6.6.5.

Economic meaning of Risk Premium:

$$
\text{Premium}\approx \mathbb{E}[H(L)]+\text{risk loading}+\text{cost of capital}+\text{liquidity premium}+\text{counterparty premium}
$$

A Risk LP may choose:

- High-probability, small-payoff shallow layers
- Low-probability, high-payoff deep tail

### 6.4 Multiple Markets

The system may host BTC, ETH, CPI, gold, EUR/USD, and other prediction markets at the same time. A Risk LP may:

- Underwrite only a specified layer of a single market

One unit of collateral must not underwrite multiple boards at once. Cross-board combination underwriting is not built.

Capacity must not be freely reallocated across different risk regions. Maintain $C_R(I)$ by region, not one global number that can be moved at will.

### 6.5 Relationship Between Risk Capital and the PDF

The prediction market supplies $f(x)$, from which one can compute the payout distribution $P(L)$, $\mathbb{E}[L]$, $\mathrm{VaR}_\alpha(L)$, $\mathrm{CVaR}_\alpha(L)$, and then a suggested coverage amount. The chain is:

$$
\text{PDF}\rightarrow\text{Liability Distribution}\rightarrow\text{Tail Risk}\rightarrow\text{Risk Capital Requirement}
$$

This is a **suggested value** for auction and display, not a reject threshold.

### 6.6 How the Risk Capital Auction Works

The principle is locked: the auction **tops up coverage**; it does not decide whether an order may be placed. The following is the business order. Football / CPI / elections / daily price / binary events share this book — a Risk LP underwrites this board’s scalar liability $L$, regardless of whether the underside is a score table or a Gaussian curve.

#### 6.6.1 One Board, One Auction

Each `market_id` carries its own risk order book and one Vault. Football 1X2 and totals do not split the auction; CPI bins do not split it either. One unit of collateral must not carry multiple boards at once.

```text
Trader buys and sells on the prediction board
        │
        │  Exposure → L_max, suggested D_required
        ▼
This board’s Risk Auction (continuously open)
        │
        │  LP quotes and locks collateral
        ▼
C_R increases → coverage rises → enters C_max at settlement
```

#### 6.6.2 When It Opens and When It Closes

| Moment | Action |
| --- | --- |
| After the reviewer opens the prediction market | Risk auction opens, even if there are not yet any prediction fills |
| While the prediction board is TRADING and `now < risk_lock_ts` | Quotes are accepted continuously; more fills raise $L_{\max}$ and the suggested size |
| `risk_lock_ts` if earlier than `close_ts` | Auction stops; prediction trading may continue until `close_ts` |
| Prediction-board `close_ts` | Prediction fills **and** the auction stop. Already-locked $C_R$ freezes. |
| Settlement or VOID | Draw or return collateral by layer |

`risk_lock_ts` is a required field at listing and SHALL satisfy `risk_lock_ts \le close_ts`. Default is equality. After `close_ts` there is no “keep topping up until report” window. There is no “extend later” exception.

#### 6.6.3 Layers Published by the Protocol, Not Freely Drawn by LPs

At listing, published layers start at 0. Example:

```text
Layer 1   0          → D_unit
Layer 2   D_unit     → 2 D_unit
Layer 3   2 D_unit   → 3 D_unit
...
```

$D_{\mathrm{unit}}$ is specified at creation (e.g. 10,000 USDC). LPs may only quote published layers. They cannot invent their own Attachment, which would overlap or misalign layers.

Suggested demand (display only, not a reject):

$$
D_{\mathrm{required}}=L_{\max}
$$

Coverage:

$$
\mathrm{Coverage}=\frac{C_R}{L_{\max}}
$$

How $L_{\max}$ is computed varies by family; the auction only consumes this scalar:

| Market | $L_{\max}$ |
| --- | --- |
| Football | $\max_{i,j}E(i,j)$ |
| CPI / BTC | $\max_k E_k$ |
| Election | $\max_i E_i$ |
| Binary event | $\max(E_{\mathrm{YES}},E_{\mathrm{NO}})$ |

#### 6.6.4 How an LP Quotes

Submit against a given layer:

| Field | Meaning |
| --- | --- |
| `layer_id` | Which layer to buy |
| `capacity` $D_i$ | Maximum payout on that layer |
| `premium` | Required premium (absolute amount, or a rate on $D_i$) |
| `profit_share` $\alpha_i$ | Optional residual profit share |
| `collateral` | $\ge D_i$; locked from the LP account into this board’s Risk Vault at submit |

A quote that is not fully locked is invalid. The same LP’s already-filled capacity on this board is $\le \gamma C_R^{\mathrm{target}}$ (default $\gamma=10\%$; if $C_R$ is still tiny, use $D_{\mathrm{required}}$).

#### 6.6.5 How Fills Happen

Each layer queues separately and is filled from lowest **unit premium** to highest:

1. Remaining unfilled size on that layer: $D_{\mathrm{layer}}^{\mathrm{remain}}$
2. Take the cheapest valid quote and fill $\min(D_i,D_{\mathrm{layer}}^{\mathrm{remain}})$
3. Premium is paid in proportion to filled capacity: from this board’s Vault (realized trading revenue) into “premium payable”. Accepting a quote makes it a protocol debt immediately; at settlement it is a $R_{\text{net}}$ deduction
4. Collateral stays locked until settlement or VOID
5. After the layer is full, more expensive quotes stay on the book or are cancelled

Fill what can be filled. If every layer is empty, the prediction board still trades; Coverage is low and the frontend shows a strong warning.

#### 6.6.6 Premium, Payout, Profit Share

| Case | LP outcome |
| --- | --- |
| Settlement $L\le A_i$ | Layer not hit; LP keeps full Premium; collateral unlocked |
| $A_i<L\le A_i+D_i$ | Draw $L-A_i$; remaining collateral returned |
| $L>A_i+D_i$ | Draw the full $D_i$ |
| User-side $\rho<1$ | LP still pays only by the layer formula, no top-up; no Profit Share |
| $\rho=1$ and $S>0$ | Then split residual by $\alpha_i$ |
| VOID / finalization failure | Return collateral; unused premium returns to this board (see 10.5) |

An LP’s liability cap is always its own $D_i$. Later traders lifting $L_{\max}$ do not rewrite already-filled layer definitions.

#### 6.6.7 Relation to the Five Prediction Types

The auction does not know Gaussian from Dirichlet. Listing opens this order book:

- Football: underwrites the peak of the whole-match $E(i,j)$, not “home only”
- CPI / BTC: underwrites the 1D grid peak
- Election / binary: underwrites maximum atom exposure (if someone buys one candidate very deep, $L_{\max}$ is that atom)

What a Risk LP sees on the board: layers, rates, current $L_{\max}$, an estimate of $P(L>A)$ (from the current $f$ or $P$), and coverage. They do not need to maintain their own PDF.

#### 6.6.8 Business Loop (one board)

```text
List → open layers 1..n
   → prediction-side trading (does not check C_R)
   → LPs may quote, lock collateral, get filled
   → close / risk_lock
   → finalize x* (or score / YES)
   → L = E(x*)
   → R_net first, then C_R by leftover shortfall, then C_P^alloc
   → ρ = min(1, C_max / L)
   → pay users at ρ → residual only then is shared
```

---

## 7. Solvency Model (v1.1)

### 7.1 Principle No Longer Used as a Hard Constraint

v1.0 required, before accepting an order:

$$
L'_{\max}=\sup_x\bigl(E(x)+q\,1_I(x)\bigr)
$$

If not met: reject, partial fill, or open an auction first.

v1.1 **removes that hard constraint**. Reasons:

- $L_{\max}$ is only theoretical liability on the worst cell; true settlement pays only $E(x^*)$
- Gating all trading on the worst case over-restricts market size
- Risk capital can keep topping up during trading; it need not be complete before every order

### 7.2 Order-Acceptance Rules

After a user submits $q\cdot 1_I(x)$, the protocol still computes the new $E'(x)$ and $L'_{\max}$, but the uses become:

- Update coverage
- Signal to trigger the risk auction / raise premium
- Strong frontend warning

**Do not reject a fill because $L'_{\max}$ exceeds locked risk capital.**

Rejections are limited to ordinary trade checks: insufficient balance, interval out of bounds, illegal quantity, market already closed, session unauthorized, etc.

### 7.3 What to Do When Risk Capital Is Short

When suggested coverage exceeds already-locked coverage, in priority order:

1. Raise Risk Premium to attract new Risk LPs
2. Start or continue the Risk Auction
3. Fill as usual and lower public coverage
4. Stop treating Reject as a solvency tool

The only remaining hard rule:

> At any moment, the upper bound a user can be guaranteed never exceeds payable capital already locked at that moment. Anything above that was never “guaranteed payout”; it is face value that $\rho$ may haircut.

---

## 8. Settlement Payout and Haircut Ratio

### 8.1 Core calculation rules (normative)

These identities are the settlement kernel. Programs, Quote, WASM, and the CLI SHALL use `crates/math` (`implied_probs`, `ticket_face`, `skellam_masks`, `outcome_cell`). A second implementation is forbidden.

#### 8.1.0 One share

| | At buy | At settlement if $c\in S$ | If $c\notin S$ |
| --- | --- | --- | --- |
| User pays | $C_S(q)+\phi C_S(q)$ USDC | — | — |
| User receives | $q$ shares on frozen $S$ | $\lfloor\rho\cdot\mathrm{face}\rfloor$ USDC, ordinary $\mathrm{face}=q$ | $0$ |
| Quote | $p_S=\sum_{k\in S}p_k$ (pure probability) | $p$ is frozen; not used to pay | — |

Worked CPI ticket: $S=$ cells for print in $[0.3,0.4]$, $p_S=0.7$, user buys $q=10$ shares.

- Cash out: $C_S(10)+\phi C_S(10)$. If $q/\beta$ is small, $C_S(10)\approx 7$ USDC, **not** “10 shares × 1 USDC now”.
- Position: $10$ shares on $S$ (the $0.7$ does not scale the share count).
- Official print $0.35$ → $c\in S$ → face $10$. If $\rho=1$, pay $10$ USDC. If $\rho=0.8$, pay $8$ USDC.
- Official print $0.50$ → miss → pay $0$.

$p_S$ is how much **one infinitesimal share costs**. Face is how much **one share pays if it hits**. Those are different numbers except in the degenerate case $p_S=1$. The cash multiple $\approx 1/p_S>1$ is display / intuition only: it SHALL NOT rescale $q$ or the hit payout. Vocabulary lock: product §1.2.2.

The same arithmetic applies to every family and every template (only the definition of $S$ and of $c$ changes):

| Board / line | $S$ | Hit when | Tiny 1-share cost | $\rho=1$ payout for $q$ shares |
| --- | --- | --- | --- | --- |
| CPI / Gaussian interval $[a,b]$ | inclusive nearest-node range (§8.1.2.1) | $i^*(x^*)\in S$ | $\approx p_S$ | $q$ USDC |
| Lognormal / BTC interval | same on the log grid | $i^*(\log x^*)\in S$ | $\approx p_S$ | $q$ USDC |
| Bernoulli YES or NO | that one atom | YES or NO | $\approx p_{\mathrm{YES}}$ or $p_{\mathrm{NO}}$ | $q$ USDC |
| Dirichlet winner / TOP_N / share band | those atoms | reported atom $\in S$ | $\approx p_S$ | $q$ USDC |
| Football 1X2 / totals / BTTS / exact / half AH | `skellam_masks` | score cell $\in S$ | $\approx p_S$ | $q$ USDC |
| Football quarter AH | two half-line sets | 2 / 1 / 0 legs hit | $\approx C_{S_1}(\tfrac12)+C_{S_2}(\tfrac12)$ for $q=1$ | $q$ / $q/2$ / $0$ |
| Custom mask | the bits | $c\in S$ | $\approx p_S$ | $q$ USDC |

#### 8.1.1 Three objects (do not mix)

| Object | Symbol | Stored as | Meaning | Used for |
| --- | --- | --- | --- | --- |
| Implied PDF | $p_k$ | derived from `p0`, $\theta$, $\beta$ | Trading-implied probability | Quotes, heat map, $p_S$ |
| Face exposure | $E_k$ | `grid.exposure[k]` | Sum of ticket **faces** that include atom $k$ | $L$, $L_{\max}$, coverage |
| Ticket face | $\mathrm{face}_j$ | computed at claim | How many share-units this ticket is owed at $c$ | $\lfloor\rho\cdot\mathrm{face}_j\rfloor$ |

$p$ is a distribution: $\sum_k p_k=1$. $E$ is **not** a distribution. Buying $S$ raises both $\theta$ (hence $p_S$) and $E$ on $S$, but $p$ is renormalized and $E$ is not. Displaying $E$ as “the market PDF” is a product error.

Committee `submit_result` writes $x^*$ only. It SHALL NOT rewrite $p0$, $\theta$, or $E$.

#### 8.1.2 A buy is a frozen atom set

The UI may show a continuous interval $[a,b]$ or a named line (home, over 2.5). At fill the client/program maps that intent to a set $S$ of grid atoms:

- 1-D Gaussian / lognormal: bitmask from §8.1.2.1 (same `interval_index` / `outcome_cell` as settle)
- Dirichlet atoms / Bernoulli: bitmask of those atoms
- Football typed line: `skellam_masks(kind,a,b,k_{\max})` — one set, or two sets for a quarter line
- Custom union: `buy_set` bitmask

The position stores $(S,q)$ (or a typed key that expands to the same masks). Settlement SHALL NOT re-integrate $[a,b]$ and SHALL NOT invent a new $S$.

**Where the mask is used (same $S$ end to end).** The bitmask is the product ticket, not a storage trick:

| Step | What the user sees | What the system uses |
| --- | --- | --- |
| Select | PDF bars, or a typed line (胜 / Over 2.5 / YES) | Bits: cell $k$ is in $S$ iff bit $k=1$ |
| Quote | $C_S(q)$, $p_S$ | Sum / LMSR only over those bits |
| Fill | Buy set / Buy line | `buy_set` / `buy_skellam_set` carries the mask (or kind that expands to the same bits) |
| Ticket | One row in Portfolio | PDA seeds include `set_hash(mask)`. Same owner + market + $S$ = same position |
| Claim | 领取 | Payout must present the same mask (or recover it from the fill journal / typed Skellam). The chain stores the hash, not a redraw of $[a,b]$ |

A **full mask** is every bit 1 (buy the whole grid). That is a legal $S$, used in tests and if someone actually buys every atom. Everyday tickets are a contiguous node range or a typed-line set.

How one fill is *split across transactions* when $S$ touches too many shards is an implementation limit (technical architecture §6.3). Product-visible fact only: a very wide $S$ may take several confirmed transactions; after `close_ts`, bringing shards home to L1 is serial. Delay is allowed. The frozen $S$ does not change.

##### 8.1.2.1 1-D interval $\to$ node set (Gaussian / lognormal)

The drawn interval is **not** a length-weighted continuous contract. LMSR atoms are the listing **nodes** (product §5.1, SRS FR-MKT-15):

$$
x_i=x_{\min}+i\cdot\frac{x_{\max}-x_{\min}}{n-1},\qquad i=0,\ldots,n-1.
$$

Lognormal: the same formula on $\log x$ (then $X_i=e^{x_i}$). There is no half-atom: $a$ and $b$ inside one node's Voronoi band still buy that **whole** node.

**Nearest node** — `crates/math::outcome::interval_index`, identical to settle `outcome_cell`:

$$
i^*(x)=\mathrm{clamp}\Bigl(\mathrm{round}\bigl((x-x_{\min})\cdot(n-1)/(x_{\max}-x_{\min})\bigr),\;0,\;n-1\Bigr).
$$

Lognormal substitutes $\log x$, $\log x_{\min}$, $\log x_{\max}$. $x<x_{\min}$ clamps to $0$; $x>x_{\max}$ clamps to $n-1$. On-chain Q64 rounding is add $2^{-1}$ then take the integer part (half-integers go to the higher index). $n<2$ or $x_{\max}\le x_{\min}$ is illegal.

Node $i$ **owns** the band between adjacent midpoints: first node also owns $[x_{\min},\mathrm{mid}(x_0,x_1))$, last node owns $(\mathrm{mid}(x_{n-2},x_{n-1}),x_{\max}]$. A print anywhere in that band maps to $i$.

**Buy $[a,b]$** (require $a\le b$; if the UI sends them swapped, swap before indexing):

1. $i_a=i^*(a)$, $i_b=i^*(b)$. If $i_a>i_b$, swap.
2. $S=\{i_a,i_a+1,\ldots,i_b\}$ (inclusive). Empty $S$ SHALL be rejected.
3. Quote $p_S=\sum_{k\in S}p_k$, charge $C_S(q)$. Write $q$ onto $\theta_k$ and $E_k$ for every $k\in S$. Liability does not scale with $|S|$ — only one node realizes.
4. Freeze the bitmask on the position. Hit iff $c=i^*(x^*)\in S$.

Preset bins (`<2`, $[2,2.5)$, above $k$, …) use this same snap, then the inclusive node range. Quote, fill, and payout SHALL NOT compute $\int_a^b f(x)\,dx$ and SHALL NOT pay a fraction of $q$ because $[a,b]$ covers only part of a Voronoi band.

**Worked example.** Nodes $6.0,6.1,\ldots,10.0$ ($x_{\min}=6$, step $0.1$). User draws $[5.5,7.8]$:

| Endpoint | $i^*$ | Why |
| --- | --- | --- |
| $5.5$ | $0$ (node $6.0$) | below $\Omega$, clamp |
| $7.8$ | node $7.8$ | exactly on a node |

$S=\{6.0,6.1,\ldots,7.8\}$ (19 atoms). Print $5.9$ still maps to $6.0$ (hit). Print $7.85$ maps to $7.9$ (miss). The sliver $[5.5,6)$ is not a separate contract. If both ends sit in one band (e.g. $[6.12,6.18]$ around node $6.1$), $S$ is that singleton and the ticket is the whole atom.

#### 8.1.3 Fill update (every LMSR leg)

For each set $S_\ell$ written by the fill, with size $q_\ell$ ($q_\ell=q$ for an ordinary ticket; $q_\ell=q/2$ for each quarter leg):

$$
\theta_k\leftarrow\theta_k+q_\ell,\qquad E_k\leftarrow E_k+q_\ell \quad\text{for all }k\in S_\ell.
$$

Cost $C_{S}(q_\ell)$ uses the **current** $p_S$ (after earlier legs in the same quarter fill). Fee $\phi C$ is recorded separately and never enters $R_{\mathrm{net}}$ or $E$.

Writing $q_\ell$ onto **every** atom in $S_\ell$ does not multiply liability by $|S_\ell|$. Only one atom realizes. The write makes $E(c)$ already contain this ticket’s face no matter which atom in $S_\ell$ is $c$.

#### 8.1.4 Map $x^*$ to one atom

$$
c=\mathrm{cell}(x^*)
$$

| Family | $x^*$ reported as | $c$ |
| --- | --- | --- |
| Skellam | score $(i^*,j^*)$ | $(i^*\wedge k_{\max},\; j^*\wedge k_{\max})$ on the $11\times 11$ table |
| Gaussian | scalar | $i^*(x^*)$ (§8.1.2.1) |
| Lognormal | positive scalar | $i^*$ on the log grid (§8.1.2.1) |
| Dirichlet (atom / TOP_N) | winner or combination id | that atom index |
| Bernoulli | YES $=1$ / NO $=0$ | that atom |

Overflow scores share the $k_{\max}$ bucket (locked). Vote-share simplex vectors (`kind=3`) are not a single cell; they SHALL use a defined layout mapper or VOID — they SHALL NOT be forced through `outcome_cell` as if they were an atom id.

#### 8.1.5 Ticket face at $c$

$$
\mathrm{face}=\mathrm{ticket\_face}(q,\; n_{\mathrm{hit}},\; n_{\mathrm{parts}})
=q\cdot n_{\mathrm{hit}}/n_{\mathrm{parts}}
$$

(integer USDC; $q$ is the ticket’s share count).

| Ticket | $n_{\mathrm{parts}}$ | $n_{\mathrm{hit}}$ | face |
| --- | --- | --- | --- |
| Ordinary set / 1X2 / totals / half AH / exact / custom mask | $1$ | $1$ if $c\in S$, else $0$ | $q$ or $0$ |
| Quarter AH (two adjacent half-lines) | $2$ | how many of the two sets contain $c$ | $q$, $q/2$, or $0$ |

Integer AH push ($2(i-j)+\mathrm{halves}=0$) is **not** in $S$: face $0$. The LMSR cost already sits in the pot; there is no stake refund.

`payout` / `payout_skellam` SHALL pay $\lfloor\rho\cdot\mathrm{face}\rfloor$, not $\lfloor\rho\cdot q\rfloor$ on an OR of quarter masks. Fill and settle SHALL expand typed lines with the same `skellam_masks`.

#### 8.1.6 Winning-share total $L$

$$
L=E(c)=\sum_{\ell} q_\ell\,1_{S_\ell}(c)=\texttt{usdc}(\texttt{grid.exposure}[c])
$$

This **is** the sum of winning faces. Implementation reads the pre-aggregated cell; it SHALL NOT scan all position accounts to form $L$, and it SHALL NOT compute $\sum_k E_k$.

Overlapping tickets that contain $c$ all count (home $q=2$, over 2.5 $q=1$, exact 2-1 $q=1$, home $-0.75$ $q=2$ with only the $-0.5$ leg hitting 2-1 $\Rightarrow L=2+1+1+1=5$).

Identity that tests SHALL keep: after any mix of fills, $\sum_j \mathrm{face}_j(c)=E(c)$.

$$
L_{\max}=\sup_k E_k
$$

is display / auction only. $\rho$ uses $L$, not $L_{\max}$.

#### 8.1.7 Implied PDF (trading distribution)

$$
Z=\sum_k p0_k\,e^{\theta_k/\beta},\qquad
p_k=\frac{p0_k\,e^{\theta_k/\beta}}{Z},\qquad
p_S=\sum_{k\in S}p_k.
$$

This is the distribution “from trading data”. It is **not** a histogram of fill counts and **not** $E$. Inspect on-chain with `p0`+$\theta$+$\beta$ via `implied_probs` (CLI: `cpm market pdf` / `cpm market info`; Market API `GET /v1/markets/{id}/info`, `GET /v1/markets/{id}/pdf`, `GET /v1/markets/{id}/ws`). The **browser does not compute** LMSR. The indexer projects $\theta$ every $400\,\mathrm{ms}$ (floor $200\,\mathrm{ms}$); the WS ticks $250\,\mathrm{ms}$ and pushes $p_k$ on connect and whenever $\theta$ changes. `/m/[id]` subscribes; if the socket is down it polls `/info` every $2\,\mathrm{s}$. After close the chart freezes. Bars / the $11\times 11$ heat are $p_k$ relative to the peak; hover is percent; $E$ is face in the tooltip only; $n>32$ bins. The stamp is live / polling / frozen plus the snapshot slot (SRS FR-UI-39). The public desk also reports **traders** (distinct owners with $q>0$), **stake** ($\sum$ `position.cost_paid`), $L_{\max}=\sup_k E_k$, and $C_R$ (locked+filled risk capital) so a new ticket can see depth and coverage before it pays $p_S$. The lobby catalog (`GET /v1/markets?q=&family=&status=&page=&limit=`) lists every indexed board so a user can search and page before opening one. A wallet’s own tickets (`GET /v1/owners/{owner}/positions`, `/portfolio`) list every board that wallet filled, the USDC paid in, and — after settlement — claimed payout / net, plus a prompt when a ticket is still unclaimed. The same board’s **pre-bet ticket** (`GET /v1/markets/{id}/preview`) then shows Pay, fee, hit / miss cashflows, and book EV for the chosen $S$ and $q$, and refreshes when the snapshot slot moves. Football shows the $11\times 11$ table **and** the typed templates (1X2, handicap, totals, BTTS, exact score, custom mask); line prices are sums of $p$ on each template $S$, not a second book. After close the public board discloses $x^*$, $\rho$, $L=E(c)$, and the $C_{\max}$ stack. After close, $\theta$ is frozen; $p$ no longer moves. Web surfaces for these reads are listed in §14.6 and ticketed in SRS §4.7.

#### 8.1.8 $\rho$ and redeem (settlement gate)

**Order is normative.** After $x^*$ is final:

1. Read $L=E(c)$ at $c=\mathrm{cell}(x^*)$ (not $L_{\max}$).
2. Form $C_{\max}=R_{\mathrm{net}}+C_R^{\mathrm{final}}+C_P^{\mathrm{alloc}}$.
3. Write $\rho=\min(1,C_{\max}/L)$ on the board ($\rho=1$ if $L=0$).
4. Only then may tickets claim $\lfloor\rho\cdot\mathrm{face}\rfloor$.

If $C_{\max}\ge L$, funds cover face: $\rho=1$, each unit of face pays $1$ USDC. If $C_{\max}<L$, funds are short: $\rho=C_{\max}/L$ is the **actual** payout ratio, one number for every winner. Misses are $0$ and are not in the haircut. Never FIFO. Dust stays in reserves. VOID / `RESOLUTION_FAILED` skip this gate and refund `cost_paid` — they SHALL NOT invent $\rho=0$ as a fake print.

$$
C_{\max}=R_{\mathrm{net}}+C_R^{\mathrm{final}}+C_P^{\mathrm{alloc}},\qquad
\rho=\min\bigl(1,C_{\max}/L\bigr)
$$

$\hat\rho$ shown while trading is display-only and SHALL NOT be this written $\rho$.

#### 8.1.9 Forbidden

| SHALL NOT | Why |
| --- | --- |
| Re-integrate the UI interval at settle | $S$ was frozen at fill |
| Treat $E$ as the PDF | $E$ is not normalized; $\rho$ would be wrong if used as $p$ |
| Sum $E$ across atoms to get $L$ | A set ticket would be counted $\|S\|$ times |
| Sum `position.q` of “OR-hits” to get $L$ | Quarter tickets would be counted as full $q$ after a half-win |
| Pay $\rho\cdot q$ on a quarter OR-mask | Diverges from $E(c)$ |
| Bake $\rho$ or coverage into $p_S$ / $C_S$ | Price is pure probability |

### 8.2 Maximum Payable $C_{\max}$

Money actually available at settlement:

$$
C_{\max}=R_{\text{net}}+C_R^{\text{final}}+C_P^{\mathrm{alloc}}
$$

| Symbol | Meaning |
| --- | --- |
| $R_{\text{net}}$ | **Primary source.** Net trading revenue: contract cost paid by users. Fees never entered this term. Premia payable to Risk LPs are deducted. |
| $C_R^{\text{final}}$ | Optional Risk LP capital locked and drawable at settlement $\sum_i D_i$. May be $0$ if nobody filled. |
| $C_P^{\mathrm{alloc}}$ | Optional draw from the platform adjustment fund, capped per board and by the pool. May be $0$. |

If there is no risk capital, payout is still due: $C_{\max}=R_{\mathrm{net}}+C_P^{\mathrm{alloc}}$. The main source is trading.

Money users have already paid the market must be used for payout first. Leaving $R_{\text{net}}$ out of the numerator would haircut users while the market sits on trading income.

$C_R^{\text{final}}$ is collateral actually locked at settlement. Unlocked promises do not count. $C_P^{\mathrm{alloc}}$ is defined in section 9.3; it is not the whole treasury.

### 8.3 Recovery Rate

$$
\rho=\min\left(1,\;\frac{C_{\max}}{L}\right)
$$

USDC paid to a ticket is $\rho\cdot\mathrm{face}_j$ (section 8.1.5), not automatically $\rho\cdot q_j$. All winners use the same $\rho$. Dust is $\lfloor\rho\cdot\mathrm{face}_j\rfloor$; remainder stays in reserves. How a holder reads the number on the ticket: **§1.2.3**.

### 8.4 First-Come-First-Served Is Forbidden

When funds fall short, later positions **must not** be treated as junior debt and cut alone by fill time.

First-come-first-served would harm later users because of queue position, conflicting with “do not block orders, cap at final capital”. Later users share the same funding gap as every winner; they do not have a worse seniority.

| User | Entry | Face value | Received at $\rho=5/6$ |
| --- | --- | --- | --- |
| A | Bought first | 600,000 | 500,000 |
| B | Bought later | 300,000 | 250,000 |

### 8.5 Division of Labor Between $L_{\max}$ and $L$

| Metric | When used | Role |
| --- | --- | --- |
| $L_{\max}=\sup_x E(x)$ | Trading period | Display worst-case coverage $C_{\max}/L_{\max}$ |
| $L=E(x^*)$ | Settlement | Compute true $\rho$ |

Even if some cell is bought out, if $x^*$ lands in an un-oversold region, $\rho=1$ is still possible.

During trading the UI may also show: if the user buys $I$, worst-case recovery is about

$$
\hat\rho_I=\min_{x\in I}\min\bigl(1,C_{\max}/E(x)\bigr)
$$

This is a warning only. It is not the fill price and does not lock final $\rho$.

### 8.6 Capital Draw Order

```text
R_net  (trading; always first)
    │
    ├─ Enough to pay L
    │     → ρ = 1, H_i = 0, C_P not drawn
    │     → surplus S = max(R_net − L, 0)
    │
    └─ Short
           │
           ▼
     Draw Risk LP by layer, only the leftover shortfall, cap C_R_final
           │
           ├─ Still short → draw C_P^alloc (section 9.3)
           │
           ├─ C_max ≥ L → ρ = 1, S = 0
           └─ C_max < L → ρ = C_max / L, same ratio, S = 0
```

Risk LPs pay at most $C_R^{\text{final}}$ and are not topped up because of later orders. $C_P$ pays at most the allocated cap. After every locked source is exhausted, the remaining gap is shared by all winners.

**Surplus is not this draw.** Drawing $H$ / $C_P$ covers a shortfall so winners can still be paid at $\rho=1$. Surplus $S=\max(R_{\mathrm{net}}-L,0)$ exists only after that full pay. Split, default $\alpha_R=70\%$, claims, and the fee ledger: **§1.2.1**. Chain: `begin_settle` writes `rho_raw`, `surplus`, `surplus_r`, `surplus_p`; then `pay_surplus_lp` / `pay_surplus_platform`.

### 8.7 Numeric Example

At expiry the cell of $x^*$ has face-value total payout $L=12{,}000{,}000$.

- Net trading revenue $3{,}000{,}000$
- Locked Risk LP $5{,}000{,}000$
- Platform adjustment allocated $0$

Then $C_{\max}=8{,}000{,}000$, $\rho=8/12=2/3$. If the pool later allocated $C_P=1{,}000{,}000$ to this board, $C_{\max}=9{,}000{,}000$ and $\rho=3/4$.

If the same market has $L_{\max}=20{,}000{,}000$ but $x^*$ lands on a cell with only $4{,}000{,}000$ of exposure, then $L=4{,}000{,}000$, $\rho=1$.

### 8.8 Coverage Display

The board continuously displays:

- Current $C_{\max}$
- Current $L_{\max}$
- Worst-case coverage $C_{\max}/L_{\max}$
- Estimated worst $\hat\rho_I$ for the user’s selected interval

When coverage is too low, show a strong warning; **orders are still allowed**. Price is always quoted as $p_I$; $\rho$ is not folded into LMSR.

---

## 9. Commercial Operating Model

This section is the commercial rule, not an implementation hint. Settlement, vault ledgers, and the UI must follow it.

### 9.1 Capital stack (what may pay users)

Three USDC pots can fund a board. Only the first is required.

| Seniority | Pot | Who provides it | Required? | When it is used |
| --- | --- | --- | --- | --- |
| 1 | $R_{\mathrm{net}}$ | Traders, via $C_S(q)$ | Yes (the default source) | Always first |
| 2 | $C_R^{\mathrm{final}}$ | Risk LPs who locked and filled | No. May be $0$ | After trading, by layer, only the leftover shortfall |
| 3 | $C_P^{\mathrm{alloc}}$ | Platform adjustment fund | No. May be $0$ | Last backstop after $C_R$, still capped |

$$
C_{\max}=R_{\mathrm{net}}+C_R^{\mathrm{final}}+C_P^{\mathrm{alloc}}
$$

**If no risk capital entered, payout is still owed and comes from trading** (plus any $C_P$ allocated to that board). Soft solvency is unchanged: if that is still less than $L$, one global $\rho$.

$C_P$ is not a promise that $\rho=1$. It is a bounded commercial reserve so that a thin board is not immediately a lottery.

### 9.2 Two payout paths

**Path A — risk capital is not needed.** $L\le R_{\mathrm{net}}$.

- Users are paid in full ($\rho=1$).
- $H_i=0$. Locked LP collateral is released, not debited.
- $C_P$ is not drawn.
- Residual $S=\max(R_{\mathrm{net}}-L,0)$ exists.
- If $C_R^{\mathrm{final}}>0$ (LPs stood ready but were not hit): $S_R=\alpha_R S$ to those LPs, $S_P=\alpha_P S$ to the platform.
- If no risk capital entered: $S_R=0$, the whole $S$ goes to the platform.

**Path B — risk capital (and maybe $C_P$) is needed.** $L>R_{\mathrm{net}}$.

- Draw layers for $\min\bigl(H_i^{\mathrm{raw}},\ (L-R_{\mathrm{net}})^+-\text{already drawn}\bigr)$.
- If still short, allocate $C_P$ as in 9.3.
- $C_{\max}\ge L$ → $\rho=1$, $S=0$ (own funds were exhausted).
- $C_{\max}<L$ → $\rho=C_{\max}/L$, $S=0$.

### 9.3 Fees and the platform adjustment fund pool

The adjustment fund is not a per-board piggy bank. It is **one protocol pool**.

| Name | Symbol | What it is |
| --- | --- | --- |
| Platform adjustment fund pool | $C_P^{\mathrm{pool}}$ | A single L1 USDC vault. All recyclable platform capital sits here. |
| Board tap cap | $C_P^{\mathrm{board}}$ | Max this board may take from the pool, locked at listing. May be $0$. |
| Settlement draw | $C_P^{\mathrm{alloc}}$ | What this board actually takes at settlement. Debited from the pool. |

$$
C_P^{\mathrm{alloc}}=\min\bigl((L-R_{\mathrm{net}}-C_R^{\mathrm{final}})^+,\; C_P^{\mathrm{board}},\; C_P^{\mathrm{pool}}\bigr)
$$

After the draw, $C_P^{\mathrm{pool}}\leftarrow C_P^{\mathrm{pool}}-C_P^{\mathrm{alloc}}$. Two short boards settling in the same window share the remaining pool; there is no reserved slice per board except the listing cap.

Each fill the user pays:

$$
\mathrm{Pay}=C_S(q)+\phi\cdot C_S(q)
$$

- $C_S(q)$: LMSR contract cost → this board’s Vault → `TradingRevenue` → $R_{\mathrm{net}}$ after premia.
- $\phi\cdot C_S(q)$: **platform income at fill time**. It does not enter this board’s $C_{\max}$ and is not deducted from $R_{\mathrm{net}}$ a second time.

$$
R_{\text{net}}=\text{TradingRevenue}-\text{RiskPremium}-\text{HedgeCost}-\text{OracleCost}
$$

Inflows to the **pool** (explicit `fund_pool` only — never a fill, never a fee):

```text
φ  →  platform fee ledger  ──claim_fees──►  platform UserVault  ──withdraw──►  platform wallet
S_P →  platform UserVault (pay_surplus_platform; only when ρ=1)
platform top-up USDC   ──fund_pool──►  C_P^pool
                                      │
                                      ▼  only at settlement of a short board
                                 C_P^alloc onto that board
```

- A fill’s fee must not jump into **that same board’s** $C_{\max}$ and SHALL NOT enter $C_P$.
- Users and LPs have no claim on unallocated pool balances.
- No `admin_withdraw` of the pool except: settlement draws, or a listed unwind of unused pool into the platform treasury after a published delay.
- The pool does not change LMSR prices. Coverage may show $C_P^{\mathrm{board}}$ as a warning, never as the quote.

### 9.4 Residual profit

Only when users have already been paid in full at face value ($\rho=1$):

$$
S=\max(R_{\mathrm{net}}-L,0)
$$

When $\rho<1$, $S=0$ and there is no profit share.

If $C_R^{\mathrm{final}}>0$:

$$
S=S_R+S_P,\qquad S_R=\alpha_R S,\quad S_P=S-S_R,\qquad \alpha_R+\alpha_P=1
$$

- $S_R$: this board’s filled Risk LPs, then by each LP’s $\alpha_i$ agreed at fill
- $S_P$: the platform

If $C_R^{\mathrm{final}}=0$, $S_R=0$ and $S=S_P$.

Profit share is a residual claim, not a guaranteed return. $\phi$ is a different pot from $S_P$. $\phi$ is claimed via `claim_fees`; it SHALL NOT be swept into $C_P$.

### 9.5 Full waterfall

```text
Trading revenue R_net          ← primary commercial source
    │
    ▼
    │
    ▼
User payout (ρ · face value, one global ρ)
    │
    ▼
Risk layer draw (H_i on leftover shortfall only)
    │
    ▼
Platform adjustment draw C_P^alloc
    │
    ▼
Residual profit (only when ρ = 1)
    ├── Risk LP (α_R)  if C_R^final > 0
    └── Platform (α_P or all of S)
```

Seniority must not be inverted: user payout precedes Risk LP profit share; fee claim is independent of settlement and never funds $C_P$.

A Risk LP’s Premium is the consideration for underwriting. It is already deducted when computing $R_{\mathrm{net}}$, so the same money is not both premium and user payout.

---

## 10. Result Finalization

$x^*$ does not appear on-chain by itself. A Solana program can only write an account inside some transaction. Without `submit_result`, a football score, CPI print, election, or December 1 BTC price cannot enter settlement.

Therefore **the only on-chain path is the committee (or an authorized reporter) submitting the settlement value**. External pages and APIs are off-chain references for the reporter; they never write $x^*$.

### 10.1 Why the Committee Must Submit

A Solana program cannot grow a football score, a CPI print, an election result, or “BTC at 00:00 on December 1” by itself. Those values exist off-chain. Someone must call `submit_result`.

| Intended action | Actual procedure |
| --- | --- |
| BTC at 00:00 December 1 | Inside the report window a member or authorized reporter calls `submit_result(price)` by the listing `price_rule` |
| Custom TWAP / multi-venue average | Written into `price_rule` at listing; the committee computes it and reports one number. The contract stores the scalar; it does not re-run the algorithm |
| Football / CPI / election / event | A score, print, winner, or YES/NO must be reported the same way |

There is no “data source automatically writes our PDA”. An authorized bot may read an API and then call the same `submit_result`; it is still a reporter, not a second settlement channel.

### 10.2 On-Chain Instructions (shared by all markets)

After the market closes and state is back on L1, the report window opens. Nobody may rewrite the PDF by bypassing this instruction.

```text
submit_result(market, value, evidence?)
challenge(market, value, bond)
finalize(market)          // challenge window ends with no objection
vote(market, value)       // after entering a vote
```

`value` by board type:

| Market | Must report |
| --- | --- |
| Football | $(x^*,y^*)$, consistent with `score_scope` |
| CPI / macro | Official first-print scalar |
| Election winner | `winner_id` |
| Election top $n$ | $n$ ids |
| Vote share | Shares or raw votes |
| BTC and other daily prices | One price scalar |
| Binary event | YES or NO |

Reporting “home won” or “above 100k holds” is invalid. Line win/loss is derived from cells.

### 10.3 Committee Flow

Each board binds a Resolver: the creator’s roster, or the protocol’s public committee. Optimistic report + challenge:

1. A member or authorized reporter submits `value` and posts a bond. The payload is the settlement value under the listing convention, not a feed account.
2. If nobody objects inside the challenge window, `finalize` locks it.
3. If someone objects and posts a bond, enter an $M/N$ vote; price/macro may use the median inside a tolerance $\varepsilon$.
4. If the vote fails, extend the report window; if it still fails, `RESOLUTION_FAILED`.

An authorized bot (sports feed, price keeper) is only “a member who may propose first”. Disputes still return to the same committee; no separate settlement channel is opened.

### 10.4 How a Price Board Is Reported

Listing locks `price_rule` and `observe_ts`. The committee (or an authorized reporter) computes the scalar from that text and calls `submit_result(price)`. `price_rule` must be verifiable, e.g. “Coinbase BTC-USD last at `observe_ts`” or “arithmetic mean of Binance + Coinbase 1-minute averages”.

The contract stores the number. It does not fetch a feed, re-run a formula, or treat any oracle as the settler. The challenge window still applies.

### 10.5 Report Failure ≠ Insufficient Funds

| State | Meaning | Handling |
| --- | --- | --- |
| `RESOLUTION_FAILED` | No lawful on-chain $x^*$ | Extend the window or refund. There is not yet a $\rho$ |
| $\rho<1$ | The outcome is already finalized; money is short | Haircut winners per section 8 |

Do not pro-rata cut positions before the result is finalized.

If the report window + challenge window ends still without a valid $x^*$: auto-extend once; if it fails again, refund user funds, return LP collateral, and return unused premium.

### 10.6 Football Score Finalization

Football must be submitted as a score by a reporter. An authorized sports bot only calls `submit_result` on someone’s behalf. Submit:

- `home_score` / `away_score`: non-negative integers
- Consistent with the creation-time `score_scope` (default regulation only)

The reported score must match that board’s `score_scope` (regulation / including extra time / including penalties). Map onto the grid: $\hat{x}=\min(x^*,k_{\max})$, $\hat{y}=\min(y^*,k_{\max})$, take $L=E(\hat{x},\hat{y})$.

Reporting “home” is invalid; a score must be reported. 1X2 is derived from the cell, so the resolution source and line convention cannot fight. Inside the challenge window, compare to the official score; if inconsistent, vote or enable a backup source. Abandoned matches take the 4.6.7 `VOID` path; they are not a “0-0 full time”.

### 10.7 Macro, Election, and Binary-Event Finalization

| Market | Report content | Mapping |
| --- | --- | --- |
| CPI / macro (4.8) | Official first-print scalar | Nearest grid, yielding $L=E_{k^*}$ |
| Election winner (4.9) | `winner_id` | $L=E_{\mathrm{winner}}$ |
| Election top $n$ (4.9) | $n$ ids | $L=$ that combination atom’s exposure |
| Election vote share (4.9) | Share vector | The share cell it falls into |
| BTC daily price (4.10) | Committee-reported price by `price_rule` | Nearest grid after clamp |
| Binary event (4.11) | YES or NO | $L=E_{\mathrm{YES}}$ or $E_{\mathrm{NO}}$ |

Elections must not report a poll. Binary events must not finalize YES early because it “looks about to happen”. Macro must match the creation-time `series_id` + `print_rule`.

---

## 11. Market Lifecycle

```text
CREATED
   │  Generic board: initialize PDF, resolution_source, grid
   │  Football / CPI / election / daily price / binary event: see 4.6–4.11
   ▼
PDF_ACTIVE / DELEGATED
   │  State delegated to MagicBlock ER
   ▼
TRADING
   │  Interval buy/sell, update θ and E(x), display coverage
   │  In parallel: RISK_AUCTION → RISK_COMMITTED (top up capital; does not block trading)
   ▼
MARKET_CLOSED
   │  Stop fills; ER state Commit / Undelegate back to L1
   ▼
ORACLE_FINAL
   │  submit_result on-chain → challenge / vote → x*
   ▼
SETTLEMENT
   │  Compute L, C_max, ρ; pay users; draw Risk LP
   ▼
PROFIT_DISTRIBUTION
   │  Only when ρ = 1 and reserves meet the bar
   ▼
CLOSED
```

Exceptions:

```text
RESOLUTION_FAILED → delay / backup source / refund
RISK_LP_DEFAULT  → that LP’s collateral is forfeited and counted in C_max; any remainder is still absorbed by ρ
```

If a Risk LP defaults, its already-locked collateral must still be used for payout. “Promised but not received” must not be counted in $C_R^{\text{final}}$.

---

## 12. Protocol Invariants

The protocol must always maintain:

**Invariant 1 — PDF normalization**

$$
\int_{\Omega}f(x)\,dx=1
$$

**Invariant 2 — Payout cap**

$$
\sum_j \text{ActualPayout}_j \le C_{\max}=R_{\text{net}}+C_R^{\text{final}}+C_P^{\mathrm{alloc}}
$$

Trading-period hard reject on $L_{\max}$ vs locked capital is no longer required.

**Invariant 3 — Risk LP collateral**

$$
\text{Collateral}_i\ge D_i
$$

Size that is not fully locked must not be counted in $C_R^{\text{final}}$.

**Invariant 4 — Profit share does not exceed residual**

$$
\text{ProfitShare}\le S
$$

**Invariant 5 — Users first**

User payout precedes Profit Share. When $\rho<1$, $S=0$.

**Invariant 6 — One recovery rate**

Every position with $x^*\in I_j$ uses the same $\rho$. Distinguishing seniority by entry time is forbidden.

**Invariant 7 — Risk LP liability cap**

$$
H_i(L)\le D_i
$$

Later trades must not increase an already-locked LP’s $D_i$.

---

## 13. Protocol Economic Loop

```text
Trader ──buy interval──► Prediction Market ──revenue──► Vault
                         │
                         │ display coverage / suggested coverage size
                         ▼
                   Risk Capital Market
                         │
              Risk LP locks collateral, receives Premium
                         │
                         ▼
              Expiry x* → L = E(x*) → R_net → C_R → C_P → ρ → payout
```

- No tail, or the layer is not breached: Risk LP receives Premium and may receive Profit Share; $C_P$ is not drawn
- Layer breached but $C_{\max}\ge L$: Risk LP pays by leftover $H_i$; $C_P$ may finish the gap; users still receive full face value
- $C_{\max}<L$: every locked source is drawn up to its cap; users are haircut at $\rho$
- No $C_R$: the board still settles from trading, then $C_P$ if allocated

A Risk LP’s core metric is not nominal APY, but risk-adjusted return:

$$
\mathrm{RAR}=\frac{\text{Premium}+\mathbb{E}[\text{ProfitShare}]-\mathbb{E}[\text{Loss}]}{\text{CapitalLocked}}
$$

---

## 14. Technical Architecture

Landing stack: **Solana L1 + MagicBlock Ephemeral Rollup**.

```text
┌─────────────────────────────────────────────────────────────┐
│                    Solana L1                                │
│  Global Vault (USDC) / Risk LP Vault / Oracle / Settlement  │
└───────────────┬──────────────────────────────▲──────────────┘
                │ Delegate                     │ Commit / Undelegate
                ▼                              │
┌──────────────────────────────────────────────┴──────────────┐
│              MagicBlock Ephemeral Rollup                    │
│  Grid θ[] / E[] / Fast Math / buy_interval / coverage update │
└──────────────────────────▲──────────────────────────────────┘
                           │ <10ms trades
                    Traders / Solvers
```

### 14.1 Layered Responsibilities

| Layer | Responsibility |
| --- | --- |
| Solana L1 | Fund custody, Risk LP collateral lock, market creation, Delegate / Commit, `submit_result` finalization, settlement transfers |
| MagicBlock ER | Grid state, LMSR pricing, fills, $E(x)$ / $L_{\max}$ / coverage updates, WebSocket PDF push |
| Client | Next.js only: website / PWA / in-wallet browser / official-site TWA. Connect wallet first, deposit USDC, open a Session |

### 14.2 Key State

A market account contains at least:

- `market_id` / `creator` / `liquidity_beta`
- `total_grid_count` / `x_min` / `x_max`
- `committed_risk_capacity` / `current_max_liability`
- `resolution_source` and its config
- `is_delegated` / lifecycle state

Grid buffers:

- `theta_states[N]`
- `exposure_states[N]`

### 14.3 Trade-Instruction Notes

`buy_interval(start_grid, end_grid, q)` on the ER:

1. Check the market is open, the interval is legal, and the user balance can pay $C_I(q)$
2. Compute $p_I$ and $C_I(q)$
3. Update $\theta_k$, $E_k$, $L_{\max}$
4. Update and push coverage
5. **Do not** reject because $L_{\max}$ exceeds locked risk capital

Solvency checking moves from “whether a fill is allowed” to computing $\rho$ at settlement.

### 14.4 Performance Targets

| Metric | Target |
| --- | --- |
| Latency inside the ER | < 10ms |
| Gas inside the ER | 0 |
| Grid size | $N=256\sim 1024$ |
| Frontend experience | Session Keys; interval point-and-click close to a CEX |

Settlement, committee votes, and fund transfers run on L1 and do not require sub-millisecond latency.

### 14.5 How Users Connect a Wallet (product convention)

There is no “login with account and password”. Identity is the wallet public key. Desktop browser, PWA, and in-wallet browser share the same flow; only how the wallet is invoked differs. There is no standalone native app.

```text
Connect wallet → sign in (SIWS, for the query session)
    → deposit USDC into the protocol Vault (main-wallet popup, on L1)
    → authorize a trading Session (main wallet pops again: time bound + size bound + buy/sell only)
    → in-board point-and-click orders no longer pop
    → withdraw / report as a committee member / create a market: must invoke the main wallet again
```

Rules:

- No deposit, no fill. In-board debit is confirmed available balance in the Vault, not USDC casually scanned in the wallet
- If a Session is lost or stolen, loss does not exceed the authorized size and term; withdrawal rights stay with the main wallet
- Users may revoke a Session at any time; disconnecting the wallet is not on-chain revocation — the product must also provide “revoke session”
- Fills are public. Connecting a wallet does not provide on-chain anonymity

Implementation stack (locked; details in `technical-architecture.md`): the user client is **Next.js only** (PWA + wallet WebView + Android official-site TWA). **Do not list** on the App Store / Play, and do not build Flutter / RN. Contracts = **Anchor**. Frictionless orders = MagicBlock Session Keys + this protocol’s limits. Funds do not go through store IAP.

### 14.6 Client surfaces (web acceptance)

Every human role in §3 has a screen. Numbered SHALL / verify live in SRS §4.7 (FR-UI-*). A route name alone is not a product.

| Role | Page | Job |
| --- | --- | --- |
| Trader | `/` | Search and page every indexed board; row heading is the market name (SRS FR-UI-38) |
| Trader | `/m/[id]` | **Trade ticket** (SRS FR-UI-32): live implied PDF (FR-UI-39), pre-bet Pay / hit / miss, typed Skellam lines, coverage warning, pending nonce. After `close_ts`: buy / sell **disabled** — no fills. After settlement: public $x^*$ / $\rho$ / $L$ / $C_{\max}$ (FR-UI-33). **Comments** only after the prediction market is indexed (FR-UI-42): off-chain catalog, not settlement or capital |
| Trader | `/portfolio` | **Cash ticket** (SRS FR-UI-35): Circle USDC deposit / withdraw on confirmed `available` / free; all fills (market **name**, not pubkey-only); **settlement tickets** for claim $\lfloor\rho\cdot\mathrm{face}\rfloor$ or refund (SRS FR-UI-33). Claim uses the fill journal (`TICKETS_PATH` / `POST /v1/tickets`) so $S$ is not this browser’s localStorage (FR-UI-41) |
| Trader | chrome | Connect / SIWS; open / renew / revoke Session (revoke ≠ disconnect); in-app inbox |
| Risk LP | `/auctions`, `/auction/[id]` | Browse open books by title; **auction ticket** on published layers (SRS FR-UI-37) |
| Risk LP | `/lp` | Locked $D_i$ by market **name**; $H$, premium, surplus / unlock claim; list reloads after each write (FR-UI-41) |
| Trader / applicant | `/create` | **Listing ticket** (SRS FR-UI-31): any SIWS user. Submits an **application** (title, tags, prior, compose spec, geo blocks). Not on-chain. Not open until the reviewer approves **and** signs create (FR-UI-43). Applicant SHALL NOT sign `create_*`. Duplicate → `409` (FR-UI-45). **No comments** on create |
| Reviewer | `/review` | System review queue (FR-UI-43): **批准并开通预测市场** — reviewer wallet signs `create_*` and 开放风险拍卖; lobby shows it (`OPEN`). **继续开通预测市场** retries if create already landed. 拒绝 / 标为重复. Confirm region blocks (FR-UI-44). Audit log. Not committee, not Vault withdraw |
| Committee | `/committee`, `/resolve/[id]` | Open window, `submit_result`, evidence object (hash on-chain), challenge, $M/N$. Both desks show the listing name and a market card (FR-UI-36 / FR-UI-41) |
| Ops / platform | `/ops` | Read-only: index lag, coverage, Vault identity, $C_P^{\mathrm{pool}}$, keeper heartbeat, per-board $C_P$ by market **name**. **No** Vault withdraw |
| Keeper | CLI | `close` / Commit / Undelegate / alerts. Not a web write path |

Football on `/m/[id]` is one Skellam prediction market with templates, not a generic cell picker pretending to be 1X2. Create collects a human listing title, catalog tags (English vocabulary, several allowed — e.g. `football` + `epl` or `football` + `world cup`; not the distribution family), $\beta$, grid, $C_P^{\mathrm{board}}$, committee, clocks, resolution rule, published layers, optional region blocks, and the family prior. Submit is an **application** (SIWS); a system reviewer opens the prediction market (FR-UI-43). Title and tags are listing metadata (`POST /v1/listings` after approve); topic / tag stay the on-chain series key. **Comments are not part of create** — they appear on `/m/[id]` only after the prediction market is approved and indexed (FR-UI-42): wallet pubkey, Postgres `market_comment`, not settlement or capital. CPI / macro priors are $\Omega$ and $\mathcal{N}(\mu,\sigma^2)$ in percentage points (survey median $\to\mu$, survey dispersion $\to\sigma$), not integer cell indices. The risk auction is a layer-stack ticket ($A$, $T$, $H$ if drawn, standing ladder), not four headline numbers. Every write path is a ticket (SRS §4.7.1): listing, identity, prior, cash, trade, settlement, auction layer, committee phase.

### 14.7 Fills and Bookkeeping Recognize Only USDC

Every amount inside the protocol — contract cost $C_S(q)$, fees, Vault balance, Risk LP collateral, premium, $L$, $C_{\max}$, $\rho$ payout, surplus — is **priced and transferred only in Circle SPL USDC**.

| Question | Answer |
| --- | --- |
| Use a stablecoin? | **USDC only**. Not “any stablecoin” |
| Can users order in SOL? | **No**. SOL is only for L1 network fees (deposit, open Session, withdraw, report) |
| Can users send any coin and have the protocol swap into USDC? | **No**. The Vault does not connect Jupiter, does not connect an FX oracle, and does not accept a second mint |
| Wallet holds only SOL / other coins? | The user **themselves** swaps into USDC in the wallet or an external DEX, then `deposit`. FX risk, slippage, and failures stay outside the protocol |

Reason: $C_{\max}$ and $L$ must be the same unit, or $\rho$ would bake FX noise into payout. Auto-swap would stuff oracle and AMM slippage into the reserve and fake the solvency numbers.

`vault.deposit` checks mint == official USDC. Token Accounts of other mints are rejected outright. Risk LP collateral uses the same mint. Payouts send only USDC.

---

## 15. Product Structure Overview

```text
┌─────────────────────────────────────────┐
│     Continuous PDF Prediction           │
│     f(x) / probability quotes / positions / coverage │
└───────────────────┬─────────────────────┘
                    ▼
┌─────────────────────────────────────────┐
│     Risk Engine                         │
│     E(x) / L_max / suggested coverage size │
└───────────────────┬─────────────────────┘
                    ▼
┌─────────────────────────────────────────┐
│     Risk Capital Auction                │
│     layers / premium / collateral (does not block trading) │
└───────────────────┬─────────────────────┘
                    ▼
┌─────────────────────────────────────────┐
│     Resolution                          │
│     submit_result → challenge / vote → x* │
└───────────────────┬─────────────────────┘
                    ▼
┌─────────────────────────────────────────┐
│     Settlement                          │
│     L=E(x*) / ρ=min(1,C_max/L) / profit share │
└─────────────────────────────────────────┘
```

Compressed mathematical objects:

| Object | Formula |
| --- | --- |
| Probability | $f(x)$ |
| User face value | $g_I(x)=1_{x\in I}$ |
| Exposure | $E(x)=\sum_j q_j g_{I_j}(x)$ |
| Settlement liability | $L=E(x^*)$ |
| Payable | $C_{\max}=R_{\text{net}}+C_R^{\text{final}}+C_P^{\mathrm{alloc}}$ |
| Recovery rate | $\rho=\min(1,C_{\max}/L)$ |
| User received | $\rho\cdot q_j\cdot 1_{I_j}(x^*)$ |
| Risk layer | $H_i(L)=\min((L-A_i)^+,D_i)$ |
| LP profit | $\Pi_i=\text{Premium}_i-H_i(L)+\alpha_i S$ |

Final chain:

$$
\text{Probability}\rightarrow\text{Liability}\rightarrow\text{Tail Risk}\rightarrow\text{Risk Capital}\rightarrow\text{Premium}\rightarrow\rho\cdot\text{Payout}
$$

---

## 16. Locked Rules

Implementation must follow the conventions below. There is no remaining fork of “fill this in later”.

| Topic | Rule |
| --- | --- |
| Reject for insufficient funds? | Do not reject |
| Settlement liability | $L=E(x^*)$, not $L_{\max}$ |
| Claim unit | Fill size $q$ is **shares** on frozen $S$. Face is $1$ USDC per ordinary share if it hits. Payout is $\rho\cdot q$ if $i^*(x^*)\in S$, else $0$. Not $q/p_S$. How the holder reads it: §1.2.3 |
| Price vs payoff | Price $=p_S=\int_I f$ (grid: $\sum_{k\in S}p_k$). $\partial C/\partial q$ is still price. Hit pays $1$ per share. $1/p_S$ is the cash multiple, not the share definition (§1.2.2) |
| Gaussian $n$ | Default $256$. $512$/$1024$ are finer, not required for CPI $\sigma$ vs $\Omega$. $n<32$ warn/reject. Floor $8$ is not a product default (§5.1.1) |
| 1-D interval $[a,b]$ | Snap both ends with `interval_index`; $S$ is the inclusive node range. No partial node, no $\int_a^b$ at fill or settle (§8.1.2.1) |
| Haircut method | If $C_{\max}<L$, one global $\rho=C_{\max}/L$ on every hitting share; FIFO forbidden |
| $C_{\max}$ | $R_{\text{net}}+C_R^{\mathrm{final}}+C_P^{\mathrm{alloc}}$; $C_R$, $C_P$ may each be $0$ |
| Trading fee | Listing locks `fee_bps` + `fee_timing`. At fill: $\phi\cdot C_S$ now. At claim: $\phi$ of the winner’s payout. Never $C_P$. `claim_fees` anytime |
| Price of the same outcome | LMSR marginal price rises with fills; buying more makes it more expensive |
| Surplus allocation | **§1.2.1.** Only when $\rho=1$: $S=\max(R_{\mathrm{net}}-L,0)$. No $C_R$ → all $S$ to the platform. Else default $\alpha_R=70\%$ to LPs (by `profit_share_bps` $\times$ filled), $30\%$ to the platform. $\rho<1\Rightarrow S=0$. Fees never enter $S$ |
| Adjustment fund | One protocol pool $C_P^{\mathrm{pool}}$; boards only receive $C_P^{\mathrm{alloc}}$ at settlement. Not an unlimited guarantee; not baked into LMSR |
| Fill price | Pure probability $p_I$; haircut is displayed, not quoted |
| Low coverage | Strong warning; orders still allowed |
| Resolution source | Always committee `submit_result` on-chain; no oracle writes $x^*$ |
| Committee | Each board names a roster or cites the public committee; optimistic report + challenge + $M/N$ |
| PDF representation | Grid or discrete atoms; no parameterized AMM |
| Football | One `score_scope` per match, one Skellam board; 1X2 / AH / totals / score are set projections on shared $\theta$; `buy_skellam_set`; report a score pair |
| CPI | Listing recipe on `create_gaussian_market`; first official print |
| Election | Winner, `TOP_N`, and vote-share boards: `create_dirichlet_market` + `layout`; all three must be completed |
| Daily price | Listing recipe on `create_lognormal_market`; committee reports by `price_rule` |
| Binary event | `create_bernoulli_market`; YES/NO; YES may finalize early before the deadline |
| Distribution family | On-chain create is by family (`skellam` / `gaussian` / `lognormal` / `dirichlet` / `bernoulli`); listing names are metadata; trading does not switch families |
| After `close_ts` | No prediction fills on any family. UI disables buy / sell; compose refuses; chain `Closed`. Kickoff / report / early YES do not reopen trading |
| Highest risk payout | During trading: thickest overlap $\{x:E(x)=L_{\max}\}$. Gaussian / lognormal: that plateau as a print interval on $\Omega$. Not ticket-volume rank. PDF peak ≠ risk peak |
| Risk auction | Opens with the prediction market; published layers, lowest unit premium first; `risk_lock_ts \le close_ts`; `close_ts` also stops the auction |
| Finalization failure | Refund user funds, return LP collateral, return unused premium |
| Chain | Solana + MagicBlock ER |
| Bookkeeping and fill currency | **Only** Circle SPL USDC; SOL only pays L1 fees; no other coins, no in-protocol auto-swap |
| Fill durability | A receipted fill does not vanish because a single node crashes; $\theta$ is replayed from the fill journal; L1 Commit is a checkpoint, not the fill criterion |
| User client | Next.js only; PWA / in-wallet browser / official-site TWA; **not on the App Store / Play**. Pages: SRS §4.7 / product §14.6 |
| Stores | Do not submit a trading app; do not build a Flutter / RN “degraded package” |
| Wallet path | Connect → SIWS → `vault.deposit` → open Session → in-board Session sign; withdraw / create / risk bid / report must use the main wallet; revoke Session ≠ disconnect |
| Keeper vs web | Keeper writes are CLI / KMS only. `/ops` is read-only. Committee desk may `resolve_open` |
| Contracts | Anchor + MagicBlock ER SDK + session-keys; four programs; `vault` / `resolution` do not Delegate and do not accept a Session |
| Backend | Rust Axum throughout; chain client `@coral-xyz/anchor` + `@solana/web3.js` |

---

## 17. Out of Scope

The following capabilities are explicitly not built, and are not written as “maybe later”.

- Rewriting LMSR prices with expected haircut
- Layered payout by user entry order
- Trading-period hard reject on $L_{\max}$ vs locked capital
- One unit of risk capital underwriting multiple boards at once
- Parameterized AMM (updating only $\mu,\sigma$ or $\lambda_H,\lambda_A$)
- A full on-chain $\mathrm{CVaR}$ portfolio optimizer (suggested size from $L_{\max}$ is enough)
- Committee token governance and a large on-chain dispute court
- Using SOL or other SPL tokens as margin / collateral / payout
- In-protocol auto-swap of arbitrary coins into USDC (no Jupiter / aggregator / oracle FX into the Vault)
- Multi-collateral, synthetic dollars, or a home-grown stablecoin
- Flutter / React Native / a standalone native store app
- Submitting a trading client or a “read-only degraded package” to the App Store or Google Play
- Funding or selling chips / prediction tokens via Apple IAP or Play Billing
- Building circumvention for listing (in-store prompts to buy chips off-site)

---

## 18. Related documents

1. `software-requirements-specification.md` — numbered SHALL / SHALL NOT for implementation and QA
2. `system-architecture.md` / `system-arch.png` — Web / PWA / CLI / services / network / machines
3. `technical-architecture.md` / `tech-arch.png` — frameworks, middleware, LMSR and settlement algorithms
