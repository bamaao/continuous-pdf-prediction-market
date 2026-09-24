//! Standalone indexer. Writes memory (and Postgres when DATABASE_URL is set).

use anyhow::Result;
use readpath::store::MemoryStore;
use readpath::{poll_once, spawn_poller};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    let rpc = std::env::var("RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8899".into());
    let store = Arc::new(MemoryStore::new());
    let pool = match std::env::var("DATABASE_URL") {
        Ok(url) => Some(readpath::store::pg_migrate_and_load(&url, &store).await?),
        Err(_) => None,
    };
    let client = RpcClient::new_with_commitment(rpc.clone(), CommitmentConfig::confirmed());
    let slot = poll_once(&client, &store, pool.as_ref()).await.unwrap_or(0);
    eprintln!("indexer start slot={slot} markets={}", store.list().len());
    spawn_poller(rpc, store, pool, 400);
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
    }
}
