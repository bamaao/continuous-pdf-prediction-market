//! Market API + optional embedded indexer for localnet.

use anyhow::Result;
use readpath::{router, spawn_poller, MemoryStore};
use std::net::SocketAddr;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    let listen: SocketAddr = std::env::var("LISTEN")
        .unwrap_or_else(|_| "127.0.0.1:8080".into())
        .parse()?;
    let rpc = std::env::var("RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8899".into());
    let store = Arc::new(MemoryStore::new());
    let pool = match std::env::var("DATABASE_URL") {
        Ok(url) => Some(readpath::store::pg_migrate_and_load(&url, &store).await?),
        Err(_) => None,
    };
    if std::env::var("EMBED_INDEXER").unwrap_or_else(|_| "1".into()) != "0" {
        spawn_poller(rpc, store.clone(), pool, 400);
        eprintln!("market-api embed-indexer on {listen}");
    } else {
        eprintln!("market-api listen {listen} (projections only)");
    }
    let listener = tokio::net::TcpListener::bind(listen).await?;
    axum::serve(listener, router(store)).await?;
    Ok(())
}
