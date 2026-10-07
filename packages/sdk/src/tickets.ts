/** Last fill mask per market — claim/refund needs the frozen S (FR-SET). Not a secret. */

import { formatTags, listingHeadline, localListing } from "./listing";
import { hashHex, setHash, skellamTicketHash } from "./pda";
import { fetchInfo, statusName } from "./preview";

const key = (owner: string) => `cpm.tickets.${owner}`;

export type TicketJournalRow = {
  market: string;
  mask?: string;
  shares: number;
  ts: number;
  kind?: "mask" | "skellam";
  skellam_kind?: number;
  a?: number;
  b?: number;
  set_hash?: string;
};

function hexBytes(hex: string): Uint8Array {
  const h = hex.replace(/^0x/, "");
  const out = new Uint8Array(h.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(h.slice(i * 2, i * 2 + 2), 16);
  return out;
}

export function journalSetHash(row: Pick<TicketJournalRow, "kind" | "mask" | "skellam_kind" | "a" | "b" | "set_hash">): string | null {
  if (row.set_hash) return row.set_hash.replace(/^0x/, "").toLowerCase();
  if (row.kind === "skellam" && row.skellam_kind != null) {
    return hashHex(skellamTicketHash(row.skellam_kind, row.a ?? 0, row.b ?? 0));
  }
  if (row.mask) return hashHex(setHash(hexBytes(row.mask)));
  return null;
}

export function rememberTicket(owner: string, market: string, mask: string, shares: number): void {
  rememberFill(owner, { market, mask, shares, kind: "mask", set_hash: hashHex(setHash(hexBytes(mask))) });
}

export function rememberSkellam(
  owner: string,
  market: string,
  skellam_kind: number,
  a: number,
  b: number,
  shares: number,
  mask?: string,
): void {
  rememberFill(owner, {
    market,
    mask,
    shares,
    kind: "skellam",
    skellam_kind,
    a,
    b,
    set_hash: hashHex(skellamTicketHash(skellam_kind, a, b)),
  });
}

function rememberFill(owner: string, row: Omit<TicketJournalRow, "ts">): void {
  if (typeof localStorage === "undefined") return;
  const rows = loadTickets(owner);
  rows.push({ ...row, ts: Date.now() });
  localStorage.setItem(key(owner), JSON.stringify(rows.slice(-80)));
}

/** Persist the frozen S on the Market API so claim does not need this browser. */
export async function saveTicket(
  api: string,
  owner: string,
  row: Pick<TicketJournalRow, "market" | "mask" | "kind" | "skellam_kind" | "a" | "b" | "set_hash">,
): Promise<void> {
  try {
    await fetch(`${api}/v1/tickets`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        owner,
        market: row.market,
        set_hash: row.set_hash ?? "",
        kind: row.kind ?? "mask",
        mask: row.mask ?? "",
        skellam_kind: row.skellam_kind,
        a: row.a,
        b: row.b,
      }),
    });
  } catch {
    /* API down — local journal still holds S */
  }
}

export function loadTickets(owner: string): TicketJournalRow[] {
  if (typeof localStorage === "undefined") return [];
  try {
    const raw = localStorage.getItem(key(owner)) ?? (typeof sessionStorage !== "undefined" ? sessionStorage.getItem(key(owner)) : null);
    return raw ? (JSON.parse(raw) as TicketJournalRow[]) : [];
  } catch {
    return [];
  }
}

export function lastMaskFor(owner: string, market: string): string | null {
  const row = lastTicketFor(owner, market);
  return row?.mask ?? null;
}

export function lastTicketFor(owner: string, market: string): TicketJournalRow | null {
  const rows = loadTickets(owner).filter((r) => r.market === market);
  return rows.length ? rows[rows.length - 1] : null;
}

export function ticketForHash(owner: string, market: string, setHashHex: string): TicketJournalRow | null {
  const want = setHashHex.replace(/^0x/, "").toLowerCase();
  const rows = loadTickets(owner).filter((r) => r.market === market);
  for (let i = rows.length - 1; i >= 0; i--) {
    const h = journalSetHash(rows[i]);
    if (h && h === want) return rows[i];
  }
  return null;
}

