"use client";

import { coveragePct, familyName, listingHeadline, listMarketsPage, type MarketListItem } from "@cpm/sdk";
import Link from "next/link";
import { useEffect, useState } from "react";
import { MARKET_API } from "@/lib/env";

/** Live list of each prediction market's thickest interval overlap. Polls the indexer projection. */
export function RiskTape({ limit = 20 }: { limit?: number }) {
  const [rows, setRows] = useState<MarketListItem[]>([]);
  const [err, setErr] = useState("");
  const [at, setAt] = useState(0);

  useEffect(() => {
    let stop = false;
    const pull = () => {
      listMarketsPage(MARKET_API, { limit, page: 1 })
        .then((page) => {
          if (stop) return;
          const items = [...(page.items ?? [])].sort(
            (a, b) => (b.peak_risk?.payout_usdc ?? b.l_max_usdc ?? 0) - (a.peak_risk?.payout_usdc ?? a.l_max_usdc ?? 0),
          );
          setRows(items);
          setAt(Date.now());
          setErr("");
        })
        .catch((e) => {
          if (!stop) setErr(e instanceof Error ? e.message : "risk tape unreachable");
        });
    };
    pull();
    const id = window.setInterval(pull, 2000);
    return () => {
      stop = true;
      window.clearInterval(id);
    };
  }, [limit]);

  return (
    <section className="mt-8">
      <p className="font-mono text-[11px] uppercase tracking-widest text-amber">Highest risk payout</p>
      <p className="mt-2 max-w-2xl text-[12px] text-paper/50">
        During trading this is where bought intervals stack the thickest, refreshed every 2s. Gaussian /
        lognormal show that overlap as a print band on Ω — not a volume ranking of tickets. Settlement still
        pays only the realized print.
      </p>
      {err && <p className="mt-3 font-mono text-[11px] text-rust">{err}</p>}
      <ul className="mt-3 divide-y divide-rule border border-rule font-mono text-[11px]">
        {rows.map((m) => {
          const peak = m.peak_risk;
          const pay = peak?.payout_usdc ?? m.l_max_usdc ?? 0;
          return (
            <li key={m.market} className="flex flex-wrap items-baseline justify-between gap-2 px-3 py-2">
              <div>
                <Link href={`/m/${m.market}`} className="font-display text-sm text-amber">
                  {listingHeadline(m)}
                </Link>
                <span className="ml-2 text-paper/40">{familyName(m.family)}</span>
                <p className="mt-0.5 text-paper/60">
                  {pay > 0 ? `if ${peak?.label ?? "thickest overlap"}` : "no open liability yet"}
                </p>
              </div>
              <div className="text-right">
                <p className="text-paper">{pay} USDC</p>
                <p className="text-paper/40">coverage {coveragePct(m.l_max_usdc, m.coverage_bps)}</p>
              </div>
            </li>
          );
        })}
        {!rows.length && !err && <li className="px-3 py-6 text-paper/45">No live prediction markets.</li>}
      </ul>
      {at > 0 && (
        <p className="mt-2 font-mono text-[10px] text-paper/35">
          snapshot {new Date(at).toLocaleTimeString()} · indexer projection
        </p>
      )}
    </section>
  );
}
