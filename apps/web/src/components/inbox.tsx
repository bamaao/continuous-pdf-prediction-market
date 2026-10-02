"use client";

import { fetchNotify, ingestNotifyEvents, loadInbox } from "@cpm/sdk";
import { MARKET_API } from "@/lib/env";
import Link from "next/link";
import { useEffect, useState } from "react";

export function Inbox() {
  const [open, setOpen] = useState(false);
  const [items, setItems] = useState(() => (typeof window === "undefined" ? [] : loadInbox()));

  useEffect(() => {
    const tick = async () => {
      try {
        ingestNotifyEvents(await fetchNotify(MARKET_API));
      } catch {
        /* feed optional */
      }
      setItems(loadInbox());
    };
    void tick();
    const id = setInterval(() => void tick(), 4000);
    return () => clearInterval(id);
  }, []);

  return (
    <div className="relative">
      <button className="border border-rule px-2 py-1 font-mono text-[11px] uppercase" onClick={() => setOpen((v) => !v)}>
        Inbox{items.length ? ` (${items.length})` : ""}
      </button>
      {open && (
        <div className="absolute right-0 z-30 mt-2 w-80 border border-rule bg-ink p-3 font-mono text-[11px]">
          <p className="uppercase tracking-widest text-amber">Alerts</p>
          <ul className="mt-3 max-h-64 space-y-2 overflow-auto">
            {items.map((it) => (
              <li key={it.id} className="border-b border-rule/60 pb-2">
                <p>{it.title}</p>
                {it.market && (
                  <Link href={`/m/${it.market}`} className="text-amber">
                    {it.market}
                  </Link>
                )}
              </li>
            ))}
            {!items.length && <li className="text-paper/45">No alerts yet.</li>}
          </ul>
        </div>
      )}
    </div>
  );
}
