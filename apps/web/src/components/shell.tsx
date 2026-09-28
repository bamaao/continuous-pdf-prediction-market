"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { Inbox } from "./inbox";
import { WalletBar } from "./wallet-bar";

const NAV = [
  ["Lobby", "/"],
  ["Auctions", "/auctions"],
  ["Portfolio", "/portfolio"],
  ["LP", "/lp"],
  ["Create", "/create"],
  ["审核", "/review"],
  ["Tags", "/tags"],
  ["Committee", "/committee"],
  ["Ops", "/ops"],
] as const;

export function Shell({ children }: { children: React.ReactNode }) {
  const path = usePathname();
  return (
    <div className="relative min-h-screen">
      <div className="grain" />
      <header className="sticky top-0 z-20 border-b border-rule/70 bg-ink/80 backdrop-blur">
        <div className="mx-auto flex max-w-6xl items-end justify-between gap-6 px-5 py-4">
          <div>
            <Link href="/" className="font-display text-3xl tracking-tight">
              Continuous
            </Link>
            <p className="mt-1 font-mono text-[11px] uppercase tracking-[0.22em] text-amber">
              PDF prediction market
            </p>
          </div>
          <nav className="flex flex-wrap items-center gap-4 font-mono text-xs uppercase tracking-widest">
            {NAV.map(([label, href]) => (
              <Link
                key={href}
                href={href}
                className={path === href || (href !== "/" && path.startsWith(href)) ? "text-amber" : "text-paper/70 hover:text-paper"}
              >
                {label}
              </Link>
            ))}
            <Inbox />
            <WalletBar />
          </nav>
        </div>
      </header>
      <main className="mx-auto max-w-6xl px-5 py-8">{children}</main>
      <footer className="mx-auto max-w-6xl px-5 pb-10 font-mono text-[11px] leading-relaxed text-paper/45">
        Fills are public on Solana. This product does not promise on-chain anonymity. Circle USDC only.
        Session revoke is not the same as disconnecting a wallet. Keeper writes stay on the CLI.
      </footer>
    </div>
  );
}
