"use client";

import {
  enrichInboxMarkets,
  fetchNotify,
  inboxTitle,
  ingestNotifyEvents,
  isInboxUnread,
  loadInbox,
  markAllInboxRead,
  markInboxRead,
  shortMarketLabel,
  unreadInboxCount,
  type InboxItem,
} from "@cpm/sdk";
import { MARKET_API } from "@/lib/env";
import Link from "next/link";
import { useEffect, useMemo, useState, useSyncExternalStore } from "react";

const EMPTY: InboxItem[] = [];
const listeners = new Set<() => void>();
let cachedRaw: string | null = null;
let cachedItems: InboxItem[] = EMPTY;

function emitInbox() {
  listeners.forEach((fn) => fn());
}

function subscribeInbox(onStoreChange: () => void) {
  listeners.add(onStoreChange);
  return () => {
    listeners.delete(onStoreChange);
  };
}

function getInboxSnapshot(): InboxItem[] {
  if (typeof sessionStorage === "undefined") return EMPTY;
  const raw = sessionStorage.getItem("cpm.inbox");
  if (raw === cachedRaw) return cachedItems;
  cachedRaw = raw;
  cachedItems = loadInbox();
  return cachedItems;
}

function formatWhen(ts: number): string {
  if (!ts) return "";
  try {
    return new Date(ts).toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return "";
  }
}

type Tab = "unread" | "history";

export function Inbox() {
  const [open, setOpen] = useState(false);
  const [tab, setTab] = useState<Tab>("unread");
  const items = useSyncExternalStore(subscribeInbox, getInboxSnapshot, () => EMPTY);
  const unread = useMemo(() => unreadInboxCount(items), [items]);
  const unreadItems = useMemo(() => items.filter(isInboxUnread), [items]);
  const historyItems = useMemo(() => items.filter((r) => !isInboxUnread(r)), [items]);
  const shown = tab === "unread" ? unreadItems : historyItems;

  useEffect(() => {
    const tick = async () => {
      try {
        ingestNotifyEvents(await fetchNotify(MARKET_API));
        await enrichInboxMarkets(MARKET_API);
      } catch {
        /* feed optional */
      }
      emitInbox();
    };
    void tick();
    const id = setInterval(() => void tick(), 60_000);
    window.addEventListener("storage", emitInbox);
    return () => {
      clearInterval(id);
      window.removeEventListener("storage", emitInbox);
    };
  }, []);

  useEffect(() => {
    if (!open) return;
    void enrichInboxMarkets(MARKET_API).then(() => emitInbox());
  }, [open]);

  function onOpenMarket(id: string) {
    markInboxRead(id);
    emitInbox();
    if (tab === "unread" && unreadInboxCount() === 0) setTab("history");
  }

  function onMarkAll() {
    markAllInboxRead();
    emitInbox();
    setTab("history");
  }

  return (
    <div className="relative">
      <button
        type="button"
        className="border border-rule px-2 py-1 font-mono text-[11px] uppercase"
        onClick={() => setOpen((v) => !v)}
      >
        Inbox{unread ? ` (${unread})` : ""}
      </button>
      {open && (
        <div className="absolute right-0 z-30 mt-2 w-[22rem] border border-rule bg-ink p-3 font-mono text-[11px]">
          <div className="flex items-start justify-between gap-2">
            <div>
              <p className="uppercase tracking-widest text-amber">Alerts</p>
              <p className="mt-1 text-[10px] leading-relaxed text-paper/45">
                Unread first; History keeps what you already opened. Session-local only.
              </p>
            </div>
            {unread ? (
              <button
                type="button"
                className="shrink-0 border border-rule px-1.5 py-0.5 text-[10px] uppercase tracking-widest text-paper/70 hover:border-amber hover:text-amber"
                onClick={onMarkAll}
              >
                Mark all read
              </button>
            ) : null}
          </div>

          <div className="mt-3 flex gap-3 border-b border-rule text-[10px] uppercase tracking-widest">
            <button
              type="button"
              className={`pb-1 ${tab === "unread" ? "border-b border-amber text-amber" : "text-paper/45 hover:text-paper"}`}
              onClick={() => setTab("unread")}
            >
              Unread{unread ? ` · ${unread}` : ""}
            </button>
            <button
              type="button"
              className={`pb-1 ${tab === "history" ? "border-b border-amber text-amber" : "text-paper/45 hover:text-paper"}`}
              onClick={() => setTab("history")}
            >
              History · {historyItems.length}
            </button>
          </div>

          <ul className="mt-3 max-h-80 space-y-3 overflow-auto">
            {shown.map((it) => {
              const event = inboxTitle(it);
              const when = formatWhen(it.ts);
              const name = it.market ? shortMarketLabel(it.market, it.market_title) : "Prediction market";
              const meta = [it.market_tags, it.market_status, when].filter(Boolean).join(" · ");
              const unreadRow = isInboxUnread(it);
              return (
                <li key={it.id} className={`border-b border-rule/60 pb-2 ${unreadRow ? "" : "opacity-75"}`}>
                  <div className="flex items-start gap-2">
                    {unreadRow ? <span className="mt-1.5 h-1.5 w-1.5 shrink-0 rounded-full bg-amber" aria-hidden /> : <span className="mt-1.5 h-1.5 w-1.5 shrink-0" aria-hidden />}
                    <div className="min-w-0 flex-1">
                      {it.market ? (
                        <Link
                          href={`/m/${it.market}`}
                          className="block font-medium leading-snug text-amber hover:underline"
                          onClick={() => onOpenMarket(it.id)}
                        >
                          {name}
                        </Link>
                      ) : (
                        <button
                          type="button"
                          className="block text-left font-medium leading-snug text-paper"
                          onClick={() => onOpenMarket(it.id)}
                        >
                          {name}
                        </button>
                      )}
                      <p className="mt-0.5 leading-snug text-paper/85">{event}</p>
                      {meta ? <p className="mt-1 text-[10px] text-paper/45">{meta}</p> : null}
                      {unreadRow ? (
                        <button
                          type="button"
                          className="mt-1 text-[10px] uppercase tracking-widest text-paper/40 hover:text-amber"
                          onClick={() => onOpenMarket(it.id)}
                        >
                          Mark read
                        </button>
                      ) : null}
                    </div>
                  </div>
                </li>
              );
            })}
            {!shown.length && (
              <li className="text-paper/45">
                {tab === "unread" ? "You're caught up — nothing unread." : "No history in this browser session yet."}
              </li>
            )}
          </ul>
        </div>
      )}
    </div>
  );
}
