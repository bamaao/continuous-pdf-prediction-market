//! Trading Gateway process. Forwards client-signed txs. Never stores keys.

use anyhow::Result;
use gateway::router;
use std::net::SocketAddr;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<()> {
    let listen: SocketAddr = std::env::var("LISTEN")
        .unwrap_or_else(|_| "127.0.0.1:8081".into())
        .parse()?;
    let rpc = std::env::var("RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8899".into());
    let receipts = PathBuf::from(std::env::var("RECEIPT_DIR").unwrap_or_else(|_| "receipts".into()));
    eprintln!(
        "trading-gateway listen {listen} rpc={rpc} holds_keys=false receipts={}",
        receipts.display()
    );
    let listener = tokio::net::TcpListener::bind(listen).await?;
    axum::serve(listener, router(rpc, receipts)).await?;
    Ok(())
}
