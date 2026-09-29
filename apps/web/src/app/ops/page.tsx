import { fetchOpsStatus, fetchPool, listingHeadline } from "@cpm/sdk";
import { RiskTape } from "@/components/risk-tape";
import { MARKET_API } from "@/lib/env";

export const dynamic = "force-dynamic";

export default async function OpsPage() {
  let err = "";
  let ops = {
    slot: 0,
    boards: 0,
    boards_with_coverage: 0,
    c_r_total: 0,
    c_p_pool: 0,
    vault_mint: "Circle SPL USDC",
    keeper_heartbeat_slot: 0,
    keeper_ok: false,
    keeper_last: "",
    index_lag_slots: 0,
    read_only: true,
    withdraw_disabled: true,
  };
  let pool = { c_p_pool: 0, boards: [] as { market: string; title?: string; category?: string; c_m: number; c_r: number; c_p_board?: number; c_p_alloc?: number }[] };
  try {
    [ops, pool] = await Promise.all([fetchOpsStatus(MARKET_API), fetchPool(MARKET_API)]);
  } catch (e) {
    err = e instanceof Error ? e.message : "ops unreachable";
  }
  return (
    <div>
      <h1 className="font-display text-5xl">Ops</h1>
      <p className="mt-2 max-w-2xl text-sm text-paper/70">
        Read-only. No Vault withdraw. Keeper close / Commit / Undelegate stay on the CLI.
      </p>
      {err && <p className="mt-4 font-mono text-xs text-rust">{err}</p>}
      <dl className="mt-8 grid grid-cols-2 gap-3 border border-rule p-4 font-mono text-xs sm:grid-cols-3">
        <Stat k="Index slot" v={String(ops.slot)} />
        <Stat k="Lag" v={`${ops.index_lag_slots} slots`} />
        <Stat k="Markets" v={String(ops.boards)} />
        <Stat k="With coverage" v={String(ops.boards_with_coverage)} />
        <Stat k="C_R total" v={`${ops.c_r_total} USDC`} />
        <Stat k="C_P pool" v={`${ops.c_p_pool} USDC`} />
        <Stat k="Fees" v="platform ledger, not C_P" />
        <Stat k="Vault mint" v={ops.vault_mint} />
        <Stat
          k="Keeper heartbeat"
          v={`slot ${ops.keeper_heartbeat_slot}${ops.keeper_ok ? " ok" : ""}${ops.keeper_last ? ` ${ops.keeper_last}` : ""}`}
        />
        <Stat k="Withdraw" v="disabled" />
      </dl>
      <RiskTape limit={50} />
      <p className="mt-8 font-mono text-[11px] uppercase tracking-widest text-amber">Per-market C_P caps</p>
      <ul className="mt-3 divide-y divide-rule border border-rule font-mono text-[11px]">
        {pool.boards.map((b) => (
          <li key={b.market} className="px-4 py-2">
            <span className="font-display text-sm">{listingHeadline(b)}</span>
            <span className="ml-2 break-all text-paper/40">{b.market}</span>
            <span className="ml-3 text-paper/50">
              C_R {b.c_r} · C_P cap {b.c_p_board ?? 0} · alloc {b.c_p_alloc ?? 0}
            </span>
          </li>
        ))}
        {!pool.boards.length && <li className="px-4 py-6 text-paper/45">No prediction markets indexed.</li>}
      </ul>
    </div>
  );
}

function Stat({ k, v }: { k: string; v: string }) {
  return (
    <div>
      <dt className="text-[10px] uppercase tracking-widest text-paper/45">{k}</dt>
      <dd className="mt-1">{v}</dd>
    </div>
  );
}
