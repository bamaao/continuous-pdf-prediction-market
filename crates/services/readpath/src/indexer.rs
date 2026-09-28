//! Local websocket / RPC poller. Yellowstone is the production transport; this is the local path.

use crate::store::{
    persist_projections, stored_ledger_genesis, wipe_projection_tables, write_ledger_genesis, LayerRow, MarketProj,
    MemoryStore, OutcomeSnap, PositionRow, QuoteRow, ResolutionRow,
};
use anyhow::Result;
use crate::store::CommitteeSnap;
use client::{
    board, committee_pda, decode_board, decode_claim, decode_committee, decode_grid, decode_layer, decode_market,
    decode_pool, decode_position, decode_quote, decode_resolution, decode_risk_book, decode_tap, grid_pda,
    risk_book,
};
use math::settle::usdc;
use math::Q64;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

/// Ledger snapshot → projection. `C_R` is locked+filled D from the risk book (0 if none).
pub fn project(
    market: String,
    family: u8,
    status: u8,
    n: u16,
    beta: i128,
    p0: Vec<i128>,
    theta: Vec<i128>,
    exposure: Vec<i128>,
    market_trading_revenue: u64,
    market_c_m: u64,
    fee_bps: u16,
    board: Option<(u64, u64, u64)>,
    risk_c_r: Option<u64>,
    slot: u64,
) -> Option<MarketProj> {
    if n == 0 || p0.len() != n as usize || theta.len() != n as usize || exposure.len() != n as usize {
        return None;
    }
    let (mut trading_revenue, mut premium_payable) = (market_trading_revenue, 0u64);
    let _ = market_c_m;
    if let Some((rev, _c_m_locked, prem)) = board {
        trading_revenue = rev;
        premium_payable = prem;
    }
    Some(MarketProj {
        market,
        family,
        status,
        n,
        beta,
        p0,
        theta,
        exposure,
        trading_revenue,
        premium_payable,
        c_m: 0,
        c_r: risk_c_r.unwrap_or(0),
        fee_bps,
        fee_timing: 0,
        slot,
        traders: 0,
        tickets: 0,
        stake_usdc: 0,
        board_phase: 0,
        rho_raw: 0,
        settle_cell: 0,
        liability: 0,
        c_p_board: 0,
        c_p_alloc: 0,
        close_ts: 0,
        risk_lock_ts: 0,
        report_window_secs: 0,
        extra_a: 0,
        extra_b: 0,
        extra_u2: 0,
    })
}

/// Unique owners / open tickets / sum of cost_paid. q≤0 is a closed ticket.
pub fn position_stats<'a, I>(datas: I) -> HashMap<String, (u64, u64, u64)>
where
    I: IntoIterator<Item = &'a [u8]>,
{
    let mut owners: HashMap<String, HashSet<String>> = HashMap::new();
    let mut tickets: HashMap<String, u64> = HashMap::new();
    let mut stake: HashMap<String, u64> = HashMap::new();
    for data in datas {
        let Ok(pos) = decode_position(data) else { continue };
        if pos.q <= 0 {
            continue;
        }
        let m = pos.market.to_string();
        owners.entry(m.clone()).or_default().insert(pos.owner.to_string());
        *tickets.entry(m.clone()).or_default() += 1;
        *stake.entry(m).or_default() += pos.cost_paid;
    }
    owners
        .into_iter()
        .map(|(m, set)| {
            let t = tickets.get(&m).copied().unwrap_or(0);
            let s = stake.get(&m).copied().unwrap_or(0);
            (m, (set.len() as u64, t, s))
        })
        .collect()
}

pub async fn reconcile_ledger(rpc_url: &str, store: &MemoryStore, pool: &sqlx::PgPool) -> Result<()> {
    let rpc = RpcClient::new_with_commitment(rpc_url.to_string(), CommitmentConfig::confirmed());
    let Ok(genesis) = rpc.get_genesis_hash().await else {
        return Ok(());
    };
    apply_genesis(store, Some(pool), genesis.to_string()).await
}

async fn apply_genesis(store: &MemoryStore, pool: Option<&sqlx::PgPool>, genesis: String) -> Result<()> {
    if store.ledger_genesis().as_deref() == Some(genesis.as_str()) {
        return Ok(());
    }
    if let Some(p) = pool {
        if stored_ledger_genesis(p).await?.as_deref() == Some(genesis.as_str()) {
            store.set_ledger_genesis(genesis);
            return Ok(());
        }
        store.wipe_derived();
        wipe_projection_tables(p).await?;
        write_ledger_genesis(p, &genesis).await?;
        eprintln!("indexer: ledger genesis changed; dropped stale projections");
    } else {
        store.wipe_derived();
    }
    store.set_ledger_genesis(genesis);
    Ok(())
}

