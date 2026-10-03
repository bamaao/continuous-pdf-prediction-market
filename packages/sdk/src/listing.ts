export const LISTING_CATEGORIES = [
  { id: "football", label: "football" },
  { id: "macro", label: "macro" },
  { id: "price", label: "price" },
  { id: "election", label: "election" },
  { id: "binary", label: "binary" },
  { id: "other", label: "other" },
] as const;

export type ListingCategoryId = (typeof LISTING_CATEGORIES)[number]["id"];

export const TAG_HINTS = ["football", "epl", "world cup", "macro", "cpi", "price", "election", "binary"] as const;

export type CatalogTag = { name: string; used: number };

export async function listCatalogTags(api: string): Promise<CatalogTag[]> {
  const r = await fetch(`${api}/v1/tags`);
  if (!r.ok) throw new Error(`tags ${r.status}`);
  const v = (await r.json()) as { items?: CatalogTag[] };
  return Array.isArray(v.items) ? v.items : [];
}

export async function createCatalogTag(api: string, name: string): Promise<CatalogTag> {
  const r = await fetch(`${api}/v1/tags`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ name }),
  });
  if (!r.ok) throw new Error(`tag ${r.status}`);
  return r.json();
}

export async function deleteCatalogTag(api: string, name: string): Promise<void> {
  const r = await fetch(`${api}/v1/tags/${encodeURIComponent(name)}`, { method: "DELETE" });
  if (r.status === 409) throw new Error("tag in use");
  if (r.status === 404) throw new Error("unknown tag");
  if (!r.ok) throw new Error(`tag ${r.status}`);
}

export type ListingMeta = {
  market: string;
  title: string;
  tags?: string[];
  category?: string;
  topic?: string;
  tag?: string;
  description?: string;
  event?: string;
  blocked_regions?: string[];
};

const LS = "cpm.listings.v3";

export function parseTagsInput(raw: string): string[] {
  return raw
    .split(/[,，/|]+/)
    .map((t) => t.trim())
    .filter(Boolean)
    .slice(0, 8);
}

export function formatTags(tags?: string[] | null, fallback?: string): string {
  const xs = (tags && tags.length ? tags : fallback ? [fallback] : []).map((t) => t.trim()).filter(Boolean);
  return xs.join(" · ") || "untagged";
}

export function categoryLabel(id: string): string {
  return formatTags(undefined, id);
}

export function defaultTags(family: number): string[] {
  return [["football"], ["macro"], ["price"], ["election"], ["binary"]][family] ?? ["other"];
}

export function defaultCategory(family: number): string {
  return defaultTags(family)[0] ?? "other";
}

/** Create-form preset only. Never use this as a listed board's fallback name. */
export function defaultTitle(family: number): string {
  return (
    ["Football match", "US CPI YoY — 2026-03", "BTC-USD daily close", "Election winner", "Binary event"][family] ??
    "Prediction market"
  );
}

/** Untitled indexed board — generic family, not a CPI / BTC example. */
export function familyFallbackTitle(family?: number | null): string {
  return (
    [
      "Football prediction market",
      "Gaussian prediction market",
      "Lognormal prediction market",
      "Dirichlet prediction market",
      "Bernoulli prediction market",
    ][family ?? -1] ?? "Prediction market"
  );
}

export function defaultDescription(family: number): string {
  return (
    [
      "State the match, competition, and settlement. Example: Arsenal vs Chelsea, Premier League. Settles on full-time score (including stoppage time), not extra time or penalties. Overflow cell is 10+. Trading continues after kickoff.",
      "State which official print, the unit, and whether revisions settle. Example: 2026-03 US CPI YoY first official print. Later revisions do not settle.",
      "State the observation time, price source, and price_rule. Example: Coinbase BTC-USD close at observation time. The rule is locked when the prediction market opens and does not change.",
      "State the certified result and layout. Winner is atoms; top-n is combinations; vote share is a simplex grid (k candidates, bins), not a second family. α_i=1.",
      "State the YES definition and deadline. Example: YES if the event has occurred by the deadline, otherwise NO.",
    ][family] ?? "State the event, settlement rule, and data source so traders can judge the market."
  );
}

