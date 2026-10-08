# Risk capital handbook

**Who this is for:** wallets that lock USDC to underwrite a prediction market (Risk LPs).  
**What this is:** the auction and payout rules in plain language.  
**What this is not:** LMSR math, committee procedure, or engineer tickets. Those stay in the product specification.  
**If this file and the product specification disagree, the product specification wins** (`docs/product-specification.md` §1.2.1, §1.2.6, §6, §8.6).

Language of this handbook: English (same as the product specification). Chinese walkthrough: `docs/risk-capital-guide.zh.md`.

---

## 0. Cash map (do not mix these)

One vault USDC account. Five ledgers:

| Ledger | For | Not for |
| --- | --- | --- |
| Trading revenue \(T\) | Winners first | Fees |
| Risk capital \(C_R\) | Winners second (draw \(H\) if short) | Cover-pool insurance |
| Adjustment pool \(C_P\) | Winners third, capped | LP losses, fees |
| LP cover pool \(S_C\) | Lifetime \(\Pi<0\) Risk LP wallets | Winners |
| Fees \(\phi\) / surplus \(S_P\) | Platform (two claims: `claim_fees` vs `pay_surplus_platform`) | \(C_P\) or cover |

Lock ρ once per prediction market, then Portfolio (winners) and `/lp` (quotes) claim **per ticket**. Formal map: product §1.2.6.

---

## 1. What you are selling

You are **not** buying a ticket. You are posting **coverage**: “this prediction market may draw up to \(D\) USDC from me if winners need more cash than trading collected.”

| You lock | Worst case | Best case |
| --- | --- | --- |
| Collateral \(\ge D\) in your UserVault (reserved, not sent out) | Settlement draws up to your filled \(D\) (you lose that \(H\)) | No draw: unused \(D\) unlocks; you may collect premium / surplus in rank order |

Traders can still buy when coverage is thin. The auction **does not** gate trading. Thin coverage only warns them that expiry might pay at a haircut \(\rho<1\).

One lock **underwrites one prediction market only**. You may quote several markets with **separate** collateral.

---

## 2. Words you will see

| Word | Meaning |
| --- | --- |
| \(D\) (capacity) | Max this quote can be drawn |
| Premium | What you ask to be paid if there is leftover cash and you rank high enough |
| Unit premium | \(\mathrm{premium}/D\). **Lower is cheaper** and ranks first |
| Profit share | Extra field on the quote. **No-draw surplus** is paid by **rank and premium cap**, not by this field. If a draw happened, this market has \(S=0\), so there is no surplus share to weight |
| \(T\) | Trading revenue (what buyers paid into this market, not fees) |
| \(L\) | Face stacked on the **realized** outcome only — not the worst cell during trading |
| \(C_R\) | Sum of filled, locked \(D\) |
| \(H\) | Draw from risk capital: leftover shortfall \((L-T)^+\), each quote at most its filled \(D\) |
| \(\rho\) | One recovery rate for **all** winning tickets. \(1\) = paid in full |
| \(S\) | Surplus, only if \(\rho=1\): \(S=\max(T-L,0)\) |
| \(\Pi\) | Your **lifetime** risk P&L on this wallet (all markets) |

Fees never enter \(T\), \(S\), or the cover pool.

---

## 3. Clock

```text
Reviewer opens the prediction market
        │  auction is already open (even with zero trades)
        ▼
Trading + quoting in parallel
        │  you post D / premium / profit share and lock collateral
        ▼
risk_lock_ts  (≤ close_ts; often equal)
        │  no new quotes
        ▼
close_ts
        │  no new trades, no new quotes; locked C_R stays
        ▼
report_open_ts  (≥ close_ts; required; often equal)
        │  committee MAY submit_result for report_window_secs (default 24h)
        │  missed report: wait for Market.platform
        │    admin_submit_result → slash committee_bond, then settle
        │    admin_void_resolution → VOID, return bond, no slash
        ▼
Committee (or platform) writes the result → finalize
        ▼
begin_settle locks L, T, C_R, ρ, S
        ▼
You claim on /lp: draw H (if any) → premium → surplus → unlock unused D
        │  surplus markets also Fund cover (20% of S)
        ▼
If your lifetime Π < 0, Claim cover from the protocol pool
```

There is **no** extra window after `close_ts` to top up coverage.

Screens: browse `/auctions`, quote `/auction/[id]`, claim `/lp`. Use the **main wallet**. Session keys cannot bid or claim.

---

## 4. How you quote

On `/auction/[id]` you post:

1. **Capacity \(D\)** — at least this listing’s min size (`d_unit`, default 10 USDC). Dust is rejected.
2. **Premium** — absolute USDC, not a percent. Rank uses \(\mathrm{premium}/D\).
3. **Profit share** — stored on the quote. No-draw leftover uses rank + premium cap instead. After a draw, \(S=0\), so this field does not pay you.
4. **Lock** — UserVault must already hold enough **free** USDC; the quote **reserves** \(D\), it does not withdraw to a third party.

Standing book: **at most 64 quotes** per prediction market. Every valid quote that fits **joins the pool in full**. There is:

- **no** layer tower \([A, A+D]\)
- **no** 10% concentration cap
- **no** “only the first quote counts”

More locked \(D\) is better for winners. Rank does **not** stop a later quote from locking.

---

## 5. Who ranks first

Rank is **only** for **who gets paid surplus / premium when nobody is drawn**. It does **not** decide who is drawn first when cash is short (the shortfall takes from filled quotes until the gap is closed, each capped at filled \(D\)).

1. Lowest unit premium \(\mathrm{premium}/D\)
2. If tied, **earlier** timestamp

Cheaper cover ranks ahead of expensive cover. Tiny \(D\) that fails min size never enters the book.

---

## 6. Three settlement stories

Settlement **first** pays winning traders at one \(\rho\). Risk capital is a **backstop**, then a **residual claim**.

### Story A — Trading already covers winners (\(L \le T\))

- Nobody is drawn. \(H=0\). Your \(D\) unlocks after you claim.
- \(\rho=1\). Surplus \(S=T-L\) exists.
- Split \(S\) as in §7. The slice for this market’s quotes (\(S_R\)) is paid **down the rank until it is gone**:
  - next quote receives up to **its own premium**
  - then the next rank
  - a quote below the leftover receives **0**
  - if all premiums are paid and \(S_R\) is still left, that leftover stays on this market’s surplus ledger (it is **not** extra bonus beyond premium)

This is **not** winner-take-all. Rank 1 does not scoop the whole \(S_R\).

Fees \(\phi\) are **always** the protocol platform (`claim_fees`), including haircut and VOID. \(S_P\) is **this market’s leftover** after winners are whole and after the 20% cover slice — not \(\phi\). Haircut / draw / VOID \(\Rightarrow S_P=0\).

### Story B — Trading is short, locked \(D\) (and maybe \(C_P\)) still make winners whole (\(T < L \le C_{\max}\))

- Shortfall \(H_{\mathrm{need}}=(L-T)^+\).
- Filled quotes are drawn, each at most filled \(D\), until the gap is closed. **More \(C_R\) is better.**
- \(\rho=1\). Surplus \(S=0\) (there is no leftover of \(T-L\)).
- After users are whole you may still claim **premium** for the filled quote.
- Unused \(D\) (if the gap did not need all of it) unlocks.

### Story C — Even \(C_R\) (and \(C_P\)) are not enough (\(L > C_{\max}\))

- Winners are paid at the same \(\rho=C_{\max}/L<1\).
- Risk capital is still drawn up to locked \(D\) to raise \(\rho\).
- **No surplus** (\(S=0\)). No surplus share, no 20% cover slice from **this** market.

### VOID / failed result

Collateral unlocks. No \(H\). No surplus. Traders get `cost_paid` back, not a \(\rho\) payout.

### How \(\rho\) is a number (includes \(C_P\))

\(C_P\) does **not** have a separate “how much % of this market” rule. It only feeds \(C_{\max}\):

1. \(T\) = this market’s trading revenue (fees never in it; premium is **not** taken out first)
2. If \(L\le T\): \(\rho=1\), \(C_P\) stays in the protocol pool
3. If \(L>T\): leftover after filled \(D\) is \((L-T-C_R)^+\). This market may take
   \(\min(\text{that leftover},\;\text{listing tap},\;\text{pool cash})\). Create default tap is **0**. Pool cash is only what `fund_pool` already deposited.
4. \(C_{\max}=T+C_R+C_P^{\mathrm{alloc}}\), then \(\rho=\min(1,C_{\max}/L)\)

Example: \(T=600\), \(L=1000\), \(C_R=300\), tap \(200\), pool \(50\) → \(C_P=50\), \(C_{\max}=950\), \(\rho=0.95\). Same numbers, tap \(0\) → \(\rho=0.90\).

---

## 7. How surplus is split (only Story A)

Only when \(\rho=1\) and \(S>0\):

```text
S  (leftover of trading revenue after paying L)
 ├─ 20%  →  protocol LP cover pool   (S_C)   ← not paid to this market’s rank
 └─ 80%  →  remainder S̃
      ├─ if this market has filled C_R:
      │     70% of S̃ → this market’s quotes (S_R)   default α_R = 7000 bps
      │     30% of S̃ → platform (S_P)
      └─ if nobody locked C_R:
            0 to quotes; all of S̃ to the platform
```