pub async fn poll_once(rpc: &RpcClient, store: &MemoryStore, pool: Option<&sqlx::PgPool>) -> Result<u64> {
    if let Ok(genesis) = rpc.get_genesis_hash().await {
        apply_genesis(store, pool, genesis.to_string()).await?;
    }
    let slot = rpc.get_slot().await?;
    store.set_slot(slot);
    let accounts = rpc.get_program_accounts(&client::market::ID).await?;
    let stats = position_stats(accounts.iter().map(|(_, a)| a.data.as_slice()));
    let mut tickets = Vec::new();
    for (key, acc) in &accounts {
        let Ok(pos) = decode_position(&acc.data) else { continue };
        if pos.cost_paid == 0 && pos.q <= 0 {
            continue;
        }
        tickets.push(PositionRow {
            position: key.to_string(),
            owner: pos.owner.to_string(),
            market: pos.market.to_string(),
            set_hash: pos.set_hash.iter().map(|b| format!("{b:02x}")).collect(),
            q_raw: pos.q,
            shares: usdc(Q64::from_raw(pos.q.max(0))),
            cost_paid: pos.cost_paid,
            claimed: pos.claimed,
            paid_usdc: 0,
        });
    }
    let mut taps: HashMap<String, (u64, u64)> = HashMap::new();
    let mut pool_avail = None;
    if let Ok(vaccs) = rpc.get_program_accounts(&client::vault::ID).await {
        let mut paid = HashMap::new();
        for (_, acc) in vaccs {
            if let Ok(c) = decode_claim(&acc.data) {
                paid.insert(c.position.to_string(), c.paid);
            } else if let Ok(p) = decode_pool(&acc.data) {
                pool_avail = Some(p.available);
            } else if let Ok(t) = decode_tap(&acc.data) {
                taps.insert(t.market.to_string(), (t.cap, t.allocated));
            }
        }
        store.set_pool(pool_avail.unwrap_or(0));
        for t in &mut tickets {
            if let Some(p) = paid.get(&t.position) {
                t.paid_usdc = *p;
                t.claimed = true;
            }
        }
    }
    if let Ok(raccs) = rpc.get_program_accounts(&client::risk::ID).await {
        let mut quotes = Vec::new();
        let mut layers = Vec::new();
        for (key, acc) in raccs {
            if let Ok(q) = decode_quote(&acc.data) {
                quotes.push(QuoteRow {
                    quote: key.to_string(),
                    market: q.market.to_string(),
                    lp: q.lp.to_string(),
                    layer_id: q.layer_id,
                    capacity: q.capacity,
                    filled: q.filled,
                    premium: q.premium,
                    premium_owed: q.premium_owed,
                    profit_share_bps: q.profit_share_bps,
                    cancelled: q.cancelled,
                });
            } else if let Ok(l) = decode_layer(&acc.data) {
                layers.push(LayerRow {
                    layer: key.to_string(),
                    market: l.market.to_string(),
                    layer_id: l.layer_id,
                    attachment: l.attachment,
                    thickness: l.thickness,
                    filled: l.filled,
                    quote_count: l.quote_count,
                });
            }
        }
        store.replace_risk(quotes, layers);
    }
    if let Ok(resaccs) = rpc.get_program_accounts(&client::resolution::ID).await {
        let mut rows = Vec::new();
        for (key, acc) in resaccs {
            let Ok(r) = decode_resolution(&acc.data) else { continue };
            let n = r.n as usize;
            rows.push(ResolutionRow {
                market: r.market.to_string(),
                record: key.to_string(),
                phase: r.phase,
                family: r.family,
                m: r.m,
                n: r.n,
                extensions: r.extensions,
                votes_proposal: r.votes_proposal,
                votes_challenge: r.votes_challenge,
                refunds_due: r.refunds_due,
                early_resolve: r.early_resolve,
                close_ts: r.close_ts,
                report_deadline: r.report_deadline,
                challenge_end: r.challenge_end,
                vote_end: r.vote_end,
                report_window_secs: r.report_window_secs,
                challenge_secs: r.challenge_secs,
                proposer: r.proposer.to_string(),
                challenger: r.challenger.to_string(),
                authorized_reporter: r.authorized_reporter.to_string(),
                members: r.members.iter().take(n).map(|p| p.to_string()).collect(),
                proposed: outcome_snap(&r.proposed),
                challenged: outcome_snap(&r.challenged),
                final_outcome: outcome_snap(&r.final_outcome),
                evidence_hash: r.evidence_hash.iter().map(|b| format!("{b:02x}")).collect(),
            });
        }
        store.replace_resolutions(rows);
    }
    if let Ok(acc) = rpc.get_account(&committee_pda()).await {
        if let Ok(c) = decode_committee(&acc.data) {
            let n = c.member_count as usize;
            store.set_committee(CommitteeSnap {
                authority: c.authority.to_string(),
                members: c.members.iter().take(n).map(|p| p.to_string()).collect(),
                m: c.m,
                n: c.member_count,
                epoch: c.epoch,
            });
        } else {
            store.clear_committee();
        }
    } else {
        store.clear_committee();
    }
    store.replace_positions(tickets);
    let live: HashSet<String> = accounts
        .iter()
        .filter_map(|(key, acc)| decode_market(&acc.data).ok().map(|_| key.to_string()))
        .collect();
    for (key, acc) in accounts {
        let Ok(mkt) = decode_market(&acc.data) else { continue };
        let gacc = match rpc.get_account(&grid_pda(&key)).await {
            Ok(a) => a,
            Err(_) => continue,
        };
        let Ok(grid) = decode_grid(&gacc.data) else { continue };
        let board_decoded = match rpc.get_account(&board(&key)).await {
            Ok(bacc) => decode_board(&bacc.data).ok(),
            Err(_) => None,
        };
        let board_triple = board_decoded
            .as_ref()
            .map(|b| (b.trading_revenue, b.c_m_locked, b.premium_payable));
        let risk_c_r = match rpc.get_account(&risk_book(&key)).await {
            Ok(racc) => decode_risk_book(&racc.data).ok().map(|b| b.c_r),
            Err(_) => None,
        };
        let Some(row) = project(
            key.to_string(),
            mkt.family,
            mkt.status,
            mkt.n,
            mkt.beta,
            grid.p0,
            grid.theta,
            grid.exposure,
            mkt.trading_revenue,
            mkt.c_m,
            mkt.fee_bps,
            board_triple,
            risk_c_r,
            slot,
        ) else {
            continue;
        };
        let mut row = row;
        row.close_ts = mkt.close_ts;
        row.risk_lock_ts = mkt.risk_lock_ts;
        row.fee_timing = mkt.fee_timing;
        row.report_window_secs = mkt.report_window_secs;
        row.extra_a = mkt.extra.a;
        row.extra_b = mkt.extra.b;
        row.extra_u2 = mkt.extra.u2;
        if let Some(b) = board_decoded {
            row.board_phase = b.phase;
            row.rho_raw = b.rho_raw;
            row.settle_cell = b.cell;
            row.liability = b.liability;
        } else if let Some(old) = store.get(&row.market) {
            row.board_phase = old.board_phase;
            row.rho_raw = old.rho_raw;
            row.settle_cell = old.settle_cell;
            row.liability = old.liability;
        }
        if let Some((traders, tickets, stake)) = stats.get(&row.market) {
            row.traders = *traders;
            row.tickets = *tickets;
            row.stake_usdc = *stake;
        }
        if let Some((cap, alloc)) = taps.get(&row.market) {
            row.c_p_board = *cap;
            row.c_p_alloc = *alloc;
        }
        store.upsert(row);
    }
    store.retain_markets(&live);
    for (market, (traders, tickets, stake)) in stats {
        store.patch_stats(&market, traders, tickets, stake);
    }
    for (market, (cap, alloc)) in taps {
        store.patch_tap(&market, cap, alloc);
    }
    if let Some(p) = pool {
        persist_projections(p, store).await?;
    }
    Ok(slot)
}

