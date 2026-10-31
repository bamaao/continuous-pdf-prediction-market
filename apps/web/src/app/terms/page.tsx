import type { Metadata } from "next";

export const metadata: Metadata = {
  title: "Terms — Continuous",
};

export default function TermsPage() {
  return (
    <article className="max-w-2xl space-y-4 text-sm leading-relaxed text-paper/80">
      <p className="font-mono text-[11px] uppercase tracking-[0.2em] text-amber">Android app</p>
      <h1 className="font-display text-5xl text-paper">Terms</h1>
      <p>
        Continuous is a Solana prediction market. The Android app submits transactions you sign in your own wallet
        through the Mobile Wallet Adapter. The app does not hold your keys, seed phrase, or tokens.
      </p>
      <p>
        Buys pay Circle USDC from your vault. A hit pays face value, then the settlement ratio. Coverage shown while a
        market is open is not the payout. Trading can stop when a market is halted, past its close time, or already
        settled. Sells stay closed.
      </p>
      <p>
        You choose the wallet that approves each signature. A signature you approve is your instruction. Network fees
        are paid in SOL. Failed or expired transactions can still cost a fee.
      </p>
      <p>
        Market data comes from the Continuous market service and from Solana. Prices, quotes, and coverage can move
        before a transaction confirms. Nothing in the app is a promise of profit or an offer of custody.
      </p>
      <p>These terms apply to the Android app distributed for the Solana dApp Store and to this website.</p>
    </article>
  );
}
