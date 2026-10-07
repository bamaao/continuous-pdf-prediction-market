"use client";

import {
  compose,
  fetchOwnerRisk,
  fetchPool,
  fetchVaultApi,
  listingHeadline,
  RiskQuoteItem,
  UserVaultSnap,
} from "@cpm/sdk";
import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import Link from "next/link";
import { useEffect, useState } from "react";
import { MARKET_API } from "@/lib/env";
import { NEED_WALLET, SESSION_BOARD_ONLY } from "@/lib/copy";
import { sendSigned } from "@/lib/tx";

type LpOp = "draw_lp" | "pay_premium" | "release_lp" | "pay_surplus_lp" | "pay_surplus_cover" | "cover_lp_loss";

export default function LpPage() {
  const { connection } = useConnection();
  const { publicKey, signTransaction } = useWallet();
  const [items, setItems] = useState<RiskQuoteItem[]>([]);
  const [vault, setVault] = useState<UserVaultSnap | null>(null);
  const [coverPool, setCoverPool] = useState(0);
  const [note, setNote] = useState("");

  const pnl = vault?.risk_pnl ?? 0;
  const coverPaid = vault?.cover_paid ?? 0;
  const uncovered = pnl >= 0 ? 0 : Math.max(0, -pnl - coverPaid);
  const me = publicKey?.toBase58() ?? "";
  const coverRow = items.find((r) => r.market);
  const canCover = !!me && !!coverRow?.platform && me === coverRow.platform;

  async function load() {
    if (!publicKey) {
      setItems([]);
      setVault(null);
      setCoverPool(0);
      return;
    }
    const owner = publicKey.toBase58();
    const [r, v, pool] = await Promise.all([
      fetchOwnerRisk(MARKET_API, owner),
      fetchVaultApi(MARKET_API, owner),
      fetchPool(MARKET_API),
    ]);
    setItems(r.items ?? []);
    setVault(v);
    setCoverPool(pool.cover_pool ?? 0);
  }

  useEffect(() => {
    load().catch(() => setItems([]));
  }, [publicKey]);

  async function act(op: LpOp, row: RiskQuoteItem) {
    if (!publicKey || !signTransaction) {
      setNote(NEED_WALLET);
      return;
    }
    if (op === "cover_lp_loss") {
      const plat = row.platform || items.find((r) => r.market === row.market)?.platform;
      if (!plat || publicKey.toBase58() !== plat) {
        setNote("Only this market's Market.platform may reimburse cover. This vault still records Π and cover paid.");
        return;
      }
    }
    try {
      const sig = await sendSigned(connection, signTransaction, publicKey, [
        await compose(MARKET_API, {
          op,
          owner: publicKey.toBase58(),
          market: row.market,
          layer: row.layer,
          weight_sum: row.weight_sum || 1,
          trader: op === "cover_lp_loss" ? publicKey.toBase58() : undefined,
        }),
      ]);
      setNote(`${op} ${sig}`);
      await load();
    } catch (e) {
      setNote(e instanceof Error ? e.message : "claim failed");
    }
  }

  const coverMarket = coverRow?.market ?? "";

  return (
    <div>
      <h1 className="font-display text-5xl">Risk LP</h1>
      <p className="mt-2 max-w-2xl text-sm leading-relaxed text-paper/70">
        Quotes you already locked. After settlement: draw H first, then premium, then surplus. A surplus market
        pays 20% into the protocol cover pool. The chain keeps lifetime risk P&amp;L and cover paid on your vault.
        The platform reimburses uncovered loss from the pool when it chooses — there is no calendar window. Fund
        cover (inflow) is always allowed. Browse the{" "}
        <Link href="/auctions" className="text-amber">
          auction book
        </Link>{" "}
        to post a new quote. {SESSION_BOARD_ONLY}
      </p>
      {!publicKey && <p className="mt-6 font-mono text-xs text-paper/50">Connect a wallet to see quotes you locked.</p>}
      {publicKey && (
        <div className="mt-6 flex flex-wrap items-center gap-3 font-mono text-[11px] text-paper/45">
          <span>{items.length} indexed quotes for this wallet.</span>
          <span>risk P&amp;L {pnl} USDC</span>
          <span>cover paid {coverPaid} USDC</span>
          <span>uncovered {uncovered} USDC</span>
          <span>cover pool {coverPool} USDC</span>
          {uncovered > 0 && (
            <button
              className="border border-amber px-2 py-1 text-amber disabled:cursor-not-allowed disabled:opacity-40"
              disabled={!canCover || !coverMarket}
              title={
                canCover
                  ? "Reimburse uncovered (−Π)⁺ from the cover pool"
                  : "Only this market's Market.platform signs cover_lp_loss"
              }
              onClick={() =>
                act("cover_lp_loss", {
                  quote: "",
                  market: coverMarket,
                  platform: coverRow?.platform,
                  layer: 1,
                  capacity: 0,
                  filled: 0,
                  d_i: 0,
                  premium: 0,
                  premium_owed: 0,
                  profit_share_bps: 0,
                  cancelled: false,
                  expected_h: 0,
                  attachment: 0,
                  weight_sum: 1,
                })
              }
            >
              Claim cover
            </button>
          )}
        </div>
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
                  {typeof row.pnl === "number" ? ` · quote P&L ${row.pnl}` : ""}
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
                    <button className="border border-rule px-2 py-1" onClick={() => act("pay_surplus_cover", row)}>
                      Fund cover
                    </button>
                  </>
                )}
                {refund && (
                  <button className="border border-rule px-2 py-1" onClick={() => act("release_lp", row)}>
                    Unlock D
                  </button>
                )}
              </div>
            </li>
          );
        })}
      </ul>
      {note && <p className="mt-4 font-mono text-xs text-amber">{note}</p>}
    </div>
  );
}