pub fn spawn_poller(url: String, store: Arc<MemoryStore>, pool: Option<sqlx::PgPool>, every_ms: u64) {
    tokio::spawn(async move {
        let rpc = RpcClient::new_with_commitment(url, CommitmentConfig::confirmed());
        loop {
            if let Err(e) = poll_once(&rpc, &store, pool.as_ref()).await {
                eprintln!("indexer poll: {e}");
            }
            tokio::time::sleep(Duration::from_millis(every_ms.max(200))).await;
        }
    });
}

fn outcome_snap(o: &client::resolution::Outcome) -> OutcomeSnap {
    OutcomeSnap {
        kind: o.kind,
        a: o.a.to_string(),
        b: o.b.to_string(),
        label: outcome_label(o.family, o.kind, o.a, o.b),
    }
}

fn outcome_label(family: u8, kind: u8, a: i128, b: i128) -> String {
    match kind {
        0 => {
            let ha = if a > 10 { "10+".to_string() } else { a.to_string() };
            let aw = if b > 10 { "10+".to_string() } else { b.to_string() };
            format!("{ha}-{aw}")
        }
        1 => {
            let x = a as f64 / ((1u128 << 64) as f64);
            if (x - x.round()).abs() < 1e-6 {
                format!("{}", x.round() as i64)
            } else {
                format!("{x:.3}")
            }
        }
        2 => format!("atom {a}"),
        4 => {
            if a == 1 {
                "YES".into()
            } else {
                "NO".into()
            }
        }
        _ => format!("family {family} kind {kind}"),
    }
}
