//! Projections of on-chain θ / E / capital. Postgres is optional; memory is the ledger-free cache.

use anyhow::Result;
use quote::Book;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketProj {
    pub market: String,
    pub family: u8,
    pub status: u8,
    pub n: u16,
    pub beta: i128,
    pub p0: Vec<i128>,
    pub theta: Vec<i128>,
    pub exposure: Vec<i128>,
    pub trading_revenue: u64,
    pub premium_payable: u64,
    pub c_m: u64,
    pub c_r: u64,
    pub slot: u64,
}

impl MarketProj {
    pub fn book(&self) -> Book {
        Book::from_grid(
            self.beta,
            &self.p0,
            &self.theta,
            &self.exposure,
            self.trading_revenue,
            self.premium_payable,
            self.c_m,
            self.c_r,
            self.slot,
        )
    }
}

#[derive(Clone, Default)]
pub struct MemoryStore {
    inner: Arc<RwLock<Inner>>,
}

#[derive(Default)]
struct Inner {
    markets: HashMap<String, MarketProj>,
    slot: u64,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn upsert(&self, row: MarketProj) {
        let mut g = self.inner.write().expect("store");
        g.slot = g.slot.max(row.slot);
        g.markets.insert(row.market.clone(), row);
    }

    pub fn get(&self, market: &str) -> Option<MarketProj> {
        self.inner.read().expect("store").markets.get(market).cloned()
    }

    pub fn list(&self) -> Vec<MarketProj> {
        self.inner.read().expect("store").markets.values().cloned().collect()
    }

    pub fn slot(&self) -> u64 {
        self.inner.read().expect("store").slot
    }

    pub fn set_slot(&self, slot: u64) {
        self.inner.write().expect("store").slot = slot;
    }
}

pub async fn pg_migrate_and_load(url: &str, mem: &MemoryStore) -> Result<sqlx::PgPool> {
    let pool = sqlx::PgPool::connect(url).await?;
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS market_proj (
            market TEXT PRIMARY KEY,
            slot BIGINT NOT NULL,
            family SMALLINT NOT NULL,
            status SMALLINT NOT NULL,
            n INTEGER NOT NULL,
            beta TEXT NOT NULL,
            p0 JSONB NOT NULL,
            theta JSONB NOT NULL,
            exposure JSONB NOT NULL,
            trading_revenue BIGINT NOT NULL,
            premium_payable BIGINT NOT NULL,
            c_m BIGINT NOT NULL,
            c_r BIGINT NOT NULL,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )
        "#,
    )
    .execute(&pool)
    .await?;
    let rows: Vec<(
        String,
        i64,
        i16,
        i16,
        i32,
        String,
        serde_json::Value,
        serde_json::Value,
        serde_json::Value,
        i64,
        i64,
        i64,
        i64,
    )> = sqlx::query_as(
        "SELECT market, slot, family, status, n, beta, p0, theta, exposure,
                trading_revenue, premium_payable, c_m, c_r FROM market_proj",
    )
    .fetch_all(&pool)
    .await?;
    for (market, slot, family, status, n, beta, p0, theta, exposure, tr, prem, c_m, c_r) in rows {
        mem.upsert(MarketProj {
            market,
            family: family as u8,
            status: status as u8,
            n: n as u16,
            beta: beta.parse().unwrap_or(0),
            p0: json_i128(p0),
            theta: json_i128(theta),
            exposure: json_i128(exposure),
            trading_revenue: tr as u64,
            premium_payable: prem as u64,
            c_m: c_m as u64,
            c_r: c_r as u64,
            slot: slot as u64,
        });
    }
    Ok(pool)
}

pub async fn pg_upsert(pool: &sqlx::PgPool, row: &MarketProj) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO market_proj (
            market, slot, family, status, n, beta, p0, theta, exposure,
            trading_revenue, premium_payable, c_m, c_r
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
        ON CONFLICT (market) DO UPDATE SET
            slot = EXCLUDED.slot,
            family = EXCLUDED.family,
            status = EXCLUDED.status,
            n = EXCLUDED.n,
            beta = EXCLUDED.beta,
            p0 = EXCLUDED.p0,
            theta = EXCLUDED.theta,
            exposure = EXCLUDED.exposure,
            trading_revenue = EXCLUDED.trading_revenue,
            premium_payable = EXCLUDED.premium_payable,
            c_m = EXCLUDED.c_m,
            c_r = EXCLUDED.c_r,
            updated_at = now()
        "#,
    )
    .bind(&row.market)
    .bind(row.slot as i64)
    .bind(row.family as i16)
    .bind(row.status as i16)
    .bind(row.n as i32)
    .bind(row.beta.to_string())
    .bind(serde_json::to_value(row.p0.iter().map(|x| x.to_string()).collect::<Vec<_>>())?)
    .bind(serde_json::to_value(row.theta.iter().map(|x| x.to_string()).collect::<Vec<_>>())?)
    .bind(serde_json::to_value(row.exposure.iter().map(|x| x.to_string()).collect::<Vec<_>>())?)
    .bind(row.trading_revenue as i64)
    .bind(row.premium_payable as i64)
    .bind(row.c_m as i64)
    .bind(row.c_r as i64)
    .execute(pool)
    .await?;
    Ok(())
}

fn json_i128(v: serde_json::Value) -> Vec<i128> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().and_then(|s| s.parse().ok()).or_else(|| x.as_i64().map(i128::from)))
                .collect()
        })
        .unwrap_or_default()
}
