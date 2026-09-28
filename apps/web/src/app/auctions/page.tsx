import { coveragePct, familyName, fetchAuctions, formatTags, listingHeadline, statusName } from "@cpm/sdk";
import Link from "next/link";
import { MARKET_API } from "@/lib/env";

export const dynamic = "force-dynamic";

type SP = { q?: string; page?: string; limit?: string };

function num(v: string | undefined): number | undefined {
  if (v == null || v === "") return undefined;
  const n = Number(v);
  return Number.isFinite(n) ? n : undefined;
}

function href(next: { q?: string; page?: number; limit?: number }): string {
  const p = new URLSearchParams();
  if (next.q) p.set("q", next.q);
  if (next.limit && next.limit !== 20) p.set("limit", String(next.limit));
  if (next.page && next.page > 1) p.set("page", String(next.page));
  const qs = p.toString();
  return qs ? `/auctions?${qs}` : "/auctions";
}

export default async function AuctionsPage({ searchParams }: { searchParams: Promise<SP> }) {
  const sp = await searchParams;
  const q = (sp.q ?? "").trim();
  const page = Math.max(1, num(sp.page) ?? 1);
  const limit = Math.min(100, Math.max(1, num(sp.limit) ?? 20));
  let err = "";
  let catalog = { page, limit, total: 0, pages: 1, items: [] as Awaited<ReturnType<typeof fetchAuctions>>["items"] };
  try {
    catalog = await fetchAuctions(MARKET_API, { q, page, limit });
  } catch (e) {
    err = e instanceof Error ? e.message : "auctions unreachable";
  }
  return (
    <div>
      <h1 className="font-display text-5xl">Auctions</h1>
      <p className="mt-2 max-w-2xl text-sm leading-relaxed text-paper/70">
        Risk auctions for live markets. Quote published layers to underwrite payout shortfall — an LP cannot
        invent an attachment. Lock D, earn premium if that layer is drawn, claim on /lp after settlement.
      </p>
      {err && <p className="mt-4 font-mono text-xs text-rust">{err}</p>}

      <form action="/auctions" method="get" className="mt-8 flex flex-wrap items-end gap-3 border border-rule p-4">
        <label className="block min-w-[16rem] flex-1 font-mono text-[10px] uppercase tracking-widest text-paper/50">
          Search
          <input
            name="q"
            defaultValue={q}
            placeholder="title, epl, or pubkey"
            className="mt-1 w-full border border-rule bg-ink px-2 py-1.5 font-mono text-sm text-paper"
          />
        </label>
        <button type="submit" className="bg-amber px-4 py-1.5 font-mono text-xs uppercase tracking-widest text-ink">
          Search
        </button>
        {limit !== 20 && <input type="hidden" name="limit" value={String(limit)} />}
      </form>

      <p className="mt-4 font-mono text-[11px] text-paper/45">
        {catalog.total} auctions · page {catalog.page} of {catalog.pages}
      </p>
      <ul className="mt-3 divide-y divide-rule border border-rule">
        {catalog.items.map((m) => (
          <li key={m.market} className="grid gap-3 px-4 py-3 sm:grid-cols-[1fr_auto]">
            <div>
              <p className="font-mono text-xs text-amber">
                {formatTags(m.tags, m.category)} · {familyName(m.family)} · {statusName(m.status ?? 1)}
              </p>
              <h2 className="font-display text-xl">{listingHeadline(m)}</h2>
              <p className="mt-0.5 break-all font-mono text-[11px] text-paper/40">{m.market}</p>
              <p className="mt-1 font-mono text-[11px] text-paper/50">
                C_R {m.c_r ?? 0} · L_max {m.l_max_usdc ?? 0} · layers {m.layers ?? 1}
                {` · coverage ${coveragePct(m.l_max_usdc, m.coverage_bps)}`}
              </p>
            </div>
            <div className="flex items-center gap-3 font-mono text-[11px] uppercase">
              <Link href={`/m/${m.market}`}>Market</Link>
              <Link href={`/auction/${m.market}`} className="text-amber">
                Quote
              </Link>
            </div>
          </li>
        ))}
        {!catalog.items.length && (
          <li className="px-4 py-8 font-mono text-sm text-paper/50">No auction books indexed.</li>
        )}
      </ul>
      <nav className="mt-4 flex items-center justify-between font-mono text-[11px] uppercase tracking-widest">
        {catalog.page > 1 ? (
          <Link href={href({ q, page: catalog.page - 1, limit })}>Previous</Link>
        ) : (
          <span className="text-paper/30">Previous</span>
        )}
        <span className="text-paper/45">
          page {catalog.page} / {catalog.pages}
        </span>
        {catalog.page < catalog.pages ? (
          <Link href={href({ q, page: catalog.page + 1, limit })}>Next</Link>
        ) : (
          <span className="text-paper/30">Next</span>
        )}
      </nav>
    </div>
  );
}
