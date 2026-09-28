"use client";

import {
  AuctionLayer,
  formatTags,
  compose,
  coverageLow,
  coveragePct,
  familyName,
  fetchInfo,
  fetchLayers,
  fetchListing,
  fetchOwnerRisk,
  listingHeadline,
  RiskQuoteItem,
  StandingQuote,
  statusName,
} from "@cpm/sdk";
import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import Link from "next/link";
import { useCallback, useEffect, useMemo, useState } from "react";
import { MarketCard } from "./market-card";
import { MARKET_API } from "@/lib/env";
import { NEED_WALLET, SESSION_BOARD_ONLY } from "@/lib/copy";
import { sendSigned } from "@/lib/tx";

function unitPremium(premium: number, capacity: number): number {
  if (capacity <= 0) return 0;
  return Math.floor((premium * 1_000_000) / capacity);
}

function shortKey(k: string): string {
  if (k.length < 12) return k;
  return `${k.slice(0, 4)}…${k.slice(-4)}`;
}

function rankLine(mine: number, best: number | null, count: number): { text: string; warn: boolean } {
  if (mine <= 0) return { text: "set capacity and premium", warn: true };
  if (best == null || count === 0) return { text: "first quote on this layer", warn: false };
  if (mine < best) return { text: "would be first — cheaper than the book", warn: false };
  if (mine === best) return { text: "tied for cheapest — earlier timestamp wins", warn: false };
  return { text: `behind ${count} standing quote${count === 1 ? "" : "s"}`, warn: true };
}

/** H_i(L) = min(D, T, max(L − A, 0)) on this published slice. */
function hIf(L: number, attachment: number, thickness: number, d: number): number {
  if (L <= attachment) return 0;
  return Math.min(Math.max(0, d), Math.max(0, thickness), L - attachment);
}

