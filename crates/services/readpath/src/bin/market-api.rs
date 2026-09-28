//! Market API + optional embedded indexer for localnet.

use anyhow::Result;
use readpath::{memory_only, open_required_pool, router_with_pool, spawn_poller, MemoryStore};
use std::net::SocketAddr;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    let listen: SocketAddr = std::env::var("LISTEN")
        .unwrap_or_else(|_| "127.0.0.1:8080".into())
        .parse()?;
    let rpc = std::env::var("RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8899".into());
    let store = Arc::new(MemoryStore::new());
    let listings = std::env::var("LISTINGS_PATH").unwrap_or_else(|_| "tmp/cpm-listings.json".into());
    if memory_only() || std::path::Path::new(&listings).exists() {
        store.persist_listings(&listings);
    }
    let fills = std::env::var("TICKETS_PATH").unwrap_or_else(|_| "tmp/cpm-tickets.json".into());
    if memory_only() || std::path::Path::new(&fills).exists() {
        store.persist_fills(&fills);
    }
    let pool = if memory_only() {
        eprintln!("market-api ALLOW_MEMORY_ONLY=1 — journals die on restart");
        None
    } else {
        Some(open_required_pool(&store).await?)
    };
    if std::env::var("EMBED_INDEXER").unwrap_or_else(|_| "1".into()) != "0" {
        spawn_poller(rpc, store.clone(), pool.clone(), 400);
        eprintln!(
            "market-api embed-indexer on {listen} pg={}",
            if pool.is_some() { "on" } else { "off" }
        );
    } else {
        eprintln!("market-api listen {listen} (projections only)");
    }
    let listener = tokio::net::TcpListener::bind(listen).await?;
    axum::serve(listener, router_with_pool(store, pool)).await?;
    Ok(())
}
