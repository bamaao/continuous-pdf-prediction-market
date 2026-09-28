import { familyName, formatTags, formatUnix, listCatalogTags, listMarketsPage, listingHeadline, statusName } from "@cpm/sdk";
import Link from "next/link";
import { RiskTape } from "@/components/risk-tape";
import { MARKET_API } from "@/lib/env";

export const dynamic = "force-dynamic";

type SP = { q?: string; family?: string; status?: string; category?: string; tag?: string; page?: string; limit?: string };

function num(v: string | undefined): number | undefined {
  if (v == null || v === "") return undefined;
  const n = Number(v);
  return Number.isFinite(n) ? n : undefined;
}

function catalogHref(next: {
  q?: string;
  family?: string;
  status?: string;
  category?: string;
  tag?: string;
  page?: number;
  limit?: number;
}): string {
  const p = new URLSearchParams();
  if (next.q) p.set("q", next.q);
  if (next.family) p.set("family", next.family);
  if (next.status) p.set("status", next.status);
  if (next.tag) p.set("tag", next.tag);
  if (next.category) p.set("category", next.category);
  if (next.limit && next.limit !== 20) p.set("limit", String(next.limit));
  if (next.page && next.page > 1) p.set("page", String(next.page));
  const qs = p.toString();
  return qs ? `/?${qs}` : "/";
}

export default async function Lobby({ searchParams }: { searchParams: Promise<SP> }) {
  const sp = await searchParams;
  const q = (sp.q ?? "").trim();
  const family = num(sp.family);
  const status = num(sp.status);
  const tag = (sp.tag ?? sp.category ?? "").trim();
  const page = Math.max(1, num(sp.page) ?? 1);
  const limit = Math.min(100, Math.max(1, num(sp.limit) ?? 20));
  let err = "";
  let catalog = {
    page,
    limit,
    total: 0,
    pages: 1,
    q,
    family: family ?? null,
    status: status ?? null,
    items: [] as Awaited<ReturnType<typeof listMarketsPage>>["items"],
  };
  let vocab: Awaited<ReturnType<typeof listCatalogTags>> = [];
  try {
    [catalog, vocab] = await Promise.all([
      listMarketsPage(MARKET_API, { q, family, status, tag: tag || undefined, page, limit }),
      listCatalogTags(MARKET_API).catch(() => []),
    ]);
  } catch (e) {
    err = e instanceof Error ? e.message : "market-api unreachable";
  }
  const fam = family != null ? String(family) : "";
  const st = status != null ? String(status) : "";
  return (
    <div>
      <h1 className="font-display text-5xl">Live markets</h1>
      <p className="mt-2 max-w-2xl text-sm text-paper/70">
        Markets that are open for trading. The row heading is the market name. Search by name, tags, event,
        family, or pubkey. Price is a probability. Coverage is a warning, not a discount.
      </p>
      <RiskTape />

      <form action="/" method="get" className="mt-8 grid gap-3 border border-rule p-4 sm:grid-cols-[1fr_8rem_8rem_8rem_auto]">
        <label className="block font-mono text-[10px] uppercase tracking-widest text-paper/50">
          Search
          <input
            name="q"
            defaultValue={q}
            placeholder="title, epl, CPI…"
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
            <option value="">All</option>
            <option value="1">Trading</option>
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
      </form>
      {vocab.length ? (
        <p className="mt-3 flex flex-wrap items-center gap-1 font-mono text-[10px] uppercase tracking-widest text-paper/45">
          <span>Catalog</span>
          {vocab.map((t) => (
            <Link
              key={t.name}
              href={catalogHref({ q, family: fam, status: st, tag: t.name, limit })}
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

      <p className="mt-4 font-mono text-[11px] text-paper/50">
        {catalog.total} markets · page {catalog.page} of {catalog.pages}
      </p>
      {err && <p className="mt-6 font-mono text-sm text-rust">Read path: {err}</p>}
      <ul className="mt-4 divide-y divide-rule border border-rule">
        {catalog.items.map((m) => (
          <li key={m.market} className="grid grid-cols-[1fr_auto] items-center gap-4 px-4 py-3">
            <div>
              <p className="font-mono text-xs text-amber">
                {formatTags(m.tags, m.category)}
                <span className="ml-2 text-paper/45">{familyName(m.family)}</span>
                <span className="ml-2 text-paper/45">{statusName(m.status ?? 1)}</span>
              </p>
              <h2 className="font-display text-xl">{listingHeadline(m)}</h2>
              {m.event ? <p className="mt-1 text-sm text-paper/80">{m.event}</p> : null}
              {m.description ? <p className="mt-1 line-clamp-3 text-sm leading-relaxed text-paper/60">{m.description}</p> : null}
              <p className="mt-0.5 break-all font-mono text-[11px] text-paper/40">{m.market}</p>
              <p className="mt-1 font-mono text-[11px] text-paper/50">
                close {formatUnix(m.close_ts)} · highest risk {m.peak_risk?.payout_usdc ?? m.l_max_usdc ?? 0} USDC
                {m.peak_risk?.label ? ` if ${m.peak_risk.label}` : ""} · C_max {m.c_max_usdc ?? 0} · payable {m.payable_usdc ?? 0}
                {m.final_result ? ` · x* ${m.final_result}` : ""}
                {m.abnormal ? " · refund" : ""}
              </p>
            </div>
            <div className="flex gap-3 font-mono text-[11px] uppercase tracking-widest">
              <Link href={`/m/${m.market}`} className="text-amber">
                Market
              </Link>
              <Link href={`/auction/${m.market}`}>Auction</Link>
              <Link href={`/resolve/${m.market}`}>Resolve</Link>
              <Link href={`/committee?market=${m.market}`}>Report</Link>
            </div>
          </li>
        ))}
        {!catalog.items.length && !err && (
          <li className="px-4 py-8 font-mono text-sm text-paper/50">
            {q || family != null || status != null || tag
              ? "No markets match this query."
              : "No prediction markets indexed yet."}
          </li>
        )}
      </ul>
      <nav className="mt-4 flex items-center justify-between font-mono text-[11px] uppercase tracking-widest">
        {catalog.page > 1 ? (
          <Link href={catalogHref({ q, family: fam, status: st, tag, page: catalog.page - 1, limit })}>Previous</Link>
        ) : (
          <span className="text-paper/30">Previous</span>
        )}
        <span className="text-paper/45">
          page {catalog.page} / {catalog.pages}
        </span>
        {catalog.page < catalog.pages ? (
          <Link href={catalogHref({ q, family: fam, status: st, tag, page: catalog.page + 1, limit })}>Next</Link>
        ) : (
          <span className="text-paper/30">Next</span>
        )}
      </nav>
    </div>
  );
}
