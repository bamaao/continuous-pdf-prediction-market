export type MarketComment = {
  id: number;
  market: string;
  author: string;
  body: string;
  created_at: number;
};

export type CommentPage = {
  market: string;
  page: number;
  limit: number;
  total: number;
  pages: number;
  items: MarketComment[];
};

export async function listComments(api: string, market: string, page = 1, limit = 20): Promise<CommentPage> {
  const q = new URLSearchParams({ page: String(page), limit: String(limit) });
  const r = await fetch(`${api}/v1/markets/${encodeURIComponent(market)}/comments?${q}`);
  if (r.status === 404) throw new Error("market not listed");
  if (!r.ok) throw new Error(`comments ${r.status}`);
  return r.json();
}

export async function postComment(api: string, market: string, author: string, body: string): Promise<MarketComment> {
  const r = await fetch(`${api}/v1/markets/${encodeURIComponent(market)}/comments`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ author, body }),
  });
  if (r.status === 404) throw new Error("market not listed");
  if (r.status === 400) throw new Error("comment rejected");
  if (!r.ok) throw new Error(`comment ${r.status}`);
  return r.json();
}

export function shortPubkey(pk: string): string {
  const t = pk.trim();
  if (t.length <= 10) return t || "—";
  return `${t.slice(0, 4)}…${t.slice(-4)}`;
}