export type SessionMeta = { expires_ts: number; remaining_usdc: number };

export function saveSessionMeta(owner: string, meta: SessionMeta): void {
  if (typeof sessionStorage === "undefined") return;
  sessionStorage.setItem(`cpm.session.meta.${owner}`, JSON.stringify(meta));
}

export function loadSessionMeta(owner: string): SessionMeta | null {
  if (typeof sessionStorage === "undefined") return null;
  try {
    const raw = sessionStorage.getItem(`cpm.session.meta.${owner}`);
    return raw ? (JSON.parse(raw) as SessionMeta) : null;
  } catch {
    return null;
  }
}

export function clearSessionMeta(owner: string): void {
  if (typeof sessionStorage === "undefined") return;
  sessionStorage.removeItem(`cpm.session.meta.${owner}`);
}

export type InboxItem = {
  id: string;
  title: string;
  /** Raw keeper / notify kind when this row came from `/v1/notify`. */
  kind?: string;
  market?: string;
  market_title?: string;
  /** Catalog tags label, e.g. `football · epl`. */
  market_tags?: string;
  /** Human status, e.g. Trading / Settled. */
  market_status?: string;
  ts: number;
  /**
   * Explicit unread flag. Missing `read` on legacy rows counts as already seen (history).
   * New alerts set `read: false`.
   */
  read?: boolean;
  read_at?: number;
};

const INBOX_CAP = 80;

/** Only explicit `read === false` is unread — legacy rows without the field stay in History. */
export function isInboxUnread(item: Pick<InboxItem, "read">): boolean {
  return item.read === false;
}

export function unreadInboxCount(rows?: InboxItem[]): number {
  return (rows ?? loadInbox()).filter(isInboxUnread).length;
}

export function markInboxRead(id: string): InboxItem[] {
  const rows = loadInbox();
  const i = rows.findIndex((r) => r.id === id);
  if (i < 0) return rows;
  if (rows[i]!.read === true) return rows;
  rows[i] = { ...rows[i]!, read: true, read_at: Date.now() };
  writeInbox(rows);
  return rows;
}

export function markAllInboxRead(): InboxItem[] {
  const now = Date.now();
  // Fold legacy rows (missing `read`) into History so Unread stays clean.
  const rows = loadInbox().map((r) => (r.read === true ? r : { ...r, read: true, read_at: r.read_at || now }));
  writeInbox(rows);
  return rows;
}

/** Short English line for keeper notify kinds (FR-UI-21). Unknown kinds stay readable. */
export function notifyKindLabel(kind: string): string {
  switch (kind.trim().toLowerCase()) {
    case "close":
      return "Trading closed — market halted at close time";
    case "commit":
      return "Book committed from the rollup to L1";
    case "undelegate":
      return "Market book returned to L1 after close";
    case "challenge":
      return "Challenge opened on this market’s result";
    case "settle":
    case "settled":
      return "Market settled — claim payout if you won";
    case "void":
    case "refund":
      return "Market voided — refund of cost paid is available";
    case "session":
    case "session_expiry":
      return "Trading Session is about to expire — renew or revoke";
    case "pending":
    case "confirmed":
      return "Fill moved from pending to confirmed";
    default: {
      const k = kind.trim();
      if (!k) return "Market update";
      return k.includes(" ") ? k : `Market update: ${k}`;
    }
  }
}

/** Prefer a human title; map legacy raw-kind titles stored in sessionStorage. */
export function inboxTitle(item: Pick<InboxItem, "title" | "kind">): string {
  if (item.kind) return notifyKindLabel(item.kind);
  const t = (item.title ?? "").trim();
  if (!t) return "Market update";
  // Old ingest stored the raw kind as title.
  if (/^[a-z][a-z0-9_]*$/i.test(t) && t.length <= 24) return notifyKindLabel(t);
  return t;
}

export function shortMarketLabel(market: string, title?: string | null): string {
  const name = (title ?? "").trim();
  if (name) return name;
  const m = market.trim();
  if (m.length <= 12) return m;
  return `${m.slice(0, 4)}…${m.slice(-4)}`;
}

