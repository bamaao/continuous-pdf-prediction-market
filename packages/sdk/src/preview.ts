/** Live coverage is C_max / L_max. Undefined while L_max is 0 — not 100%. */
export function coveragePct(lMax?: number | null, bps?: number | null): string {
  if (lMax == null || lMax <= 0 || bps == null) return "—";
  return `${(bps / 100).toFixed(2)}%`;
}

export function coverageLow(lMax?: number | null, bps?: number | null): boolean {
  return lMax != null && lMax > 0 && bps != null && bps < 8000;
}

/** Written settlement ρ after halt. Live ρ̂ stays coverage / rho_hat_bps. */
export function formatRho(boardPhase?: number | null, rhoBps?: number | null): string {
  if (boardPhase == null || boardPhase < 1) return "—";
  if (rhoBps == null) return "—";
  return (rhoBps / 10_000).toFixed(4);
}

export type BookSnap = {
  beta_raw: string;
  p0_raw: string[];
  theta_raw: string[];
  exposure_raw: string[];
  trading_revenue: number;
  premium_payable: number;
  c_m: number;
  c_r: number;
  slot: number;
};

export type Preview = {
  p_s_raw: string;
  p_s_bps: number;
  c_s_raw: string;
  c_s_usdc: number;
  coverage_bps: number;
  rho_hat_bps: number;
  l_max_usdc: number;
  c_max_usdc: number;
  r_net: number;
  slot: number;
  n: number;
  pdf_bps?: number[];
  fee_bps?: number;
  fee_usdc?: number;
  pay_usdc?: number;
  face_usdc?: number;
  payout_if_hit_usdc?: number;
  payout_if_miss_usdc?: number;
  net_if_hit?: number;
  ev_if_p_s?: number;
};

export type MarketListItem = {
  market: string;
  family: number;
  status?: number;
  n: number;
  slot: number;
  traders?: number;
  stake_usdc?: number;
  l_max_usdc?: number;
  c_r?: number;
  title?: string;
  tags?: string[];
  category?: string;
  topic?: string;
  tag?: string;
  description?: string;
  event?: string;
  close_ts?: number;
  risk_lock_ts?: number;
  report_open_ts?: number;
  extensions?: number;
  abnormal?: string;
  final_result?: string;
  liability?: number;
  c_max_usdc?: number;
  payable_usdc?: number;
  coverage_bps?: number;
  peak_risk?: PeakRisk;
  resolution_phase?: number;
};

export type PeakRisk = {
  cell: number;
  payout_usdc: number;
  label: string;
  lo?: number;
  hi?: number;
  run_lo: number;
  run_hi: number;
  ties: number;
};

export type MarketListQuery = {
  q?: string;
  family?: number;
  status?: number;
  category?: string;
  tag?: string;
  page?: number;
  limit?: number;
};

export type MarketListPage = {
  page: number;
  limit: number;
  total: number;
  pages: number;
  q: string;
  family: number | null;
  status: number | null;
  category?: string | null;
  tag?: string | null;
  items: MarketListItem[];
};

export type MarketInfo = {
  market: string;
  title?: string;
  tags?: string[];
  category?: string;
  topic?: string;
  tag?: string;
  description?: string;
  event?: string;
  close_ts?: number;
  risk_lock_ts?: number;
  report_open_ts?: number;
  extensions?: number;
  abnormal?: string;
  final_result?: string;
  payable_usdc?: number;
  family: number;
  status: number;
  n: number;
  slot: number;
  traders: number;
  tickets: number;
  stake_usdc: number;
  trading_revenue: number;
  l_max_usdc: number;
  c_r: number;
  c_m: number;
  r_net: number;
  c_max_usdc: number;
  coverage_bps: number;
  rho_hat_bps: number;
  fee_bps?: number;
  fee_timing?: number;
  board_phase?: number;
  rho_raw?: string;
  rho_bps?: number;
  settle_cell?: number;
  liability?: number;
  c_p_board?: number;
  c_p_alloc?: number;
  c_p_pool?: number;
  peak_risk?: PeakRisk;
  cells: { cell: number; p_bps: number; e: number }[];
};

