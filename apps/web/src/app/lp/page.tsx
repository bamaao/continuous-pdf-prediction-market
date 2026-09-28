"use client";

import { compose, fetchOwnerRisk, listingHeadline, RiskQuoteItem } from "@cpm/sdk";
import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import Link from "next/link";
import { useEffect, useState } from "react";
import { MARKET_API } from "@/lib/env";
import { NEED_WALLET, SESSION_BOARD_ONLY } from "@/lib/copy";
import { sendSigned } from "@/lib/tx";

type LpOp = "draw_lp" | "pay_premium" | "release_lp" | "pay_surplus_lp";

export default function LpPage() {
  const { connection } = useConnection();
  const { publicKey, signTransaction } = useWallet();
  const [items, setItems] = useState<RiskQuoteItem[]>([]);
  const [note, setNote] = useState("");

  async function load() {
    if (!publicKey) {
      setItems([]);
      return;
    }
    const r = await fetchOwnerRisk(MARKET_API, publicKey.toBase58());
    setItems(r.items ?? []);
  }

  useEffect(() => {
    load().catch(() => setItems([]));
  }, [publicKey]);

  async function act(op: LpOp, row: RiskQuoteItem) {
    if (!publicKey || !signTransaction) {
      setNote(NEED_WALLET);
      return;
    }
    try {
      const sig = await sendSigned(connection, signTransaction, publicKey, [
        await compose(MARKET_API, {
          op,
          owner: publicKey.toBase58(),
          market: row.market,
          layer: row.layer,
          weight_sum: row.weight_sum || 1,
        }),
      ]);
      setNote(`${op} ${sig}`);
      await load();
    } catch (e) {
      setNote(e instanceof Error ? e.message : "claim failed");
    }
  }

  return (
    <div>
      <h1 className="font-display text-5xl">Risk LP</h1>
      <p className="mt-2 max-w-2xl text-sm leading-relaxed text-paper/70">
        Quotes you already locked. After settlement: draw H first, then premium, then surplus. VOID unlocks
        reserved D. Browse the <Link href="/auctions" className="text-amber">auction book</Link> to post a new
        layer quote. {SESSION_BOARD_ONLY}
      </p>
      {!publicKey && <p className="mt-6 font-mono text-xs text-paper/50">Connect a wallet to see quotes you locked.</p>}
      {publicKey && (
        <p className="mt-6 font-mono text-[11px] text-paper/45">{items.length} indexed quotes for this wallet.</p>
      )}
      <ul className="mt-3 divide-y divide-rule border border-rule">
        {items.map((row) => {
          const settled = (row.board_phase ?? 0) === 1;
          const refund = (row.board_phase ?? 0) === 2;
          return (
            <li key={row.quote} className="grid gap-2 px-4 py-3 font-mono text-[11px] sm:grid-cols-[1fr_auto]">
              <div>
                <p className="text-amber">
                  layer {row.layer} · D_i {row.d_i} · Ĥ {row.expected_h}
                  {settled ? " · settled" : refund ? " · VOID" : ""}
                </p>
                <p className="mt-0.5 font-display text-base text-paper">{listingHeadline(row)}</p>
                <p className="mt-0.5 break-all text-paper/40">{row.market}</p>
                <p className="mt-1 text-paper/55">
                  filled {row.filled}/{row.capacity} · premium owed {row.premium_owed} · share {row.profit_share_bps} bps
                  {row.cancelled ? " · cancelled" : ""}
                </p>
              </div>
              <div className="flex flex-col gap-1">
                <Link href={`/auction/${row.market}`} className="text-amber">
                  Auction
                </Link>
                {settled && (
                  <>
                    <button className="border border-amber px-2 py-1 text-amber" onClick={() => act("draw_lp", row)}>
                      Draw H
                    </button>
                    <button className="border border-rule px-2 py-1" onClick={() => act("pay_premium", row)}>
                      Premium
                    </button>
                    <button className="border border-rule px-2 py-1" onClick={() => act("pay_surplus_lp", row)}>
                      Surplus
                    </button>
                  </>
                )}
                {refund && (
                  <button className="border border-rule px-2 py-1" onClick={() => act("release_lp", row)}>
                    Unlock
                  </button>
                )}
              </div>
            </li>
          );
        })}
        {publicKey && !items.length && (
          <li className="px-4 py-8 font-mono text-sm text-paper/50">
            No filled layers yet. Browse <Link href="/auctions" className="text-amber">published books</Link> and quote a layer.
          </li>
        )}
      </ul>
      {note && <p className="mt-4 font-mono text-xs text-paper/60">{note}</p>}
    </div>
  );
}