function writeInbox(rows: InboxItem[]): void {
  if (typeof sessionStorage === "undefined") return;
  sessionStorage.setItem("cpm.inbox", JSON.stringify(rows.slice(0, INBOX_CAP)));
}

/** Insert or merge. Existing rows keep identity / read state; empty market fields are filled in. */
export function pushInbox(item: InboxItem): void {
  if (typeof sessionStorage === "undefined") return;
  const rows = loadInbox();
  const i = rows.findIndex((r) => r.id === item.id);
  if (i >= 0) {
    const cur = rows[i]!;
    rows[i] = {
      ...cur,
      ...item,
      title: item.title || cur.title,
      kind: item.kind || cur.kind,
      market: item.market || cur.market,
      market_title: item.market_title?.trim() || cur.market_title,
      market_tags: item.market_tags?.trim() || cur.market_tags,
      market_status: item.market_status?.trim() || cur.market_status,
      ts: item.ts || cur.ts,
      // Never re-arm an alert the user already dismissed.
      read: cur.read === true ? true : item.read !== undefined ? item.read : cur.read,
      read_at: cur.read_at ?? item.read_at,
    };
  } else {
    rows.unshift({ ...item, read: item.read ?? false });
  }
  writeInbox(rows);
}

export function loadInbox(): InboxItem[] {
  if (typeof sessionStorage === "undefined") return [];
  try {
    const raw = sessionStorage.getItem("cpm.inbox");
    return raw ? (JSON.parse(raw) as InboxItem[]) : [];
  } catch {
    return [];
  }
}

/** Fill market name / tags / status from Market API for rows that still lack them. */
export async function enrichInboxMarkets(api: string): Promise<InboxItem[]> {
  const rows = loadInbox();
  const markets = [
    ...new Set(
      rows
        .filter((r) => r.market && (!r.market_title?.trim() || !r.market_status?.trim()))
        .map((r) => r.market!.trim())
        .filter(Boolean),
    ),
  ].slice(0, 16);
  if (!markets.length) return rows;
  await Promise.all(
    markets.map(async (market) => {
      try {
        const info = await fetchInfo(api, market);
        const market_title = listingHeadline({ title: info.title, family: info.family, market });
        const market_tags = formatTags(info.tags, info.category);
        const market_status = statusName(info.status ?? 1);
        for (const row of loadInbox().filter((r) => r.market === market)) {
          pushInbox({
            ...row,
            market_title,
            market_tags,
            market_status,
          });
        }
      } catch {
        /* market may be gone */
      }
    }),
  );
  return loadInbox();
}

export type NotifyEvent = { ts: number; kind: string; market: string };

export function notifyKeysOnly(ev: unknown): ev is NotifyEvent {
  if (!ev || typeof ev !== "object") return false;
  const o = ev as Record<string, unknown>;
  const keys = Object.keys(o).sort();
  return (
    keys.length === 3 &&
    keys[0] === "kind" &&
    keys[1] === "market" &&
    keys[2] === "ts" &&
    typeof o.kind === "string" &&
    typeof o.market === "string" &&
    typeof o.ts === "number" &&
    o.market.length > 0
  );
}

export async function fetchNotify(api: string): Promise<NotifyEvent[]> {
  const r = await fetch(`${api}/v1/notify`);
  if (!r.ok) throw new Error(`notify ${r.status}`);
  const j = (await r.json()) as { events?: unknown[] };
  return (j.events ?? []).filter(notifyKeysOnly);
}

export function ingestNotifyEvents(events: NotifyEvent[]): InboxItem[] {
  for (const ev of events) {
    const meta = localListing(ev.market);
    const market_title = meta?.title?.trim()
      ? listingHeadline({ title: meta.title, family: undefined, market: ev.market })
      : undefined;
    const market_tags = meta ? formatTags(meta.tags, meta.category) : undefined;
    pushInbox({
      id: `ntf-${ev.ts}-${ev.kind}-${ev.market}`,
      kind: ev.kind,
      title: notifyKindLabel(ev.kind),
      market: ev.market,
      market_title,
      market_tags,
      ts: ev.ts < 1e12 ? ev.ts * 1000 : ev.ts,
    });
  }
  return loadInbox();
}
