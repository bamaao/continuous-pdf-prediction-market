"use client";

import { listListingApplications, reviewListingApplication, type ListingApplication } from "@cpm/sdk";
import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import Link from "next/link";
import { useCallback, useEffect, useState } from "react";
import { MARKET_API } from "@/lib/env";
import { APPROVE_AND_OPEN_MARKET, NEED_WALLET, OPEN_MARKET, OPEN_RISK_AUCTION, RETRY_OPEN_MARKET } from "@/lib/copy";
import { openBoardAsOwner } from "@/lib/open-board";

const STATUS: { id?: number; label: string }[] = [
  { id: 0, label: "待审" },
  { id: 1, label: "已批准" },
  { id: 2, label: "已拒绝" },
  { id: 3, label: "重复" },
  { label: "全部" },
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
    setNote(`#${row.id} 正在${OPEN_MARKET}…`);
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
      `#${opened.id} ${opened.status_name} · ${market} · ${created ? OPEN_MARKET : "预测市场已在链上，已写入大厅"} · ${sig}`,
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
      setNote("拒绝或标为重复时必须写原因");
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
            `#${approved.id} 已批准，但预测市场还没出现在大厅。点「${RETRY_OPEN_MARKET}」：${
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
      setNote(e instanceof Error ? e.message : `${OPEN_MARKET}失败`);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div>
      <h1 className="font-display text-5xl">审核</h1>
      <p className="mt-2 max-w-2xl text-sm text-paper/70">
        批准后，用当前钱包{OPEN_MARKET}：大厅能看到，交易者能买。同时{OPEN_RISK_AUCTION}，让资金方报出赔付覆盖。
        如果这个预测市场已经在链上，再点「{RETRY_OPEN_MARKET}」只会把它写进大厅，不会再建一个。
      </p>
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
        原因（拒绝或标为重复时必填）
        <input className="mt-1 w-full border border-rule bg-ink px-2 py-1" value={reason} onChange={(e) => setReason(e.target.value)} />
      </label>
      {note && <p className="mt-3 font-mono text-xs text-amber">{note}</p>}
      {justOpened ? (
        <p className="mt-2">
          <Link href={`/m/${justOpened}`} className="font-mono text-sm text-amber">
            进入预测市场
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
                  进入预测市场 {row.market.slice(0, 4)}…{row.market.slice(-4)}
                </Link>
              </p>
            ) : null}
            {row.status === 0 && (
              <div className="mt-3 flex flex-wrap gap-2">
                <button className="bg-amber px-3 py-1 text-ink disabled:opacity-50" disabled={busy} onClick={() => decide(row, "approve")}>
                  {APPROVE_AND_OPEN_MARKET}
                </button>
                <button className="border border-rule px-3 py-1 disabled:opacity-50" disabled={busy} onClick={() => decide(row, "reject")}>
                  拒绝
                </button>
                <button className="border border-rule px-3 py-1 disabled:opacity-50" disabled={busy} onClick={() => decide(row, "duplicate")}>
                  标为重复
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
        {!rows.length && <li className="px-4 py-8 text-paper/45">这一栏没有申请。</li>}
      </ul>
    </div>
  );
}
