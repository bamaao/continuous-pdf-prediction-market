import type { Metadata } from "next";

export const metadata: Metadata = {
  title: "Privacy — Continuous",
};

export default function PrivacyPage() {
  return (
    <article className="max-w-2xl space-y-4 text-sm leading-relaxed text-paper/80">
      <p className="font-mono text-[11px] uppercase tracking-[0.2em] text-amber">Android app</p>
      <h1 className="font-display text-5xl text-paper">Privacy</h1>
      <p>
        The Android app reads public market data from the Continuous market service. When you connect a wallet, the
        Mobile Wallet Adapter returns your public key to the app. The app uses that address to load your vault,
        tickets, and quotes, and to build transactions.
      </p>
      <p>
        Signing stays inside your wallet. The app does not ask for, transmit, or store a seed phrase or private key.
        Signed transactions are sent to the Solana RPC configured in the build. Fills and account data on Solana are
        public.
      </p>
      <p>
        The app does not sell personal data. It does not include an advertising SDK. Support mail you send to the
        publisher is used only to answer that request.
      </p>
      <p>
        Publisher identity checks required by the Solana dApp Store are handled by Solana Mobile, not by this app.
      </p>
    </article>
  );
}