export function AuctionDesk({ market }: { market: string }) {
  const { connection } = useConnection();
  const { publicKey, signTransaction } = useWallet();
  const [layers, setLayers] = useState<AuctionLayer[]>([]);
  const [quotes, setQuotes] = useState<StandingQuote[]>([]);
  const [cR, setCR] = useState<number | null>(null);
  const [lMax, setLMax] = useState<number | null>(null);
  const [coverage, setCoverage] = useState<number | null>(null);
  const [family, setFamily] = useState<number | null>(null);
  const [status, setStatus] = useState<number | null>(null);
  const [title, setTitle] = useState("");
  const [tags, setTags] = useState<string[]>([]);
  const [description, setDescription] = useState("");
  const [event, setEvent] = useState("");
  const [closeTs, setCloseTs] = useState<number | null>(null);
  const [cMax, setCMax] = useState<number | null>(null);
  const [payable, setPayable] = useState<number | null>(null);
  const [abnormal, setAbnormal] = useState("");
  const [finalResult, setFinalResult] = useState("");
  const [reportOpen, setReportOpen] = useState<number | null>(null);
  const [layerId, setLayerId] = useState(1);
  const [capacity, setCapacity] = useState(100);
  const [premium, setPremium] = useState(1);
  const [shareBps, setShareBps] = useState(0);
  const [mine, setMine] = useState<RiskQuoteItem[]>([]);
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [nowSec, setNowSec] = useState(() => Math.floor(Date.now() / 1000));

  const load = useCallback(async () => {
    const [book, desk] = await Promise.all([fetchLayers(MARKET_API, market), fetchInfo(MARKET_API, market).catch(() => null)]);
    setCR(book.c_r);
    setLMax(book.l_max_usdc ?? desk?.l_max_usdc ?? null);
    setCoverage(book.coverage_bps ?? desk?.coverage_bps ?? null);
    setFamily(book.family ?? desk?.family ?? null);
    setStatus(book.status ?? desk?.status ?? null);
    if (book.title || desk?.title) setTitle(book.title || desk?.title || "");
    if (desk?.tags?.length) setTags(desk.tags);
    else if (book.category || desk?.category) setTags([book.category || desk?.category || ""]);
    if (book.description || desk?.description) setDescription(book.description || desk?.description || "");
    if (book.event || desk?.event) setEvent(book.event || desk?.event || "");
    if (book.close_ts || desk?.close_ts) setCloseTs(book.close_ts || desk?.close_ts || null);
    setCMax(desk?.c_max_usdc ?? null);
    setPayable(desk?.payable_usdc ?? null);
    setAbnormal(desk?.abnormal ?? "");
    setFinalResult(desk?.final_result ?? "");
    setReportOpen(desk?.report_open_ts ?? desk?.close_ts ?? null);
    setLayers(book.layers);
    setQuotes(book.quotes ?? []);
    setLayerId((cur) => (book.layers.some((l) => l.id === cur) ? cur : book.layers[0]?.id ?? 1));
    const listing = await fetchListing(MARKET_API, market);
    if (listing?.title) setTitle(listing.title);
    if (listing?.tags?.length) setTags(listing.tags);
    else if (listing?.category) setTags([listing.category]);
    if (listing?.description) setDescription(listing.description);
    if (listing?.event) setEvent(listing.event);
  }, [market]);

  useEffect(() => {
    load().catch(() => undefined);
  }, [load]);

  useEffect(() => {
    const id = window.setInterval(() => setNowSec(Math.floor(Date.now() / 1000)), 1000);
    return () => window.clearInterval(id);
  }, []);

  useEffect(() => {
    if (!publicKey) {
      setMine([]);
      return;
    }
    fetchOwnerRisk(MARKET_API, publicKey.toBase58())
      .then((r) => setMine((r.items ?? []).filter((q) => q.market === market)))
      .catch(() => setMine([]));
  }, [publicKey, market]);

  const selected = layers.find((l) => l.id === layerId) ?? layers[0];
  const standing = useMemo(
    () => quotes.filter((q) => q.layer === (selected?.id ?? layerId)).sort((a, b) => a.unit_premium - b.unit_premium),
    [quotes, selected, layerId],
  );
  const mineUnit = unitPremium(premium, capacity);
  const best = standing[0]?.unit_premium ?? selected?.unit_premium ?? null;
  const rank = rankLine(mineUnit, best, standing.length);
  const lockWarn = capacity > (selected?.remaining ?? 0);
  const A = selected?.attachment ?? 0;
  const T = Math.max(1, selected?.thickness ?? selected?.remaining ?? 0);
  const scenarios = [
    { label: "L ≤ A — layer not drawn", L: A, h: hIf(A, A, T, capacity) },
    { label: "L mid-layer", L: A + Math.max(1, Math.floor(T / 2)), h: hIf(A + Math.max(1, Math.floor(T / 2)), A, T, capacity) },
    { label: "L ≥ A+T — layer fully used", L: A + T, h: hIf(A + T, A, T, capacity) },
  ];
  const stackMax = Math.max(1, ...layers.map((l) => l.attachment + Math.max(1, l.thickness ?? l.remaining)));

  const auctionClosed = closeTs != null && closeTs > 0 && nowSec >= closeTs;

  async function bid() {
    if (auctionClosed) {
      setNote("trading closed — no new risk quotes after the deadline");
      return;
    }
    if (!publicKey || !signTransaction) {
      setNote(NEED_WALLET);
      return;
    }
    if (capacity <= 0 || premium <= 0) {
      setNote("capacity and premium must be > 0");
      return;
    }
    if (shareBps < 0 || shareBps > 10_000) {
      setNote("profit share is 0–10000 bps");
      return;
    }
    setBusy(true);
    setNote("signing…");
    try {
      const quoteIx = () =>
        compose(MARKET_API, {
          op: "risk_quote",
          owner: publicKey.toBase58(),
          market,
          layer: selected?.id ?? 1,
          capacity,
          premium,
          profit_share_bps: shareBps,
        });
      try {
        const sig = await sendSigned(connection, signTransaction, publicKey, [await quoteIx()]);
        setNote(`quoted ${sig}`);
      } catch (first) {
        const msg = first instanceof Error ? first.message : "";
        if (!/AccountNotInitialized|already in use|already initialized/i.test(msg)) throw first;
        try {
          await sendSigned(connection, signTransaction, publicKey, [
            await compose(MARKET_API, { op: "risk_open_book", owner: publicKey.toBase58(), market }),
          ]);
        } catch {
          /* racing open */
        }
        const sig = await sendSigned(connection, signTransaction, publicKey, [await quoteIx()]);
        setNote(`quoted ${sig}`);
      }
      await load();
      if (publicKey) {
        const r = await fetchOwnerRisk(MARKET_API, publicKey.toBase58());
        setMine((r.items ?? []).filter((q) => q.market === market));
      }
    } catch (e) {
      setNote(e instanceof Error ? e.message : "bid failed");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div>
      <p className="font-mono text-[11px] uppercase tracking-[0.2em] text-amber">Risk auction</p>
      <h1 className="mt-1 font-display text-4xl">{listingHeadline({ title, family: family ?? undefined, market })}</h1>
      <p className="mt-1 font-mono text-xs text-amber">
        {tags.length ? formatTags(tags) : "untagged"}
        {family != null ? ` · ${familyName(family)}` : ""}
        {status != null ? ` · ${statusName(status)}` : ""}
      </p>
      {event ? <p className="mt-2 max-w-3xl text-lg leading-snug text-paper/80">{event}</p> : null}
      {description ? (
        <p className="mt-2 max-w-3xl whitespace-pre-wrap text-sm leading-relaxed text-paper/70">{description}</p>
      ) : null}
      <p className="mt-1 break-all font-mono text-[11px] text-paper/40">{market}</p>
      <div className="mt-4 border border-rule p-4">
        <p className="font-mono text-[11px] uppercase tracking-widest text-amber">Market card</p>
        <MarketCard
          compact
          row={{
            market,
            title,
            tags,
            category: tags[0],
            description,
            event,
            family: family ?? undefined,
            status: status ?? undefined,
            close_ts: closeTs ?? undefined,
            report_open_ts: reportOpen ?? undefined,
            abnormal,
            final_result: finalResult,
            l_max_usdc: lMax ?? undefined,
            c_max_usdc: cMax ?? undefined,
            payable_usdc: payable ?? undefined,
          }}
        />
      </div>
      <p className="mt-4 max-w-2xl text-sm leading-relaxed text-paper/70">
        You lock unused margin <span className="text-paper">D</span> against a published layer. If that layer is
        drawn, loss <span className="text-paper">H</span> comes from locked D first. Lowest premium per unit of D
        fills first, then earlier timestamp. LPs cannot invent an attachment. The book closes at{" "}
        <span className="text-paper">close_ts</span> with the prediction market — no quotes after trading
        cutoff. {SESSION_BOARD_ONLY}
      </p>
      <div className="mt-4 flex flex-wrap gap-4 font-mono text-[11px] uppercase">
        <Link href={`/m/${market}`} className="text-amber">
          Open market
        </Link>
        <Link href="/lp">Your quotes</Link>
        <Link href="/auctions">All auctions</Link>
      </div>

      <dl className="mt-6 grid grid-cols-2 gap-3 border border-rule p-4 font-mono text-xs sm:grid-cols-4">
        <Stat k="C_R" v={cR == null ? "—" : `${cR} USDC`} hint="locked + filled risk capital" />
        <Stat k="L_max" v={lMax == null ? "—" : `${lMax} USDC`} hint="expected max payout" />
        <Stat
          k="Coverage"
          v={coveragePct(lMax, coverage)}
          hint="C_max / L_max — undefined while L_max is 0"
          warn={coverageLow(lMax, coverage)}
        />
        <Stat k="Layers" v={String(layers.length)} hint="published only" />
      </dl>

      <section className="mt-8 border border-rule p-4">
        <p className="font-mono text-[11px] uppercase tracking-widest text-amber">Layer stack</p>
        <p className="mt-2 max-w-2xl text-[12px] text-paper/50">
          Attachment A is loss already taken before this slice pays. Thickness T is how much L this layer can
          absorb. You cannot invent A.
        </p>
        <div className="mt-4 space-y-3">
          {layers.map((l) => {
            const thick = Math.max(1, l.thickness ?? l.remaining + (l.filled ?? 0));
            const left = Math.round((l.attachment / stackMax) * 100);
            const width = Math.max(4, Math.round((thick / stackMax) * 100));
            const used = Math.min(100, Math.round(((l.filled ?? 0) / thick) * 100));
            const on = l.id === (selected?.id ?? layerId);
            return (
              <button
                key={l.id}
                type="button"
                onClick={() => setLayerId(l.id)}
                className={`w-full border px-3 py-3 text-left font-mono text-[11px] ${on ? "border-amber bg-amber/10" : "border-rule hover:border-paper/30"}`}
              >
                <div className="flex items-center justify-between gap-3">
                  <span className={on ? "text-amber" : ""}>
                    Layer {l.id} · A={l.attachment} → {l.attachment + thick}
                  </span>
                  <span className="text-paper/50">{l.quotes} quotes</span>
                </div>
                <div className="relative mt-2 h-2 bg-paper/10">
                  <div className="absolute inset-y-0 bg-amber/70" style={{ left: `${left}%`, width: `${width}%` }}>
                    <div className="h-full bg-amber" style={{ width: `${used}%` }} />
                  </div>
                </div>
                <p className="mt-2 text-paper/55">
                  remaining D {l.remaining} · filled {l.filled ?? 0} · best unit {l.unit_premium} · γ {l.gamma_bps}{" "}
                  bps
                </p>
              </button>
            );
          })}
          {!layers.length && <p className="font-mono text-[11px] text-paper/45">No published layers indexed.</p>}
        </div>
      </section>

      <section className="mt-8 grid gap-8 lg:grid-cols-[1.15fr_0.85fr]">
        <div>
          <p className="font-mono text-[11px] uppercase tracking-widest text-amber">If this layer is drawn</p>
          <p className="mt-2 text-[12px] text-paper/50">
            H is taken from your locked D first. These rows use the D you are about to quote on layer{" "}
            {selected?.id ?? "—"}.
          </p>
          <ul className="mt-3 divide-y divide-rule border border-rule font-mono text-[11px]">
            {scenarios.map((s) => (
              <li key={s.label} className="flex justify-between gap-3 px-3 py-2">
                <span className="text-paper/55">{s.label}</span>
                <span className={s.h > 0 ? "text-amber" : ""}>H = {s.h} USDC</span>
              </li>
            ))}
          </ul>
          <p className="mt-2 font-mono text-[10px] text-paper/40">
            H = min(D, T, max(L − A, 0)). Unused D unlocks on VOID / unused remainder after settlement.
          </p>

          <p className="mt-8 font-mono text-[11px] uppercase tracking-widest text-amber">Standing ladder</p>
          <p className="mt-2 text-[12px] text-paper/50">
            Fill order: lowest unit premium, then earlier timestamp. Your quote is not in this book until the
            chain confirms.
          </p>
          <ul className="mt-3 divide-y divide-rule border border-rule font-mono text-[11px]">
            {standing.map((q, i) => (
              <li key={q.quote} className="grid grid-cols-[2rem_1fr_auto] gap-2 px-3 py-2">
                <span className="text-paper/40">{i + 1}</span>
                <span>
                  {shortKey(q.lp)} · D {q.capacity} · filled {q.filled} · share {q.profit_share_bps} bps
                </span>
                <span className="text-amber">unit {q.unit_premium}</span>
              </li>
            ))}
            {!standing.length && (
              <li className="px-3 py-4 text-paper/45">No standing quotes on this layer yet. First quote ranks first.</li>
            )}
          </ul>
        </div>

        <aside className="border border-amber/40 bg-amber/5 p-4 font-mono text-xs">
          <p className="uppercase tracking-widest text-amber">{auctionClosed ? "Auction closed" : "Quote ticket"}</p>
          <p className="mt-2 text-[10px] leading-relaxed text-paper/50">
            {auctionClosed
              ? "Trading closed at close_ts. New risk quotes stop with the prediction market."
              : "Lock is a vault reserve, not a transfer out. After settlement, claim H then premium / surplus on /lp. VOID unlocks unused D."}
          </p>
          <label className="mt-4 block text-[10px] uppercase text-paper/50">
            Capacity D
            <input
              className="mt-1 w-full border border-rule bg-ink px-2 py-1"
              type="number"
              min={1}
              value={capacity}
              onChange={(e) => setCapacity(Number(e.target.value))}
            />
          </label>
          <label className="mt-3 block text-[10px] uppercase text-paper/50">
            Premium
            <input
              className="mt-1 w-full border border-rule bg-ink px-2 py-1"
              type="number"
              min={1}
              value={premium}
              onChange={(e) => setPremium(Number(e.target.value))}
            />
          </label>
          <label className="mt-3 block text-[10px] uppercase text-paper/50">
            Profit share (bps)
            <input
              className="mt-1 w-full border border-rule bg-ink px-2 py-1"
              type="number"
              min={0}
              max={10000}
              value={shareBps}
              onChange={(e) => setShareBps(Number(e.target.value))}
            />
          </label>
          <div className="mt-4 space-y-1.5 border-t border-rule/60 pt-3">
            <Row k="Layer" v={selected ? String(selected.id) : "—"} />
            <Row k="Attaches" v={selected ? `A=${A} · T=${T}` : "—"} />
            <Row k="You lock now" v={`${capacity} USDC`} />
            <Row k="Unit premium" v={mineUnit ? String(mineUnit) : "—"} />
            <Row k="Rank" v={rank.text} warn={rank.warn} />
            <Row k="If layer fully drawn" v={`H ≤ ${hIf(A + T, A, T, capacity)} USDC`} />
          </div>
          {lockWarn && (
            <p className="mt-3 text-amber">
              D is larger than remaining {selected?.remaining ?? 0}. The quote is still allowed; fill is min(D,
              remaining).
            </p>
          )}
          <button
            className="mt-4 w-full bg-amber py-2 text-ink disabled:opacity-50"
            disabled={busy || auctionClosed}
            onClick={bid}
          >
            {busy ? "Posting…" : auctionClosed ? "Trading closed" : "Quote layer"}
          </button>
          {note && <p className={`mt-2 text-[10px] ${/failed|Error|error/i.test(note) ? "text-rust" : "text-paper/60"}`}>{note}</p>}
        </aside>
      </section>

      {mine.length > 0 && (
        <section className="mt-10">
          <p className="font-mono text-[11px] uppercase tracking-widest text-amber">Your quotes on this market</p>
          <ul className="mt-3 divide-y divide-rule border border-rule font-mono text-[11px]">
            {mine.map((q) => (
              <li key={q.quote} className="px-3 py-2">
                layer {q.layer} · D {q.d_i} · filled {q.filled}/{q.capacity} · premium owed {q.premium_owed}
                {q.expected_h != null ? ` · expected H ${q.expected_h}` : ""}
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}

function Stat({ k, v, hint, warn }: { k: string; v: string; hint: string; warn?: boolean }) {
  return (
    <div className={warn ? "text-amber" : ""}>
      <dt className="text-[10px] uppercase tracking-widest text-paper/45">{k}</dt>
      <dd className="mt-1 text-sm">{v}</dd>
      <p className="mt-0.5 text-[10px] text-paper/35">{hint}</p>
    </div>
  );
}

function Row({ k, v, warn }: { k: string; v: string; warn?: boolean }) {
  return (
    <div className={`flex justify-between gap-3 py-1 ${warn ? "text-amber" : ""}`}>
      <span className="text-paper/50">{k}</span>
      <span className="text-right">{v}</span>
    </div>
  );
}
