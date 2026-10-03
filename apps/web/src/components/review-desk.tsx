"use client";

import { fetchRoles, listListingApplications, reviewListingApplication, type ListingApplication } from "@cpm/sdk";
import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import Link from "next/link";
import { useCallback, useEffect, useState } from "react";
import { MARKET_API } from "@/lib/env";
import { APPROVE_AND_OPEN_MARKET, ENTER_MARKET, NEED_WALLET, OPEN_MARKET, OPEN_RISK_AUCTION, RETRY_OPEN_MARKET } from "@/lib/copy";
import { openBoardAsOwner } from "@/lib/open-board";

const STATUS: { id?: number; label: string }[] = [
  { id: 0, label: "Pending" },
  { id: 1, label: "Approved" },
  { id: 2, label: "Rejected" },
  { id: 3, label: "Duplicate" },
  { label: "All" },
];

export function ReviewDesk() {
  const { connection } = useConnection();
  const { publicKey, signTransaction } = useWallet();
  const [status, setStatus] = useState<number | undefined>(0);
  const [rows, setRows] = useState<ListingApplication[]>([]);
  const [note, setNote] = useState("");
  const [reason, setReason] = useState("");
  const [busy, setBusy] = useState(false);
  const [justOpened, setJustOpened] = useState<string | null>(null);
  const [roleNote, setRoleNote] = useState("");

  useEffect(() => {
    if (!publicKey) {
      setRoleNote("Connect a wallet — Review is only for R-REVIEW.");
      return;
    }
    let stop = false;
    fetchRoles(MARKET_API, publicKey.toBase58())
      .then((r) => {
        if (stop) return;
        if (!r.reviewer && !r.open_review) {
          setRoleNote("This wallet is not on REVIEWER_PUBKEYS.");
        } else if (r.open_review) {
          setRoleNote("Local open review — any SIWS wallet may approve (empty REVIEWER_PUBKEYS).");
        } else {
          setRoleNote("");
        }
      })
      .catch(() => {
        if (!stop) setRoleNote("Could not load roles.");
      });
    return () => {
      stop = true;
    };
  }, [publicKey]);

  const load = useCallback(async () => {
    const page = await listListingApplications(MARKET_API, { status });
    setRows(page.items ?? []);
  }, [status]);

  useEffect(() => {
    load().catch((e) => setNote(e instanceof Error ? e.message : "review queue"));
  }, [load]);

  async function openOnChain(row: ListingApplication) {
    if (!publicKey || !signTransaction) {
      throw new Error(NEED_WALLET);
    }
    const spec = row.compose ?? {};
    if (!spec.op) {
      throw new Error("application has no compose spec — ask the applicant to resubmit");
    }
    setNote(`#${row.id} ${OPEN_MARKET}…`);
    const { market, sig, created } = await openBoardAsOwner({
      api: MARKET_API,
      connection,
      signTransaction,
      owner: publicKey,
      spec,
      listing: {
        title: row.title,
        tags: row.tags,
        topic: String(spec.topic ?? row.topic ?? ""),
        tag: String(spec.tag ?? row.tag ?? ""),
        description: row.description,
        event: row.event,
        blocked_regions: row.blocked_regions,
      },
    });
    const opened = await reviewListingApplication(MARKET_API, {
      id: row.id,
      reviewer: publicKey.toBase58(),
      action: "opened",
      market,
    });
    setNote(
      `#${opened.id} ${opened.status_name} · ${market} · ${created ? OPEN_MARKET : "prediction market already on-chain, written to the lobby"} · ${sig}`,
    );
    setJustOpened(market);
    return opened;
  }

  async function decide(row: ListingApplication, action: "approve" | "reject" | "duplicate") {
    if (!publicKey) {
      setNote(NEED_WALLET);
      return;
    }
    if (action !== "approve" && !reason.trim()) {
      setNote("a reason is required to reject or mark as duplicate");
      return;
    }
    setBusy(true);
    try {
      if (action === "approve") {
        const approved = await reviewListingApplication(MARKET_API, {
          id: row.id,
          reviewer: publicKey.toBase58(),
          action: "approve",
          reason: reason.trim(),
        });
        try {
          await openOnChain(approved);
        } catch (e) {
          setNote(
            `#${approved.id} approved, but the prediction market is not in the lobby yet. Use “${RETRY_OPEN_MARKET}”: ${
              e instanceof Error ? e.message : OPEN_MARKET
            }`,
          );
          await load();
          return;
        }
      } else {
        const next = await reviewListingApplication(MARKET_API, {
          id: row.id,
          reviewer: publicKey.toBase58(),
          action,
          reason: reason.trim(),
        });
        setNote(`#${next.id} ${next.status_name}`);
      }
      setReason("");
      await load();
    } catch (e) {
      setNote(e instanceof Error ? e.message : "review failed");
    } finally {
      setBusy(false);
    }
  }

  async function retryOpen(row: ListingApplication) {
    if (!publicKey) {
      setNote(NEED_WALLET);
      return;
    }
    setBusy(true);
    try {
      await openOnChain(row);
      await load();
    } catch (e) {
      setNote(e instanceof Error ? e.message : `failed to ${OPEN_MARKET}`);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div>
      <h1 className="font-display text-5xl">Review</h1>
      <p className="mt-2 max-w-2xl text-sm text-paper/70">
        After approve, the connected wallet {OPEN_MARKET}: the lobby shows it, traders can buy. The same step {OPEN_RISK_AUCTION}, so capital can quote payout coverage.
        If this prediction market is already on-chain, “{RETRY_OPEN_MARKET}” only writes it into the lobby — it does not create a second one.
      </p>
      {roleNote ? <p className="mt-3 font-mono text-xs text-amber">{roleNote}</p> : null}
      <div className="mt-6 flex flex-wrap gap-2 font-mono text-[11px] uppercase">
        {STATUS.map((s) => (
          <button
            key={s.label}
            className={`border px-2 py-1 ${status === s.id ? "border-amber text-amber" : "border-rule"}`}
            onClick={() => setStatus(s.id)}
          >
            {s.label}
          </button>
        ))}
      </div>
      <label className="mt-4 block font-mono text-[10px] uppercase text-paper/50">
        Reason (required to reject or mark as duplicate)
        <input className="mt-1 w-full border border-rule bg-ink px-2 py-1" value={reason} onChange={(e) => setReason(e.target.value)} />
      </label>
      {note && <p className="mt-3 font-mono text-xs text-amber">{note}</p>}
      {justOpened ? (
        <p className="mt-2">
          <Link href={`/m/${justOpened}`} className="font-mono text-sm text-amber">
            {ENTER_MARKET}
          </Link>
        </p>
      ) : null}
      <ul className="mt-6 divide-y divide-rule border border-rule">
        {rows.map((row) => (
          <li key={row.id} className="p-4 font-mono text-xs">
            <p className="text-[11px] uppercase tracking-widest text-amber">
              #{row.id} · {row.status_name} · family {row.family}
            </p>
            <p className="mt-1 font-display text-lg">{row.title}</p>
            <p className="mt-1 text-paper/70">{row.event}</p>
            <p className="mt-2 whitespace-pre-wrap text-paper/60">{row.description}</p>
            <p className="mt-2 text-paper/40">
              {row.tags.join(" · ")} · blocked {row.blocked_regions.join(" · ") || "none"} · {row.applicant.slice(0, 4)}…
              {row.applicant.slice(-4)}
            </p>
            {row.market ? (
              <p className="mt-2">
                <Link href={`/m/${row.market}`} className="text-amber">
                  {ENTER_MARKET} {row.market.slice(0, 4)}…{row.market.slice(-4)}
                </Link>
              </p>
            ) : null}
            {row.status === 0 && (
              <div className="mt-3 flex flex-wrap gap-2">
                <button className="bg-amber px-3 py-1 text-ink disabled:opacity-50" disabled={busy} onClick={() => decide(row, "approve")}>
                  {APPROVE_AND_OPEN_MARKET}
                </button>
                <button className="border border-rule px-3 py-1 disabled:opacity-50" disabled={busy} onClick={() => decide(row, "reject")}>
                  Reject
                </button>
                <button className="border border-rule px-3 py-1 disabled:opacity-50" disabled={busy} onClick={() => decide(row, "duplicate")}>
                  Mark duplicate
                </button>
              </div>
            )}
            {row.status === 1 && !row.market && (
              <button className="mt-3 bg-amber px-3 py-1 text-ink disabled:opacity-50" disabled={busy} onClick={() => retryOpen(row)}>
                {RETRY_OPEN_MARKET}
              </button>
            )}
            {!!row.logs?.length && (
              <ol className="mt-3 space-y-1 text-[11px] text-paper/45">
                {row.logs.map((l) => (
                  <li key={l.id}>
                    {l.action} · {l.reviewer === "system" ? "system" : `${l.reviewer.slice(0, 4)}…`} · {l.reason}
                  </li>
                ))}
              </ol>
            )}
          </li>
        ))}
        {!rows.length && <li className="px-4 py-8 text-paper/45">No applications in this column.</li>}
      </ul>
    </div>
  );
}
