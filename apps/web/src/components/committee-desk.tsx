"use client";

import { compose, familyName, fetchCommittee, formatTags, listingHeadline, listMarketsPage, MarketListItem, resolutionPhaseName, statusName, type CommitteeSnap } from "@cpm/sdk";
import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import Link from "next/link";
import { useEffect, useState } from "react";
import { MARKET_API } from "@/lib/env";
import { NEED_WALLET, SESSION_BOARD_ONLY } from "@/lib/copy";
import { sendSigned } from "@/lib/tx";
import { MarketCard } from "./market-card";
import { ResultForm } from "./result-form";

function shortKey(k: string): string {
  if (!k || k.length < 12) return k || "—";
  return `${k.slice(0, 4)}…${k.slice(-4)}`;
}

function parseRoster(raw: string): string[] {
  return raw
    .split(/[\s,]+/)
    .map((s) => s.trim())
    .filter(Boolean);
}

export function CommitteeDesk({ initialMarket = "" }: { initialMarket?: string }) {
  const { connection } = useConnection();
  const { publicKey, signTransaction } = useWallet();
  const [q, setQ] = useState("");
  const [page, setPage] = useState(1);
  const [items, setItems] = useState<MarketListItem[]>([]);
  const [total, setTotal] = useState(0);
  const [pages, setPages] = useState(1);
  const [err, setErr] = useState("");
  const [picked, setPicked] = useState(initialMarket);
  const [roster, setRoster] = useState<CommitteeSnap | null>(null);
  const [membersText, setMembersText] = useState("");
  const [m, setM] = useState(1);
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let stop = false;
    const load = () =>
      fetchCommittee(MARKET_API)
        .then((row) => {
          if (stop) return;
          setRoster(row);
          setMembersText((cur) => {
            if (cur.trim() || !row) return cur;
            setM(row.m);
            return row.members.join("\n");
          });
        })
        .catch(() => {
          if (!stop) setRoster(null);
        });
    load();
    const tick = window.setInterval(load, 60_000);
    return () => {
      stop = true;
      window.clearInterval(tick);
    };
  }, []);

  useEffect(() => {
    let stop = false;
    listMarketsPage(MARKET_API, { q, page, limit: 10, status: "all" })
      .then((cat) => {
        if (stop) return;
        setItems(cat.items);
        setTotal(cat.total);
        setPages(cat.pages);
        setPicked((cur) => cur || cat.items[0]?.market || "");
      })
      .catch((e) => setErr(e instanceof Error ? e.message : "catalog"));
    return () => {
      stop = true;
    };
  }, [q, page]);

  const selected = items.find((m) => m.market === picked);
  const me = publicKey?.toBase58() ?? "";
  const isAuthority = !!roster && !!me && roster.authority === me;
  const canWrite = !roster || isAuthority;

  async function writeRoster(op: "init_committee" | "set_roster") {
    if (!publicKey || !signTransaction) {
      setNote(NEED_WALLET);
      return;
    }
    const members = parseRoster(membersText);
    if (!members.length) {
      setNote("paste at least one member pubkey");
      return;
    }
    if (m < 1 || m > members.length) {
      setNote(`M must be between 1 and ${members.length}`);
      return;
    }
    setBusy(true);
    setNote("signing…");
    try {
      const ix = await compose(MARKET_API, { op, owner: publicKey.toBase58(), members, m });
      const sig = await sendSigned(connection, signTransaction, publicKey, [ix]);
      const next = await fetchCommittee(MARKET_API);
      setRoster(next);
      if (next) {
        setMembersText(next.members.join("\n"));
        setM(next.m);
      }
      setNote(`${op} ${sig}`);
    } catch (e) {
      setNote(e instanceof Error ? e.message : op);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div>
      <p className="font-mono text-[11px] uppercase tracking-[0.2em] text-amber">Committee</p>
      <h1 className="font-display text-5xl">Committee</h1>
      <p className="mt-2 max-w-2xl text-sm leading-relaxed text-paper/70">
        One protocol-wide roster. Creating a market only binds this PDA; it does not mint a new committee. Membership
        may change. An in-flight vote keeps the snapshot taken at <span className="text-paper">resolve_open</span>.{" "}
        {SESSION_BOARD_ONLY}
      </p>

      <section className="mt-8 border border-amber/40 bg-amber/5 p-4 font-mono text-xs">
        <p className="uppercase tracking-widest text-amber">Shared roster</p>
        <p className="mt-2 text-[11px] text-paper/60">
          {roster
            ? `Live ${roster.m}/${roster.n} · epoch ${roster.epoch} · authority ${shortKey(roster.authority)}`
            : "Not initialized. Authority is the first wallet that writes this account."}
        </p>
        {roster && (
          <ul className="mt-3 columns-1 gap-x-6 sm:columns-2">
            {roster.members.map((pk) => (
              <li key={pk} className="break-all text-[11px] text-paper/80">
                {pk}
              </li>
            ))}
          </ul>
        )}
        {canWrite ? (
          <div className="mt-4 space-y-3">
            <label className="block text-[10px] uppercase text-paper/50">
              Members (one pubkey per line)
              <textarea
                className="mt-1 h-28 w-full border border-rule bg-ink px-2 py-1.5 font-mono text-[11px]"
                value={membersText}
                onChange={(e) => setMembersText(e.target.value)}
                placeholder="paste pubkeys"
              />
            </label>
            <label className="block max-w-[8rem] text-[10px] uppercase text-paper/50">
              Majority M
              <input
                className="mt-1 w-full border border-rule bg-ink px-2 py-1"
                type="number"
                min={1}
                max={16}
                value={m}
                onChange={(e) => setM(Number(e.target.value))}
              />
            </label>
            <button
              type="button"
              className="bg-amber px-4 py-1.5 font-mono text-xs uppercase text-ink disabled:opacity-50"
              disabled={busy}
              onClick={() => writeRoster(roster ? "set_roster" : "init_committee")}
            >
              {busy ? "Signing…" : roster ? "Update live roster" : "Initialize shared committee"}
            </button>
            {note && <p className="text-[11px] text-paper/60">{note}</p>}
          </div>
        ) : (
          <p className="mt-3 text-[11px] text-paper/45">
            Only the committee authority can replace members. Connect {shortKey(roster?.authority ?? "")} to edit.
          </p>
        )}
      </section>

      <div className="mt-8 grid gap-8 lg:grid-cols-[1.05fr_0.95fr]">
        <section>
          <form
            className="flex gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              const fd = new FormData(e.currentTarget);
              setPage(1);
              setQ(String(fd.get("q") ?? "").trim());
            }}
          >
            <label className="flex-1 font-mono text-[10px] uppercase tracking-widest text-paper/50">
              Search markets
              <input
                name="q"
                defaultValue={q}
                placeholder="market, family, status…"
                className="mt-1 w-full border border-rule bg-ink px-2 py-1.5 font-mono text-sm"
              />
            </label>
            <button type="submit" className="mt-5 bg-amber px-4 py-1.5 font-mono text-xs uppercase text-ink">
              Search
            </button>
          </form>
          {err && <p className="mt-3 font-mono text-xs text-rust">{err}</p>}
          <p className="mt-3 font-mono text-[11px] text-paper/45">
            {total} markets · page {page} of {pages}
          </p>
          <ul className="mt-3 divide-y divide-rule border border-rule">
            {items.map((m) => (
              <li key={m.market}>
                <button
                  type="button"
                  onClick={() => setPicked(m.market)}
                  className={`block w-full px-3 py-2 text-left font-mono text-xs ${picked === m.market ? "bg-amber/10" : ""}`}
                >
                  <span className="text-amber">
                    {formatTags(m.tags, m.category)} · {familyName(m.family)} · {statusName(m.status ?? 1)} · {resolutionPhaseName(m.resolution_phase)}
                  </span>
                  <span className="mt-0.5 block font-display text-base text-paper">{listingHeadline(m)}</span>
                  <span className="mt-0.5 block break-all text-[11px] text-paper/45">{m.market}</span>
                </button>
              </li>
            ))}
            {!items.length && <li className="px-3 py-6 font-mono text-xs text-paper/45">No markets match this query.</li>}
          </ul>
          <div className="mt-3 flex justify-between font-mono text-[11px] uppercase">
            <button type="button" disabled={page <= 1} className="disabled:text-paper/25" onClick={() => setPage((p) => Math.max(1, p - 1))}>
              Previous
            </button>
            <button type="button" disabled={page >= pages} className="disabled:text-paper/25" onClick={() => setPage((p) => p + 1)}>
              Next
            </button>
          </div>
        </section>

        <section>
          <p className="font-mono text-[11px] uppercase tracking-[0.2em] text-amber">Submit event result</p>
          {picked ? (
            <>
              <p className="mt-2 font-display text-xl">{selected ? listingHeadline(selected) : "Submit event result"}</p>
              <p className="mt-1 break-all font-mono text-[11px] text-paper/40">{picked}</p>
              <p className="mt-1 font-mono text-[11px] text-paper/45">
                {selected
                  ? `${formatTags(selected.tags, selected.category)} · ${familyName(selected.family)} · ${statusName(selected.status ?? 1)} · ${resolutionPhaseName(selected.resolution_phase)}`
                  : "selected market"}
              </p>
              {selected && (
                <div className="mt-3 border border-rule p-3">
                  <MarketCard compact row={selected} />
                </div>
              )}
              <div className="mt-4">
                <ResultForm market={picked} family={selected?.family ?? 1} />
              </div>
              <div className="mt-4 flex flex-wrap gap-4 font-mono text-[11px] uppercase">
                <Link href={`/resolve/${picked}`} className="text-paper/50">
                  Per-market resolve
                </Link>
                <Link href={`/m/${picked}`} className="text-amber">
                  Open market
                </Link>
              </div>
            </>
          ) : (
            <p className="mt-4 font-mono text-xs text-paper/50">Pick a prediction market to report x*.</p>
          )}
        </section>
      </div>
    </div>
  );
}
