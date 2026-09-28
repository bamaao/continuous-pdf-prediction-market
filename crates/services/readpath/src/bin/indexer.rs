//! Standalone indexer. Writes memory cache + Postgres.

use anyhow::Result;
use readpath::store::MemoryStore;
use readpath::{memory_only, open_required_pool, poll_once, spawn_poller};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    let rpc = std::env::var("RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8899".into());
    let store = Arc::new(MemoryStore::new());
    let pool = if memory_only() {
        None
    } else {
        Some(open_required_pool(&store).await?)
    };
    let client = RpcClient::new_with_commitment(rpc.clone(), CommitmentConfig::confirmed());
    let slot = poll_once(&client, &store, pool.as_ref()).await.unwrap_or(0);
    eprintln!("indexer start slot={slot} markets={} pg={}", store.list().len(), pool.is_some());
    spawn_poller(rpc, store, pool, 400);
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
    }
}
