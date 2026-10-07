import {
  coveragePct,
  familyName,
  fetchAuctions,
  formatTags,
  formatUnix,
  listCatalogTags,
  listingHeadline,
  listingImageSrc,
  statusName,
} from "@cpm/sdk";
import Link from "next/link";
import { headers } from "next/headers";
import { MARKET_API } from "@/lib/env";

export const dynamic = "force-dynamic";

type SP = {
  q?: string;
  family?: string;
  status?: string;
  category?: string;
  tag?: string;
  page?: string;
  limit?: string;
  view?: string;
};

function num(v: string | undefined): number | undefined {
  if (v == null || v === "") return undefined;
  const n = Number(v);
  return Number.isFinite(n) ? n : undefined;
}

function catalogStatusParam(raw?: string): number | "all" {
  if (raw == null || raw === "") return 1;
  if (raw === "all" || raw === "-1") return "all";
  const n = Number(raw);
  return Number.isFinite(n) ? n : 1;
}

function href(next: {
  q?: string;
  family?: string;
  status?: string;
  tag?: string;
  category?: string;
  view?: string;
  page?: number;
  limit?: number;
}): string {
  const p = new URLSearchParams();
  if (next.q) p.set("q", next.q);
  if (next.family) p.set("family", next.family);
  if (next.status) p.set("status", next.status);
  if (next.tag) p.set("tag", next.tag);
  if (next.category) p.set("category", next.category);
  if (next.view && next.view !== "grid") p.set("view", next.view);
  if (next.limit && next.limit !== 20) p.set("limit", String(next.limit));
  if (next.page && next.page > 1) p.set("page", String(next.page));
  const qs = p.toString();
  return qs ? `/auctions?${qs}` : "/auctions";
}

