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

export type LocaleCopy = {
  title?: string;
  event?: string;
  description?: string;
};

export type ListingI18n = Record<string, LocaleCopy>;

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
  source_locale?: string;
  i18n?: ListingI18n;
  image_id?: string;
  image_url?: string;
  title_en?: string;
  event_en?: string;
  description_en?: string;
  locale?: string;
  is_translation?: boolean;
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

/** Prefer title_en / event_en / description_en so a translated desk view cannot seed identity. */
export function canonicalListing(meta: ListingMeta): ListingMeta {
  return {
    ...meta,
    title: (meta.title_en ?? meta.title).trim(),
    event: (meta.event_en ?? meta.event ?? "").trim(),
    description: (meta.description_en ?? meta.description ?? "").trim(),
    title_en: meta.title_en ?? meta.title,
    event_en: meta.event_en ?? meta.event,
    description_en: meta.description_en ?? meta.description,
  };
}

export async function saveListing(api: string, meta: ListingMeta): Promise<void> {
  const canon = canonicalListing(meta);
  rememberListing({ ...canon, i18n: meta.i18n ?? {} });
  const r = await fetch(`${api}/v1/listings`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      market: canon.market,
      title: canon.title,
      tags: canon.tags?.length ? canon.tags : parseTagsInput(canon.category ?? ""),
      category: canon.category ?? canon.tags?.[0] ?? "",
      topic: canon.topic ?? "",
      tag: canon.tag ?? "",
      description: canon.description ?? "",
      event: canon.event ?? "",
      blocked_regions: canon.blocked_regions ?? [],
      source_locale: canon.source_locale ?? "en",
      i18n: meta.i18n ?? {},
      image_id: canon.image_id ?? meta.image_id ?? "",
    }),
  });
  if (r.status === 409) throw new Error("listing 409 — canonical English locked after OPEN");
  if (!r.ok) throw new Error(`listing ${r.status}`);
}

/** Replay the browser cache into the API after a restart. Never overwrite a title the API already has. */
export async function hydrateListings(api: string): Promise<number> {
  const all = readLocal();
  let n = 0;
  await Promise.all(
    Object.values(all).map(async (meta) => {
      const canon = canonicalListing(meta);
      if (!canon?.market || !canon.title?.trim()) return;
      if (!(canon.tags && canon.tags.length) && !canon.category?.trim()) return;
      try {
        // Force English so a translated Accept-Language view never seeds the wrong identity.
        const cur = await fetch(`${api}/v1/listings/${canon.market}?locale=en`);
        if (cur.ok) {
          const v = (await cur.json()) as ListingMeta & { title_en?: string };
          if ((v?.title_en ?? v?.title)?.trim()) return;
        }
        const r = await fetch(`${api}/v1/listings`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({
            market: canon.market,
            title: canon.title,
            tags: canon.tags?.length ? canon.tags : parseTagsInput(canon.category ?? ""),
            category: canon.category ?? canon.tags?.[0] ?? "",
            topic: canon.topic ?? "",
            tag: canon.tag ?? "",
            description: canon.description ?? "",
            event: canon.event ?? "",
            blocked_regions: canon.blocked_regions ?? [],
            source_locale: canon.source_locale ?? "en",
            i18n: meta.i18n ?? {},
            image_id: canon.image_id ?? meta.image_id ?? "",
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

export async function fetchListing(api: string, market: string, acceptLanguage?: string): Promise<ListingMeta | null> {
  try {
    const headers: Record<string, string> = {};
    if (acceptLanguage) headers["accept-language"] = acceptLanguage;
    const r = await fetch(`${api}/v1/listings/${market}`, { headers });
    if (r.ok) {
      const v = (await r.json()) as ListingMeta & {
        title_en?: string;
        event_en?: string;
        description_en?: string;
        locale?: string;
        is_translation?: boolean;
        source_locale?: string;
        i18n?: ListingI18n;
        blocked_regions?: string[];
      };
      if (v?.title || v?.title_en) {
        const titleEn = (v.title_en ?? v.title ?? "").trim();
        const eventEn = (v.event_en ?? v.event ?? "").trim();
        const descriptionEn = (v.description_en ?? v.description ?? "").trim();
        // Cache always stores canonical English so hydrate cannot seed a translation as identity.
        rememberListing({
          market,
          title: titleEn,
          title_en: titleEn,
          tags: v.tags?.length ? v.tags : parseTagsInput(v.category ?? ""),
          category: v.category ?? v.tags?.[0] ?? "",
          topic: v.topic,
          tag: v.tag,
          description: descriptionEn,
          description_en: descriptionEn,
          event: eventEn,
          event_en: eventEn,
          source_locale: v.source_locale ?? "en",
          i18n: v.i18n ?? {},
          blocked_regions: v.blocked_regions ?? [],
          image_id: v.image_id ?? "",
          image_url: v.image_url ?? "",
        });
        return {
          market,
          title: v.title ?? titleEn,
          title_en: titleEn,
          tags: v.tags?.length ? v.tags : parseTagsInput(v.category ?? ""),
          category: v.category ?? v.tags?.[0] ?? "",
          topic: v.topic,
          tag: v.tag,
          description: v.description ?? descriptionEn,
          description_en: descriptionEn,
          event: v.event ?? eventEn,
          event_en: eventEn,
          locale: v.locale,
          is_translation: v.is_translation,
          source_locale: v.source_locale ?? "en",
          i18n: v.i18n ?? {},
          blocked_regions: v.blocked_regions ?? [],
          image_id: v.image_id ?? "",
          image_url: v.image_url ?? "",
        };
      }
    }
  } catch {
    /* offline — local cache */
  }
  return localListing(market);
}

/** Absolute URL for a listing cover (`/v1/media/{id}` from Market API). Empty when none. */
export function listingImageSrc(api: string, imageUrl?: string | null): string {
  const u = (imageUrl ?? "").trim();
  if (!u) return "";
  if (/^https?:\/\//i.test(u)) return u;
  const base = api.replace(/\/$/, "");
  return `${base}${u.startsWith("/") ? u : `/${u}`}`;
}

export async function uploadListingMedia(
  api: string,
  owner: string,
  file: Blob,
): Promise<{ id: string; url: string }> {
  const fd = new FormData();
  fd.set("owner", owner);
  fd.set("file", file);
  const r = await fetch(`${api}/v1/listings/media`, { method: "POST", body: fd });
  if (r.status === 401) throw new Error("sign in, then upload");
  if (r.status === 413) throw new Error("cover must be ≤ 2 MiB");
  if (r.status === 415) throw new Error("cover must be jpeg, png, or webp");
  if (!r.ok) throw new Error(`media ${r.status}`);
  return r.json() as Promise<{ id: string; url: string }>;
}
