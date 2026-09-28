/** Last fill mask per market — claim/refund needs the frozen S (FR-SET). Not a secret. */

import { hashHex, setHash, skellamTicketHash } from "./pda";

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

export type InboxItem = { id: string; title: string; market?: string; market_title?: string; ts: number };

export function pushInbox(item: InboxItem): void {
  if (typeof sessionStorage === "undefined") return;
  const rows = loadInbox();
  if (rows.some((r) => r.id === item.id)) return;
  sessionStorage.setItem("cpm.inbox", JSON.stringify([item, ...rows].slice(0, 40)));
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