Worked default: \(S=100\) and \(C_R>0\) → **20 / 56 / 24** (cover / this market’s quotes / platform).  
Same \(S\), no \(C_R\) → **20 / 0 / 80**.

\(S_C\) is **one protocol account** (`LossPool`), not split among this market’s quotes. It exists so **losing** risk wallets (see §8) can be topped up later, including from **other** prediction markets that finished with surplus.

On `/lp`, after settlement: **Fund cover** moves this market’s \(S_C\) into that pool (any time). The vault shows lifetime \(\Pi\) and **cover paid**. **Claim cover** is signed by the protocol platform when it chooses to reimburse uncovered loss — there is no 7-day calendar window.

---

## 8. Lifetime P&L and the cover pool

Your UserVault keeps \(\Pi\) (`risk_pnl`) **across all prediction markets**:

| Event | \(\Pi\) |
| --- | --- |
| Draw \(H\) | minus \(H\) |
| Premium credited | plus premium |
| Surplus \(S_R\) credited | plus that credit |
| Cover paid to you | `cover_paid` increases; \(\Pi\) does **not** change |

Cover rule:

- Uncovered amount is \((-\Pi)^+ - \texttt{cover\_paid}\)
- Credit \(\min(\text{uncovered},\;\text{pool balance})\)
- Rank on **this** market does **not** order cover claims
- A wallet with uncovered \(=0\) gets **0** from the pool
- **Only the platform signs** `cover_lp_loss`. Timing is operational. Funding the pool from surplus is not gated.

The chain’s job is to **stat operating P&L and cover paid strictly**. Whether to reimburse this week is a platform choice — not a unix window.

A market that **drew** \(H\) has \(S=0\), so it does **not** fund the pool. Losers are reimbursed (partially) from **other** markets’ 20% surplus slices.

---

## 9. Caps (read this twice)

- You never lose more than **this quote’s filled \(D\)** on that market’s draw.
- Later traders raising live \(L_{\max}\) **do not** rewrite your locked \(D\).
- Premium is a **reward after users are whole**, not a haircut of winners.
- You cannot unwind a quote at the live LMSR price. Unused \(D\) unlocks at settlement or VOID.
- You cannot bid or claim with a Session key.
- USDC only (Circle SPL). SOL only pays network fees.

---

## 10. Numeric pictures

**No draw, ranked surplus.** \(T=1\,000\), \(L=800\) → \(S=200\).  
\(S_C=40\), remainder \(160\), \(S_R=112\), \(S_P=48\).

| Rank | \(D\) | Premium | Unit | Paid from \(S_R\) |
| --- | --- | --- | --- | --- |
| 1 | 100 | 5 | 0.05 | 5 |
| 2 | 100 | 8 | 0.08 | 8 |
| 3 | 200 | 20 | 0.10 | 20 |

Paid \(33\). Rank did not take \(112\) in one cheque. Each line stopped at its premium. Unlock all \(D\). \(\Pi\) on each wallet rises by that premium (and they were not drawn).

**Draw.** \(T=600\), \(L=1\,000\) → need \(400\). Three filled quotes \(D=100+100+200\). Each can be drawn up to \(D\); together they cover the \(400\). \(S=0\). After users are paid, claim premium; \(\Pi\) falls by \(H\) then rises by premium.

**Cover across markets.** Last month \(\Pi=-50\). This month a **different** market has \(S=100\) → \(20\) into the pool. When the platform signs **Claim cover**, it pays \(20\); \(\Pi\) stays \(-50\), `cover_paid` \(=20\), uncovered \(=30\). Next surplus can pay more until uncovered reaches \(0\).

---

## 11. Claim order on `/lp`

After `begin_settle`:

1. **Draw H** if Story B / C (this is the loss)
2. **Premium**
3. **Surplus** (Story A; ranked waterfall)
4. **Fund cover** (moves \(S_C\) into the protocol pool; any wallet may do this, any time)
5. **Unlock** unused \(D\) (or the VOID path)
6. **Claim cover** is platform-signed, if uncovered \((-\Pi)^+-\texttt{cover\_paid}>0\)

Do not skip draw when \(H\) is owed: that is how \(\Pi\) records the loss.

---

## 12. Formal pointers

| Topic | Spec |
| --- | --- |
| \(\rho\), surplus, \(S_C\), \(\Pi\) | Product §1.2.1 |
| Ledgers / who claims | Product §1.2.6 |
| Auction, one pool, rank | Product §6 / §6.6 |
| Settlement stack | Product §8.6 |
| SHALL text | SRS FR-RSK-01–08, FR-SET-04, FR-SET-12, FR-UI-23, FR-UI-37 |
| Chain | `programs/risk` matching; `programs/vault` settle; `crates/math` `surplus_split`, `waterfall_pay` |