export async function listMarketsPage(api: string, query: MarketListQuery = {}): Promise<MarketListPage> {
  const usp = new URLSearchParams();
  if (query.q) usp.set("q", query.q);
  if (query.family != null) usp.set("family", String(query.family));
  if (query.status != null) usp.set("status", String(query.status));
  if (query.category) usp.set("category", query.category);
  if (query.tag) usp.set("tag", query.tag);
  if (query.page) usp.set("page", String(query.page));
  if (query.limit) usp.set("limit", String(query.limit));
  const qs = usp.toString();
  const r = await fetch(`${api}/v1/markets${qs ? `?${qs}` : ""}`);
  if (!r.ok) throw new Error(`markets ${r.status}`);
  const v = await r.json();
  if (Array.isArray(v)) {
    return { page: 1, limit: v.length || 20, total: v.length, pages: 1, q: query.q ?? "", family: query.family ?? null, status: query.status ?? null, category: query.category ?? null, tag: query.tag ?? null, items: v };
  }
  return v;
}

export async function listMarkets(api: string, query: MarketListQuery = {}): Promise<MarketListItem[]> {
  return (await listMarketsPage(api, query)).items;
}

export function statusName(status: number): string {
  return ["", "Trading", "Halted", "Settled", "Void"][status] ?? `status ${status}`;
}

export async function fetchBook(
  api: string,
  market: string,
): Promise<BookSnap & { family: number; n: number; status: number; c_r: number }> {
  const r = await fetch(`${api}/v1/markets/${market}/book`);
  if (!r.ok) throw new Error(`book ${r.status}`);
  return r.json();
}

/** Live quote from Market API / math-wasm preview. Not a TypeScript LMSR. */
export async function fetchQuote(api: string, market: string, mask: string, shares: number): Promise<Preview> {
  const preview = await fetch(`${api}/v1/markets/${market}/preview?mask=${mask}&shares=${shares}`);
  if (preview.ok) return normalizePreview(await preview.json());
  const r = await fetch(`${api}/v1/markets/${market}/quote?mask=${mask}&shares=${shares}`);
  if (!r.ok) throw new Error(`quote ${r.status}`);
  return normalizePreview(await r.json());
}

function normalizePreview(v: Preview): Preview {
  const fee = v.fee_usdc ?? 0;
  const pay = v.pay_usdc ?? v.c_s_usdc + fee;
  const face = v.face_usdc ?? 1;
  const hit = v.payout_if_hit_usdc ?? face;
  return {
    ...v,
    fee_bps: v.fee_bps ?? 0,
    fee_usdc: fee,
    pay_usdc: pay,
    face_usdc: face,
    payout_if_hit_usdc: hit,
    payout_if_miss_usdc: v.payout_if_miss_usdc ?? 0,
    net_if_hit: v.net_if_hit ?? hit - pay,
    ev_if_p_s: v.ev_if_p_s ?? 0,
  };
}

export type PdfCell = { cell: number; p_bps: number; e: number };

export async function fetchPdf(api: string, market: string): Promise<{ market?: string; slot?: number; cells: PdfCell[] }> {
  const r = await fetch(`${api}/v1/markets/${market}/pdf`);
  if (!r.ok) throw new Error(`pdf ${r.status}`);
  return r.json();
}

export type PdfPush = {
  market: string;
  slot: number;
  p_bps: number[];
  e?: number[];
  coverage_bps?: number;
  rho_hat_bps?: number;
  l_max_usdc?: number;
  c_max_usdc?: number;
  peak_risk?: PeakRisk;
};

export function marketWsUrl(api: string, market: string): string {
  const u = new URL(api);
  u.protocol = u.protocol === "https:" ? "wss:" : "ws:";
  u.pathname = `/v1/markets/${market}/ws`;
  u.search = "";
  return u.toString();
}

export function cellsFromPush(push: PdfPush, prev: PdfCell[] = []): PdfCell[] {
  const n = push.p_bps?.length ?? prev.length;
  return Array.from({ length: n }, (_, i) => ({
    cell: i,
    p_bps: push.p_bps[i] ?? prev[i]?.p_bps ?? 0,
    e: push.e?.[i] ?? prev[i]?.e ?? 0,
  }));
}

/** Public desk: traders, stake, L_max, C_R, implied PDF p_k (not E). */
export type PositionTicket = {
  position: string;
  market: string;
  family: number;
  status: number;
  board_phase: number;
  set_hash: string;
  shares: number;
  cost_paid: number;
  claimed: boolean;
  paid_usdc: number;
  net_usdc: number | null;
  rho_hat_bps: number;
  settle_cell: number;
  prompt: string;
  bucket?: string;
  title?: string;
  category?: string;
  event?: string;
  mask?: string;
  ticket_kind?: string;
  skellam_kind?: number;
  a?: number;
  b?: number;
};

export type OwnerPositionsPage = {
  owner: string;
  page: number;
  limit: number;
  total: number;
  pages: number;
  claimable: number;
  paid_tickets: number;
  paid_usdc: number;
  net_claimed: number;
  items: PositionTicket[];
};