export default async function AuctionsPage({ searchParams }: { searchParams: Promise<SP> }) {
  const sp = await searchParams;
  const q = (sp.q ?? "").trim();
  const family = num(sp.family);
  const statusQuery = catalogStatusParam(sp.status);
  const tag = (sp.tag ?? sp.category ?? "").trim();
  const view = sp.view === "list" ? "list" : "grid";
  const page = Math.max(1, num(sp.page) ?? 1);
  const limit = Math.min(100, Math.max(1, num(sp.limit) ?? 20));
  const acceptLanguage = (await headers()).get("accept-language") ?? undefined;
  let err = "";
  let catalog = {
    page,
    limit,
    total: 0,
    pages: 1,
    items: [] as Awaited<ReturnType<typeof fetchAuctions>>["items"],
  };
  let vocab: Awaited<ReturnType<typeof listCatalogTags>> = [];
  try {
    const [cat, tags] = await Promise.all([
      fetchAuctions(MARKET_API, {
        q,
        family,
        status: statusQuery,
        tag: tag || undefined,
        page,
        limit,
        acceptLanguage,
      }),
      listCatalogTags(MARKET_API).catch(() => []),
    ]);
    catalog = cat;
    vocab = tags;
  } catch (e) {
    err = e instanceof Error ? e.message : "auctions unreachable";
  }
  const fam = family != null ? String(family) : "";
  const st = sp.status == null || sp.status === "" ? "1" : sp.status;
  const hrefBase = {
    q,
    family: fam,
    status: st === "1" && (sp.status == null || sp.status === "") ? "" : st,
    tag,
    view,
    limit,
  };
  const filtered = Boolean(q || family != null || tag || (sp.status != null && sp.status !== ""));

  return (
    <div>
      <h1 className="font-display text-5xl">Auctions</h1>
      <p className="mt-2 max-w-2xl text-sm leading-relaxed text-paper/70">
        Risk auctions for live markets. Lock D into one pool (min size applies). If winners need a draw, more
        locked capital is better. If they do not, α_R of surplus is paid down the rank until the pot is gone.
        Claim on /lp after settlement.
      </p>
      {err && <p className="mt-4 font-mono text-xs text-rust">{err}</p>}

      <form
        action="/auctions"
        method="get"
        className="mt-8 grid gap-3 border border-rule p-4 sm:grid-cols-[1fr_8rem_8rem_8rem_auto]"
      >
        <label className="block font-mono text-[10px] uppercase tracking-widest text-paper/50">
          Search
          <input
            name="q"
            defaultValue={q}
            placeholder="title, epl, or pubkey"
            className="mt-1 w-full border border-rule bg-ink px-2 py-1.5 font-mono text-sm text-paper"
          />
        </label>
        <label className="block font-mono text-[10px] uppercase tracking-widest text-paper/50">
          Tag
          <input
            name="tag"
            defaultValue={tag}
            placeholder="football"
            className="mt-1 w-full border border-rule bg-ink px-2 py-1.5 font-mono text-sm text-paper"
          />
        </label>
        <label className="block font-mono text-[10px] uppercase tracking-widest text-paper/50">
          Family
          <select name="family" defaultValue={fam} className="mt-1 w-full border border-rule bg-ink px-2 py-1.5 font-mono text-sm">
            <option value="">All</option>
            <option value="0">Skellam</option>
            <option value="1">Gaussian</option>
            <option value="2">Lognormal</option>
            <option value="3">Dirichlet</option>
            <option value="4">Bernoulli</option>
          </select>
        </label>
        <label className="block font-mono text-[10px] uppercase tracking-widest text-paper/50">
          Status
          <select name="status" defaultValue={st} className="mt-1 w-full border border-rule bg-ink px-2 py-1.5 font-mono text-sm">
            <option value="1">Trading</option>
            <option value="all">All</option>
            <option value="2">Halted</option>
            <option value="3">Settled</option>
            <option value="4">Void</option>
          </select>
        </label>
        <div className="flex items-end">
          <button type="submit" className="w-full bg-amber px-4 py-1.5 font-mono text-xs uppercase tracking-widest text-ink">
            Search
          </button>
        </div>
        {limit !== 20 && <input type="hidden" name="limit" value={String(limit)} />}
        {view === "list" ? <input type="hidden" name="view" value="list" /> : null}
      </form>

      {vocab.length ? (
        <p className="mt-3 flex flex-wrap items-center gap-1 font-mono text-[10px] uppercase tracking-widest text-paper/45">
          <span>Catalog</span>
          {vocab.map((t) => (
            <Link
              key={t.name}
              href={href({ ...hrefBase, tag: t.name })}
              className={`border px-1.5 py-0.5 normal-case ${tag === t.name ? "border-amber text-amber" : "border-rule hover:border-amber hover:text-amber"}`}
            >
              {t.name}
              {t.used ? ` · ${t.used}` : ""}
            </Link>
          ))}
          <Link href="/tags" className="ml-1 text-amber">
            Maintain
          </Link>
        </p>
      ) : null}

      <p className="mt-4 flex flex-wrap items-center justify-between gap-2 font-mono text-[11px] text-paper/50">
        <span>
          {catalog.total} auctions · page {catalog.page} of {catalog.pages}
        </span>
        <span className="uppercase tracking-widest">
          <Link href={href({ ...hrefBase, view: "grid" })} className={view === "grid" ? "text-amber" : "hover:text-amber"}>
            Grid
          </Link>
          <span className="mx-2 text-paper/25">/</span>
          <Link href={href({ ...hrefBase, view: "list" })} className={view === "list" ? "text-amber" : "hover:text-amber"}>
            List
          </Link>
        </span>
      </p>

      {view === "grid" ? (
        <ul className="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {catalog.items.map((m) => {
            const cover = listingImageSrc(MARKET_API, m.image_url);
            return (
              <li key={m.market} className="flex flex-col border border-rule">
                <div className="relative aspect-[16/9] bg-paper/5">
                  {cover ? <img src={cover} alt="" className="h-full w-full object-cover" /> : null}
                </div>
                <div className="flex flex-1 flex-col gap-2 p-3">
                  <p className="font-mono text-[10px] uppercase tracking-widest text-amber">
                    {formatTags(m.tags, m.category)}
                    <span className="ml-2 text-paper/45">{statusName(m.status ?? 1)}</span>
                  </p>
                  <h2 className="font-display text-xl leading-tight">{listingHeadline(m)}</h2>
                  {m.event ? <p className="text-sm text-paper/70">{m.event}</p> : null}
                  <p className="font-mono text-[11px] text-paper/50">
                    C_R {m.c_r ?? 0} · coverage {coveragePct(m.l_max_usdc, m.coverage_bps)} · close{" "}
                    {formatUnix(m.close_ts)}
                  </p>
                  <div className="mt-auto flex gap-3 font-mono text-[11px] uppercase tracking-widest">
                    <Link href={`/auction/${m.market}`} className="text-amber">
                      Quote
                    </Link>
                    <Link href={`/m/${m.market}`}>Market</Link>
                  </div>
                </div>
              </li>
            );
          })}
        </ul>
      ) : (
        <ul className="mt-4 divide-y divide-rule border border-rule">
          {catalog.items.map((m) => (
            <li key={m.market} className="grid gap-3 px-4 py-3 sm:grid-cols-[1fr_auto]">
              <div>
                <p className="font-mono text-xs text-amber">
                  {formatTags(m.tags, m.category)} · {familyName(m.family)} · {statusName(m.status ?? 1)}
                </p>
                <h2 className="font-display text-xl">{listingHeadline(m)}</h2>
                <p className="mt-0.5 break-all font-mono text-[11px] text-paper/40">{m.market}</p>
                <p className="mt-1 font-mono text-[11px] text-paper/50">
                  C_R {m.c_r ?? 0} · L_max {m.l_max_usdc ?? 0} · quotes {m.layers ?? 0}
                  {` · coverage ${coveragePct(m.l_max_usdc, m.coverage_bps)}`} · close {formatUnix(m.close_ts)}
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
        </ul>
      )}

      {!catalog.items.length && !err ? (
        <p className="mt-4 border border-rule px-4 py-8 font-mono text-sm text-paper/50">
          {filtered ? "No auctions match this query." : "No auction books indexed."}
        </p>
      ) : null}

      <nav className="mt-4 flex items-center justify-between font-mono text-[11px] uppercase tracking-widest">
        {catalog.page > 1 ? (
          <Link href={href({ ...hrefBase, page: catalog.page - 1 })}>Previous</Link>
        ) : (
          <span className="text-paper/30">Previous</span>
        )}
        <span className="text-paper/45">
          page {catalog.page} / {catalog.pages}
        </span>
        {catalog.page < catalog.pages ? (
          <Link href={href({ ...hrefBase, page: catalog.page + 1 })}>Next</Link>
        ) : (
          <span className="text-paper/30">Next</span>
        )}
      </nav>
    </div>
  );
}