export function defaultEvent(family: number): string {
  return (
    [
      "Arsenal vs Chelsea — Premier League full-time score",
      "US CPI YoY first official print",
      "BTC-USD close at observation time",
      "Certified election winner",
      "Whether the event has occurred by the deadline",
    ][family] ?? "State what this prediction market is about"
  );
}

export function listingHeadline(row: { title?: string | null; family?: number; market: string }): string {
  const t = (row.title ?? "").trim();
  if (t) return t;
  return familyFallbackTitle(row.family);
}

export function formatUnix(ts?: number | null): string {
  if (ts == null || ts <= 0) return "—";
  return `${new Date(ts * 1000).toISOString().replace("T", " ").slice(0, 16)} UTC`;
}

export function abnormalLabel(code?: string | null): string {
  if (code === "void_refund") return "VOID — refund cost_paid";
  if (code === "resolution_failed_refund") return "RESOLUTION_FAILED — refund cost_paid";
  return "Normal close";
}

function readLocal(): Record<string, ListingMeta> {
  if (typeof window === "undefined") return {};
  try {
    const raw = window.localStorage.getItem(LS) ?? window.localStorage.getItem("cpm.listings.v1");
    if (!raw) return {};
    const v = JSON.parse(raw) as Record<string, ListingMeta>;
    return v && typeof v === "object" ? v : {};
  } catch {
    return {};
  }
}

export function rememberListing(meta: ListingMeta): void {
  if (typeof window === "undefined") return;
  const all = readLocal();
  all[meta.market] = meta;
  window.localStorage.setItem(LS, JSON.stringify(all));
}

export function localListing(market: string): ListingMeta | null {
  return readLocal()[market] ?? null;
}

export async function saveListing(api: string, meta: ListingMeta): Promise<void> {
  rememberListing(meta);
  const r = await fetch(`${api}/v1/listings`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      market: meta.market,
      title: meta.title,
      tags: meta.tags?.length ? meta.tags : parseTagsInput(meta.category ?? ""),
      category: meta.category ?? meta.tags?.[0] ?? "",
      topic: meta.topic ?? "",
      tag: meta.tag ?? "",
      description: meta.description ?? "",
      event: meta.event ?? "",
      blocked_regions: meta.blocked_regions ?? [],
    }),
  });
  if (!r.ok) throw new Error(`listing ${r.status}`);
}

/** Replay the browser cache into the API after a restart. Never overwrite a title the API already has. */
export async function hydrateListings(api: string): Promise<number> {
  const all = readLocal();
  let n = 0;
  await Promise.all(
    Object.values(all).map(async (meta) => {
      if (!meta?.market || !meta.title?.trim()) return;
      if (!(meta.tags && meta.tags.length) && !meta.category?.trim()) return;
      try {
        const cur = await fetch(`${api}/v1/listings/${meta.market}`);
        if (cur.ok) {
          const v = (await cur.json()) as ListingMeta;
          if (v?.title?.trim()) return;
        }
        const r = await fetch(`${api}/v1/listings`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({
            market: meta.market,
            title: meta.title,
            tags: meta.tags?.length ? meta.tags : parseTagsInput(meta.category ?? ""),
            category: meta.category ?? meta.tags?.[0] ?? "",
            topic: meta.topic ?? "",
            tag: meta.tag ?? "",
            description: meta.description ?? "",
            event: meta.event ?? "",
            blocked_regions: meta.blocked_regions ?? [],
          }),
        });
        if (r.ok) n += 1;
      } catch {
        /* API down */
      }
    }),
  );
  return n;
}

export async function fetchListing(api: string, market: string): Promise<ListingMeta | null> {
  try {
    const r = await fetch(`${api}/v1/listings/${market}`);
    if (r.ok) {
      const v = (await r.json()) as ListingMeta;
      if (v?.title) {
        const meta = {
          market,
          title: v.title,
          tags: v.tags?.length ? v.tags : parseTagsInput(v.category ?? ""),
          category: v.category ?? v.tags?.[0] ?? "",
          topic: v.topic,
          tag: v.tag,
          description: v.description,
          event: v.event,
        };
        rememberListing(meta);
        return meta;
      }
    }
  } catch {
    /* offline — local cache */
  }
  return localListing(market);
}
