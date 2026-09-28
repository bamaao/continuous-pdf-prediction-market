"use client";

import { fetchInfo, fetchListing, listingHeadline } from "@cpm/sdk";
import Link from "next/link";
import { useEffect, useState } from "react";
import { MARKET_API } from "@/lib/env";
import { SESSION_BOARD_ONLY } from "@/lib/copy";
import { MarketCard } from "./market-card";
import { ResultForm } from "./result-form";

export function ResolveDesk({ market }: { market: string }) {
  const [family, setFamily] = useState<number | undefined>();
  const [title, setTitle] = useState("");
  const [tags, setTags] = useState<string[]>([]);
  const [description, setDescription] = useState("");
  const [event, setEvent] = useState("");
  const [status, setStatus] = useState<number | undefined>();
  const [closeTs, setCloseTs] = useState<number | undefined>();
  const [lMax, setLMax] = useState<number | undefined>();
  const [cMax, setCMax] = useState<number | undefined>();
  const [payable, setPayable] = useState<number | undefined>();

  useEffect(() => {
    fetchInfo(MARKET_API, market)
      .then((info) => {
        setFamily(info.family);
        setTitle(info.title ?? "");
        setTags(info.tags?.length ? info.tags : info.category ? [info.category] : []);
        setDescription(info.description ?? "");
        setEvent(info.event ?? "");
        setStatus(info.status);
        setCloseTs(info.close_ts);
        setLMax(info.l_max_usdc);
        setCMax(info.c_max_usdc);
        setPayable(info.payable_usdc);
      })
      .catch(() => undefined);
    fetchListing(MARKET_API, market)
      .then((row) => {
        if (!row) return;
        if (row.title) setTitle(row.title);
        if (row.tags?.length) setTags(row.tags);
        else if (row.category) setTags([row.category]);
        if (row.description) setDescription(row.description);
        if (row.event) setEvent(row.event);
      })
      .catch(() => undefined);
  }, [market]);

  return (
    <div>
      <p className="font-mono text-[11px] uppercase tracking-[0.2em] text-amber">Resolution</p>
      <h1 className="mt-1 font-display text-4xl">{listingHeadline({ title, family, market })}</h1>
      {event ? <p className="mt-2 max-w-3xl text-lg leading-snug text-paper/80">{event}</p> : null}
      {description ? (
        <p className="mt-2 max-w-3xl whitespace-pre-wrap text-sm leading-relaxed text-paper/70">{description}</p>
      ) : null}
      <p className="mt-1 break-all font-mono text-[11px] text-paper/40">{market}</p>
      <p className="mt-4 max-w-2xl text-sm leading-relaxed text-paper/70">
        Only committee submit_result writes x*. There is no oracle settler on this path. Open the window, propose,
        challenge, then vote M/N. {SESSION_BOARD_ONLY} A trading Session cannot open, submit, challenge, or vote.
      </p>
      <div className="mt-4 border border-rule p-4">
        <p className="font-mono text-[11px] uppercase tracking-widest text-amber">Market card</p>
        <MarketCard
          row={{
            market,
            title,
            tags,
            category: tags[0],
            description,
            event,
            family,
            status,
            close_ts: closeTs,
            l_max_usdc: lMax,
            c_max_usdc: cMax,
            payable_usdc: payable,
          }}
        />
      </div>
      <div className="mt-4 flex flex-wrap gap-4 font-mono text-[11px] uppercase">
        <Link href={`/committee?market=${market}`} className="text-amber">
          Committee
        </Link>
        <Link href={`/m/${market}`}>Back to market</Link>
      </div>
      <div className="mt-8 max-w-xl">
        <ResultForm market={market} family={family ?? 1} />
      </div>
    </div>
  );
}