export async function fetchOwnerPositions(
  api: string,
  owner: string,
  query: { q?: string; page?: number; limit?: number; filter?: string } = {},
): Promise<OwnerPositionsPage> {
  const usp = new URLSearchParams();
  if (query.q) usp.set("q", query.q);
  if (query.filter) usp.set("filter", query.filter);
  if (query.page) usp.set("page", String(query.page));
  if (query.limit) usp.set("limit", String(query.limit));
  const qs = usp.toString();
  const r = await fetch(`${api}/v1/owners/${owner}/positions${qs ? `?${qs}` : ""}`);
  if (!r.ok) throw new Error(`positions ${r.status}`);
  return r.json();
}

export function ticketPrompt(prompt: string): string {
  switch (prompt) {
    case "unclaimed_settle":
      return "Market settled. Claim if your set contains x*. Payout is ⌊ρ·face⌋.";
    case "unclaimed_refund":
      return "VOID / failed resolution. Reclaim cost_paid.";
    case "paid":
      return "Payout received.";
    case "claimed_zero":
      return "Claimed 0 — miss or dust after ρ.";
    case "refunded":
      return "Refund received.";
    default:
      return "Open ticket.";
  }
}

export async function fetchInfo(api: string, market: string): Promise<MarketInfo> {
  const r = await fetch(`${api}/v1/markets/${market}/info`);
  if (!r.ok) throw new Error(`info ${r.status}`);
  return r.json();
}

export async function fetchAuctions(
  api: string,
  query: MarketListQuery = {},
): Promise<{ page: number; limit: number; total: number; pages: number; items: (MarketListItem & { coverage_bps?: number; layers?: number })[] }> {
  const usp = new URLSearchParams();
  if (query.q) usp.set("q", query.q);
  if (query.category) usp.set("category", query.category);
  if (query.tag) usp.set("tag", query.tag);
  if (query.page) usp.set("page", String(query.page));
  if (query.limit) usp.set("limit", String(query.limit));
  const qs = usp.toString();
  const r = await fetch(`${api}/v1/auctions${qs ? `?${qs}` : ""}`);
  if (!r.ok) throw new Error(`auctions ${r.status}`);
  return r.json();
}

export type AuctionLayer = {
  id: number;
  attachment: number;
  remaining: number;
  unit_premium: number;
  gamma_bps: number;
  quotes: number;
  filled?: number;
  thickness?: number;
};

export type StandingQuote = {
  quote: string;
  lp: string;
  layer: number;
  capacity: number;
  filled: number;
  premium: number;
  unit_premium: number;
  profit_share_bps: number;
};

export async function fetchLayers(api: string, market: string): Promise<{
  market: string;
  title?: string;
  tags?: string[];
  category?: string;
  topic?: string;
  tag?: string;
  description?: string;
  event?: string;
  close_ts?: number;
  family?: number;
  status?: number;
  c_r: number;
  l_max_usdc?: number;
  coverage_bps?: number;
  layers: AuctionLayer[];
  quotes?: StandingQuote[];
}> {
  const r = await fetch(`${api}/v1/markets/${market}/layers`);
  if (!r.ok) throw new Error(`layers ${r.status}`);
  return r.json();
}

export type RiskQuoteItem = {
  quote: string;
  market: string;
  title?: string;
  category?: string;
  layer: number;
  capacity: number;
  filled: number;
  d_i: number;
  premium: number;
  premium_owed: number;
  profit_share_bps: number;
  cancelled: boolean;
  expected_h: number;
  attachment: number;
  weight_sum: number;
  board_phase?: number;
};

export async function fetchOwnerRisk(
  api: string,
  owner: string,
): Promise<{ owner: string; total: number; items: RiskQuoteItem[] }> {
  const r = await fetch(`${api}/v1/owners/${owner}/risk`);
  if (!r.ok) throw new Error(`risk ${r.status}`);
  return r.json();
}

export async function fetchOpsStatus(api: string): Promise<{
  slot: number;
  boards: number;
  boards_with_coverage: number;
  c_r_total: number;
  c_p_pool: number;
  vault_mint: string;
  keeper_heartbeat_slot: number;
  index_lag_slots: number;
  read_only: boolean;
  withdraw_disabled: boolean;
}> {
  const r = await fetch(`${api}/v1/ops/status`);
  if (!r.ok) throw new Error(`ops ${r.status}`);
  return r.json();
}

export async function fetchPool(api: string): Promise<{
  c_p_pool: number;
  boards: { market: string; title?: string; category?: string; c_m: number; c_r: number; c_p_board?: number; c_p_alloc?: number }[];
}> {
  const r = await fetch(`${api}/v1/pool`);
  if (!r.ok) throw new Error(`pool ${r.status}`);
  return r.json();
}

