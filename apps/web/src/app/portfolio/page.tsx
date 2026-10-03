"use client";

import { compose, familyName, fetchOwnerPositions, lastTicketFor, listingHeadline, OwnerPositionsPage, PositionTicket, pushInbox, statusName, ticketForHash, ticketPrompt } from "@cpm/sdk";
import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import Link from "next/link";
import { useCallback, useEffect, useState } from "react";
import { CashDesk } from "@/components/cash-desk";
import { MARKET_API } from "@/lib/env";
import { SESSION_BOARD_ONLY } from "@/lib/copy";
import { sendSigned } from "@/lib/tx";

function signedUsdc(n: number): string {
  if (n > 0) return `+${n} USDC`;
  return `${n} USDC`;
}

export default function Portfolio() {
  const { connection } = useConnection();
  const { publicKey, signTransaction } = useWallet();
  const [note, setNote] = useState("");
  const [q, setQ] = useState("");
  const [filter, setFilter] = useState("");
  const [page, setPage] = useState(1);
  const [book, setBook] = useState<OwnerPositionsPage | null>(null);
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    if (!publicKey) {
      setBook(null);
      return;
    }
    const p = await fetchOwnerPositions(MARKET_API, publicKey.toBase58(), { q, filter, page, limit: 10 });
    setBook(p);
  }, [publicKey, q, filter, page]);

  useEffect(() => {
    load().catch((e) => setErr(e instanceof Error ? e.message : "positions"));
  }, [load]);

  useEffect(() => {
    if (publicKey && book && book.claimable > 0) {
      const first = book.items.find((t) => t.prompt === "unclaimed_settle" || t.prompt === "unclaimed_refund");
      pushInbox({
        id: `claimable-${publicKey.toBase58()}`,
        title: `${book.claimable} ticket(s) waiting for claim / refund`,
        market: first?.market,
        market_title: first ? listingHeadline(first) : undefined,
        ts: Date.now(),
      });
    }
  }, [publicKey, book]);

  async function claim(t: PositionTicket) {
    if (!publicKey || !signTransaction) {
      setNote("connect + SIWS first");
      return;
    }
    const local =
      ticketForHash(publicKey.toBase58(), t.market, t.set_hash) ?? lastTicketFor(publicKey.toBase58(), t.market);
    const mask = t.mask || local?.mask;
    const skellamKind = t.skellam_kind ?? local?.skellam_kind;
    const a = t.a ?? local?.a;
    const b = t.b ?? local?.b;
    const skellam = (t.ticket_kind === "skellam" || local?.kind === "skellam") && skellamKind != null;
    setBusy(true);
    try {
      const refund = t.prompt === "unclaimed_refund";
      const op = refund ? "refund" : skellam ? "payout_skellam" : "payout";
      if (op === "payout" && !mask && !t.set_hash) {
        setNote("frozen set S is missing (no fill journal / durable journal, and the grid is too large to brute-force)");
        return;
      }
      if (op === "payout_skellam" && skellamKind == null && !t.set_hash) {
        setNote("this Skellam ticket is not in the fill journal yet");
        return;
      }
      // Compose hydrates mask / skellam from fill journal or set_hash recovery (n≤20 / typed lines).
      const ix = await compose(MARKET_API, {
        op,
        owner: publicKey.toBase58(),
        market: t.market,
        mask: mask || undefined,
        position: t.position,
        set_hash: t.set_hash,
        kind: skellamKind,
        value: a,
        value_b: b,
        n: t.family === 0 ? 121 : undefined,
        family: t.family,
      });
      const sig = await sendSigned(connection, signTransaction, publicKey, [ix]);
      setNote(`${op} ${sig}`);
      await load();
    } catch (e) {
      setNote(e instanceof Error ? e.message : "claim failed");
    } finally {
      setBusy(false);
    }
  }

  const waiting = (book?.items ?? []).filter((t) => t.prompt === "unclaimed_settle" || t.prompt === "unclaimed_refund");

  return (
    <div>
      <p className="font-mono text-[11px] uppercase tracking-[0.2em] text-amber">Wallet</p>
      <h1 className="font-display text-5xl">Portfolio</h1>
      <p className="mt-2 max-w-2xl text-sm leading-relaxed text-paper/70">
        Deposit, withdraw, and claims use the connected wallet. A trading Session can only buy and sell
        in a prediction market — it cannot move unused margin. Your tickets across every market live
        here. After settlement, payout is ρ · face if the set contains x*. {SESSION_BOARD_ONLY}
      </p>

      {book && book.claimable > 0 && (
        <p className="mt-6 border border-amber/50 bg-amber/10 px-4 py-3 font-mono text-xs text-amber">
          {book.claimable} settled ticket{book.claimable === 1 ? "" : "s"} need a claim review — payout is
          ⌊ρ·face⌋ if your set contains x*, else 0.
        </p>
      )}
      {book && book.paid_tickets > 0 && (
        <p className="mt-3 border border-moss/40 bg-moss/10 px-4 py-3 font-mono text-xs text-moss">
          {book.paid_tickets} ticket{book.paid_tickets === 1 ? "" : "s"} already paid {book.paid_usdc} USDC.
          Claimed net {signedUsdc(book.net_claimed)}.
        </p>
      )}

      {waiting.length > 0 && (
        <section className="mt-8">
          <p className="font-mono text-[11px] uppercase tracking-[0.2em] text-amber">Settlement tickets</p>
          <p className="mt-2 max-w-2xl text-[12px] text-paper/50">
            Same ρ for every winner. VOID / RESOLUTION_FAILED refunds cost_paid, not a fake 0–0. Session cannot claim.
          </p>
          <ul className="mt-4 grid gap-3 lg:grid-cols-2">
            {waiting.map((t) => (
              <li key={t.position} className="border border-amber/40 bg-amber/5 p-4 font-mono text-xs">
                <p className="uppercase tracking-widest text-amber">
                  {t.prompt === "unclaimed_refund" ? "Refund due" : "Claim due"}
                </p>
                <p className="mt-2 text-amber">
                  {listingHeadline(t)} · {familyName(t.family)} · {statusName(t.status)}
                </p>
                <p className="mt-1 break-all text-[11px] text-paper/70">{t.market}</p>
                <div className="mt-3 space-y-1 border-t border-rule/60 pt-2">
                  <Row k="You paid" v={`${t.cost_paid} USDC`} />
                  <Row k="Shares" v={String(t.shares)} />
                  <Row k="If S contains x*" v={t.prompt === "unclaimed_refund" ? "refund cost_paid" : "⌊ρ·face⌋"} />
                  <Row k="If miss" v={t.prompt === "unclaimed_refund" ? "refund cost_paid" : "0"} />
                </div>
                <p className="mt-2 text-[11px] text-amber">{ticketPrompt(t.prompt)}</p>
                <div className="mt-3 flex gap-3">
                  <button
                    type="button"
                    className="flex-1 bg-amber py-2 text-ink disabled:opacity-50"
                    disabled={busy}
                    onClick={() => claim(t)}
                  >
                    {t.prompt === "unclaimed_refund" ? "Refund" : "Claim"}
                  </button>
                  <Link href={`/m/${t.market}`} className="flex-1 border border-rule py-2 text-center uppercase">
                    Market
                  </Link>
                </div>
              </li>
            ))}
          </ul>
        </section>
      )}

      <section className="mt-8 grid gap-8 lg:grid-cols-[1.15fr_0.85fr]">
        <div>
          <p className="font-mono text-[11px] uppercase tracking-[0.2em] text-amber">Your tickets</p>
          {!publicKey && (
            <p className="mt-3 font-mono text-xs text-paper/50">Connect a wallet to see tickets you have filled.</p>
          )}
          {publicKey && (
            <>
              <form
                className="mt-3 flex gap-2"
                onSubmit={(e) => {
                  e.preventDefault();
                  const fd = new FormData(e.currentTarget);
                  setPage(1);
                  setQ(String(fd.get("q") ?? "").trim());
                }}
              >
                <label className="flex-1 font-mono text-[10px] uppercase tracking-widest text-paper/50">
                  Search tickets
                  <input
                    name="q"
                    defaultValue={q}
                    placeholder="market, gaussian, paid…"
                    className="mt-1 w-full border border-rule bg-ink px-2 py-1.5 font-mono text-sm"
                  />
                </label>
                <label className="font-mono text-[10px] uppercase tracking-widest text-paper/50">
                  Filter
                  <select
                    className="mt-1 border border-rule bg-ink px-2 py-1.5 font-mono text-sm"
                    value={filter}
                    onChange={(e) => {
                      setPage(1);
                      setFilter(e.target.value);
                    }}
                  >
                    <option value="">All</option>
                    <option value="open">Open</option>
                    <option value="won">Won</option>
                    <option value="lost">Lost</option>
                    <option value="unclaimed">Unclaimed</option>
                  </select>
                </label>
                <button type="submit" className="mt-5 bg-amber px-4 py-1.5 font-mono text-xs uppercase text-ink">
                  Search
                </button>
              </form>
              {err && <p className="mt-3 font-mono text-xs text-rust">{err}</p>}
              <p className="mt-3 font-mono text-[11px] text-paper/45">
                {book ? `${book.total} tickets · page ${book.page} of ${book.pages}` : "loading…"}
              </p>
              <ul className="mt-3 divide-y divide-rule border border-rule">
                {(book?.items ?? []).map((t) => (
                  <li key={t.position} className="grid gap-2 px-4 py-3 sm:grid-cols-[1fr_auto]">
                    <div>
                      <p className="font-mono text-xs text-amber">
                        {familyName(t.family)} · {statusName(t.status)}
                      </p>
                      <p className="mt-0.5 font-display text-base">{listingHeadline(t)}</p>
                      <p className="mt-0.5 break-all font-mono text-[11px] text-paper/40">{t.market}</p>
                      <p className="mt-1 font-mono text-[11px] text-paper/55">
                        {t.shares} shares · paid in {t.cost_paid} USDC
                        {t.claimed ? ` · payout ${t.paid_usdc} USDC · net ${signedUsdc(t.net_usdc ?? 0)}` : ""}
                      </p>
                      <p className={`mt-1 font-mono text-[11px] ${t.prompt.startsWith("unclaimed") ? "text-amber" : "text-paper/45"}`}>
                        {ticketPrompt(t.prompt)}
                      </p>
                    </div>
                    <div className="flex flex-col items-end gap-2">
                      <Link href={`/m/${t.market}`} className="font-mono text-[11px] uppercase text-amber">
                        Market
                      </Link>
                      {(t.prompt === "unclaimed_settle" || t.prompt === "unclaimed_refund") && (
                        <button
                          type="button"
                          className="border border-amber px-2 py-1 font-mono text-[10px] uppercase text-amber disabled:opacity-50"
                          disabled={busy}
                          onClick={() => claim(t)}
                        >
                          {t.prompt === "unclaimed_refund" ? "Refund" : "Claim"}
                        </button>
                      )}
                    </div>
                  </li>
                ))}
                {publicKey && book && !book.items.length && (
                  <li className="px-4 py-8 font-mono text-sm text-paper/50">No tickets for this wallet.</li>
                )}
              </ul>
              {book && book.pages > 1 && (
                <div className="mt-3 flex justify-between font-mono text-[11px] uppercase">
                  <button type="button" disabled={page <= 1} className="disabled:text-paper/25" onClick={() => setPage((p) => Math.max(1, p - 1))}>
                    Previous
                  </button>
                  <button type="button" disabled={page >= book.pages} className="disabled:text-paper/25" onClick={() => setPage((p) => p + 1)}>
                    Next
                  </button>
                </div>
              )}
            </>
          )}
        </div>

        <CashDesk />
      </section>
    </div>
  );
}

function Row({ k, v }: { k: string; v: string }) {
  return (
    <div className="flex justify-between gap-3 py-1">
      <span className="text-paper/50">{k}</span>
      <span className="text-right">{v}</span>
    </div>
  );
}
