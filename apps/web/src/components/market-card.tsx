import { abnormalLabel, familyName, formatTags, formatUnix, listingHeadline, listingImageSrc, statusName, type PeakRisk } from "@cpm/sdk";
import { MARKET_API } from "@/lib/env";

export type MarketCardData = {
  market: string;
  title?: string;
  tags?: string[];
  category?: string;
  description?: string;
  event?: string;
  family?: number;
  status?: number;
  close_ts?: number;
  risk_lock_ts?: number;
  report_open_ts?: number;
  extensions?: number;
  abnormal?: string;
  final_result?: string;
  liability?: number;
  l_max_usdc?: number;
  c_max_usdc?: number;
  payable_usdc?: number;
  peak_risk?: PeakRisk;
  resolution_phase?: number;
  image_url?: string;
};

export function MarketCard({ row, compact = false }: { row: MarketCardData; compact?: boolean }) {
  const close = formatUnix(row.close_ts);
  const report = formatUnix(row.report_open_ts || row.close_ts);
  const lock = formatUnix(row.risk_lock_ts);
  const payout = row.liability && row.liability > 0 ? row.liability : row.l_max_usdc;
  const cover = listingImageSrc(MARKET_API, row.image_url);
  return (
    <div>
      {cover ? <img src={cover} alt="" className="mb-3 max-h-48 w-full object-cover" /> : null}
    <dl className={`grid gap-3 font-mono text-[11px] ${compact ? "grid-cols-2 sm:grid-cols-3" : "grid-cols-2 sm:grid-cols-3 lg:grid-cols-4"}`}>
      <Item k="Name" v={listingHeadline({ title: row.title, family: row.family, market: row.market })} />
      <Item k="Tags" v={formatTags(row.tags, row.category)} />
      <Item k="Status" v={statusName(row.status ?? 1)} />
      <Item k="Family" v={row.family != null ? familyName(row.family) : "—"} />
      <Item k="Trading event" v={row.event?.trim() || "—"} />
      <Item k="Trading close" v={close} />
      <Item k="Committee may report" v={report} hint="required report_open_ts ≥ close_ts" />
      <Item
        k="Close extension"
        v={row.extensions && row.extensions > 0 ? `report/challenge +${row.extensions}` : "close_ts locked"}
        hint="trading deadline does not move"
      />
      <Item k="Abnormal end" v={abnormalLabel(row.abnormal)} warn={!!row.abnormal} />
      <Item k="Committee result" v={row.final_result?.trim() || "—"} />
      <Item k="Liability L" v={payout != null ? `${payout} USDC` : "—"} hint={row.liability ? "E(c) after settle" : "L_max while open"} />
      <Item
        k="Highest risk payout"
        v={row.peak_risk ? `${row.peak_risk.payout_usdc} USDC` : row.l_max_usdc != null ? `${row.l_max_usdc} USDC` : "—"}
        hint={row.peak_risk?.label ? `if ${row.peak_risk.label}` : "thickest overlap of bought intervals"}
      />
      <Item k="Capital pool C_max" v={row.c_max_usdc != null ? `${row.c_max_usdc} USDC` : "—"} />
      <Item k="Payable" v={row.payable_usdc != null ? `${row.payable_usdc} USDC` : "—"} hint="min(L, C_max)" />
      {!compact && lock !== "—" && <Item k="Risk lock" v={lock} />}
      <div className="col-span-full">
        <dt className="text-[10px] uppercase tracking-widest text-paper/45">Description</dt>
        <dd className="mt-1 whitespace-pre-wrap text-paper/80">{row.description?.trim() || "—"}</dd>
      </div>
    </dl>
    </div>
  );
}

function Item({ k, v, hint, warn }: { k: string; v: string; hint?: string; warn?: boolean }) {
  return (
    <div className={warn ? "text-amber" : ""}>
      <dt className="text-[10px] uppercase tracking-widest text-paper/45">{k}</dt>
      <dd className="mt-1 break-words">{v}</dd>
      {hint && <p className="mt-0.5 text-[10px] text-paper/35">{hint}</p>}
    </div>
  );
}