export function familyName(family: number): string {
  return ["Skellam", "Gaussian", "Lognormal", "Dirichlet", "Bernoulli"][family] ?? `family ${family}`;
}

export type OutcomeSnap = {
  kind: number;
  a: string;
  b: string;
  label: string;
};

export type ResolutionSnap = {
  market: string;
  record: string;
  phase: number;
  phase_name: string;
  family: number;
  m: number;
  n: number;
  extensions: number;
  votes_proposal: number;
  votes_challenge: number;
  refunds_due: boolean;
  early_resolve: boolean;
  close_ts: number;
  report_deadline: number;
  challenge_end: number;
  vote_end: number;
  report_window_secs: number;
  challenge_secs: number;
  proposer: string;
  challenger: string;
  authorized_reporter: string;
  members: string[];
  proposed: OutcomeSnap;
  challenged: OutcomeSnap;
  final_outcome: OutcomeSnap;
  evidence_hash: string;
  has_proposed: boolean;
  has_challenged: boolean;
  has_final: boolean;
};

export function resolutionPhaseName(phase: number | null | undefined): string {
  if (phase == null) return "Not opened";
  return ["Open", "Proposed", "Voting", "Finalized", "Failed", "Voided"][phase] ?? `phase ${phase}`;
}

export type CommitteeSnap = {
  authority: string;
  members: string[];
  m: number;
  n: number;
  epoch: number;
  shared: boolean;
};

/** Protocol-wide live roster. In-flight votes keep the snapshot taken at resolve_open. */
export async function fetchCommittee(api: string): Promise<CommitteeSnap | null> {
  const r = await fetch(`${api}/v1/committee`);
  if (r.status === 404) return null;
  if (!r.ok) throw new Error(`committee ${r.status}`);
  return r.json();
}

export async function fetchResolution(api: string, market: string): Promise<ResolutionSnap | null> {
  const r = await fetch(`${api}/v1/markets/${market}/resolution`);
  if (r.status === 404) return null;
  if (!r.ok) throw new Error(`resolution ${r.status}`);
  return r.json();
}

export type PriorQuery = {
  family: number;
  n?: number;
  milli?: boolean;
  x_min?: number;
  x_max?: number;
  mu?: number;
  sigma?: number;
  lambda_home?: number;
  lambda_away?: number;
  layout?: number;
  k?: number;
  bins?: number;
  top_n?: number;
};

export type PriorSnap = {
  family: number;
  n: number;
  peak: number;
  peak_x: number;
  omega: { x_min: number; x_max: number };
  mu: number;
  sigma: number;
  units: string;
  cells: { i: number; x: number; p_bps: number }[];
  intervals: { label: string; p_bps: number }[];
  lines: { label: string; p_bps: number }[];
  warnings: string[];
};

/** Live P0 preview. milli fields are thousandths (2.4 → 2400). */
export async function fetchPrior(api: string, q: PriorQuery): Promise<PriorSnap> {
  const usp = new URLSearchParams();
  usp.set("family", String(q.family));
  if (q.n != null) usp.set("n", String(q.n));
  if (q.milli) usp.set("milli", "true");
  if (q.x_min != null) usp.set("x_min", String(q.x_min));
  if (q.x_max != null) usp.set("x_max", String(q.x_max));
  if (q.mu != null) usp.set("mu", String(q.mu));
  if (q.sigma != null) usp.set("sigma", String(q.sigma));
  if (q.lambda_home != null) usp.set("lambda_home", String(q.lambda_home));
  if (q.lambda_away != null) usp.set("lambda_away", String(q.lambda_away));
  if (q.layout != null) usp.set("layout", String(q.layout));
  if (q.k != null) usp.set("k", String(q.k));
  if (q.bins != null) usp.set("bins", String(q.bins));
  if (q.top_n != null) usp.set("top_n", String(q.top_n));
  const r = await fetch(`${api}/v1/prior?${usp}`);
  if (!r.ok) throw new Error(`prior ${r.status}`);
  return r.json();
}

export function hexMaskFromBits(bits: boolean[]): string {
  const need = Math.ceil(bits.length / 8);
  const bytes = new Uint8Array(need);
  bits.forEach((on, i) => {
    if (on) bytes[i >> 3] |= 1 << (i & 7);
  });
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

export function parseHexMask(hex: string): Uint8Array {
  const h = hex.replace(/^0x/, "");
  const out = new Uint8Array(h.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(h.slice(i * 2, i * 2 + 2), 16);
  return out;
}
