//! Local websocket / RPC poller. Yellowstone is the production transport; this is the local path.

use crate::store::{pg_upsert, MarketProj, MemoryStore};
use anyhow::Result;
use client::{board, decode_board, decode_grid, decode_market, decode_risk_book, grid_pda, risk_book};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
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
    board: Option<(u64, u64, u64)>,
    risk_c_r: Option<u64>,
    slot: u64,
) -> Option<MarketProj> {
    if n == 0 || p0.len() != n as usize || theta.len() != n as usize || exposure.len() != n as usize {
        return None;
    }
    let (mut trading_revenue, mut c_m, mut premium_payable) = (market_trading_revenue, market_c_m, 0u64);
    if let Some((rev, c_m_locked, prem)) = board {
        trading_revenue = rev;
        if c_m_locked > 0 {
            c_m = c_m_locked;
        }
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
        c_m,
        c_r: risk_c_r.unwrap_or(0),
        slot,
    })
}

pub async fn poll_once(rpc: &RpcClient, store: &MemoryStore, pool: Option<&sqlx::PgPool>) -> Result<u64> {
    let slot = rpc.get_slot().await?;
    store.set_slot(slot);
    let accounts = rpc.get_program_accounts(&client::market::ID).await?;
    for (key, acc) in accounts {
        let Ok(mkt) = decode_market(&acc.data) else { continue };
        let gacc = match rpc.get_account(&grid_pda(&key)).await {
            Ok(a) => a,
            Err(_) => continue,
        };
        let Ok(grid) = decode_grid(&gacc.data) else { continue };
        let board_triple = match rpc.get_account(&board(&key)).await {
            Ok(bacc) => decode_board(&bacc.data)
                .ok()
                .map(|b| (b.trading_revenue, b.c_m_locked, b.premium_payable)),
            Err(_) => None,
        };
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
            board_triple,
            risk_c_r,
            slot,
        ) else {
            continue;
        };
        store.upsert(row.clone());
        if let Some(p) = pool {
            pg_upsert(p, &row).await?;
        }
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
