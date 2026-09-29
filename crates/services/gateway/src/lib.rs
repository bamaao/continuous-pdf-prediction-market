//! Trading Gateway (FR-WAL-09, FR-TRD-09, NFR-13–18). Forwards signed txs. No keys.

pub mod limit;
pub mod store;

use crate::limit::RateLimit;
use crate::store::{sha256_hex, FileStore, Stored};
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use journal::Journal;
use serde::{Deserialize, Serialize};
use solana_client::rpc_client::RpcClient;
use solana_client::rpc_config::RpcSendTransactionConfig;
use solana_sdk::signature::Signature;
use solana_sdk::transaction::Transaction;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;
use tower_http::cors::CorsLayer;

const MAX_TX_BYTES: usize = 4096;

#[derive(Clone)]
pub struct AppState {
    pub rpc: String,
    pub store: FileStore,
    pub limit: RateLimit,
    pub journal: Option<Journal>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Receipt {
    pub status: String,
    #[serde(default)]
    pub sig: String,
    pub owner: String,
    pub market: String,
    pub nonce: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Receipt {
    pub fn key(&self) -> Option<String> {
        Some(receipt_key(&self.owner, &self.market, self.nonce))
    }
}

#[derive(Deserialize)]
pub struct SubmitBody {
    pub tx_b64: String,
    pub owner: String,
    pub market: String,
    pub nonce: u64,
}

#[derive(Deserialize)]
pub struct ReceiptQ {
    pub owner: Option<String>,
    pub market: Option<String>,
    pub nonce: Option<u64>,
    pub sig: Option<String>,
}

fn receipt_key(owner: &str, market: &str, nonce: u64) -> String {
    format!("{owner}:{market}:{nonce}")
}

pub fn reject_secrets(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Object(m) => m.keys().any(|k| {
            let k = k.to_ascii_lowercase();
            k.contains("private") || k.contains("secret") || k.contains("mnemonic") || k == "keypair"
        }) || m.values().any(reject_secrets),
        serde_json::Value::Array(a) => a.iter().any(reject_secrets),
        _ => false,
    }
}

pub fn router(rpc: String, receipt_dir: PathBuf) -> Router {
    router_with_journal(rpc, receipt_dir, None)
}

pub fn router_with_journal(rpc: String, receipt_dir: PathBuf, journal: Option<Journal>) -> Router {
    let store = FileStore::open(&receipt_dir).expect("receipt dir");
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/submit", post(submit))
        .route("/v1/receipt", get(receipt))
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(CorsLayer::permissive())
        .with_state(AppState {
            rpc,
            store,
            limit: RateLimit::new(20, Duration::from_secs(10)),
            journal,
        })
}

async fn health() -> impl axum::response::IntoResponse {
    Json(serde_json::json!({
        "ok": true,
        "holds_keys": false,
        "receipts": "durable-files",
    }))
}

async fn submit(
    State(st): State<AppState>,
    Json(raw): Json<serde_json::Value>,
) -> Result<Json<Receipt>, (StatusCode, String)> {
    if reject_secrets(&raw) {
        return Err((StatusCode::BAD_REQUEST, "secrets rejected".into()));
    }
    let body: SubmitBody = serde_json::from_value(raw).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    if body.owner.is_empty() || body.market.is_empty() || body.nonce == 0 {
        return Err((StatusCode::BAD_REQUEST, "owner, market, nonce required".into()));
    }
    if !st.limit.allow(&body.owner) {
        return Err((StatusCode::TOO_MANY_REQUESTS, "rate limited".into()));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(body.tx_b64.trim())
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    if bytes.len() > MAX_TX_BYTES {
        return Err((StatusCode::BAD_REQUEST, "tx too large".into()));
    }
    let tx: Transaction = bincode::deserialize(&bytes).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    if tx.signatures.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "unsigned tx".into()));
    }
    let key = receipt_key(&body.owner, &body.market, body.nonce);
    if let Some(prev) = st.store.get(&key) {
        if prev.receipt.status == "confirmed" {
            return Ok(Json(prev.receipt));
        }
    }
    let rec = Receipt {
        status: "pending".into(),
        sig: String::new(),
        owner: body.owner.clone(),
        market: body.market.clone(),
        nonce: body.nonce,
        error: None,
    };
    let stored = Stored {
        receipt: rec.clone(),
        tx_b64: body.tx_b64,
        tx_sha: sha256_hex(&bytes),
    };
    st.store
        .put(&key, stored)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let st2 = st.clone();
    let key2 = key.clone();
    tokio::spawn(async move {
        tokio::task::spawn_blocking(move || forward(st2, key2)).await.ok();
    });
    Ok(Json(rec))
}

fn forward(st: AppState, key: String) {
    let Some(row) = st.store.get(&key) else { return };
    if row.receipt.status == "confirmed" {
        return;
    }
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(row.tx_b64.trim()) else {
        mark_failed(&st, &key, row, "bad stored tx");
        return;
    };
    let Ok(tx) = bincode::deserialize::<Transaction>(&bytes) else {
        mark_failed(&st, &key, row, "bad stored tx");
        return;
    };
    let rpc = RpcClient::new(st.rpc.clone());
    let cfg = RpcSendTransactionConfig {
        skip_preflight: true,
        ..Default::default()
    };
    let mut next = row.clone();
    match rpc.send_transaction_with_config(&tx, cfg) {
        Ok(sig) => {
            next.receipt.sig = sig.to_string();
            next.receipt.status = "pending".into();
            next.receipt.error = None;
            let _ = st.store.put(&key, next.clone());
            match rpc.confirm_transaction(&sig) {
                Ok(_) => {
                    next.receipt.status = "confirmed".into();
                    let _ = st.store.put(&key, next.clone());
                    if let Some(j) = &st.journal {
                        if let Err(e) = j.append(
                            &next.receipt.market,
                            &next.receipt.owner,
                            next.receipt.nonce,
                            &next.receipt.sig,
                        ) {
                            eprintln!("journal append: {e}");
                        }
                    }
                }
                Err(e) => {
                    next.receipt.error = Some(e.to_string());
                    let _ = st.store.put(&key, next);
                }
            }
        }
        Err(e) => mark_failed(&st, &key, next, &e.to_string()),
    }
}

fn mark_failed(st: &AppState, key: &str, mut row: Stored, err: &str) {
    row.receipt.status = "failed".into();
    row.receipt.error = Some(err.to_string());
    let _ = st.store.put(key, row);
}

async fn receipt(State(st): State<AppState>, Query(q): Query<ReceiptQ>) -> Result<Json<Receipt>, StatusCode> {
    if let (Some(owner), Some(market), Some(nonce)) = (q.owner.as_ref(), q.market.as_ref(), q.nonce) {
        if let Some(row) = st.store.get(&receipt_key(owner, market, nonce)) {
            return Ok(Json(row.receipt));
        }
    }
    if let Some(sig) = q.sig.as_ref() {
        let rpc = RpcClient::new(st.rpc.clone());
        let parsed = Signature::from_str(sig).map_err(|_| StatusCode::BAD_REQUEST)?;
        let statuses = rpc.get_signature_statuses(&[parsed]).map_err(|_| StatusCode::BAD_GATEWAY)?;
        let confirmed = statuses
            .value
            .first()
            .and_then(|s| s.as_ref())
            .map(|s| s.satisfies_commitment(solana_sdk::commitment_config::CommitmentConfig::confirmed()))
            .unwrap_or(false);
        return Ok(Json(Receipt {
            status: if confirmed { "confirmed" } else { "pending" }.into(),
            sig: sig.clone(),
            owner: q.owner.unwrap_or_default(),
            market: q.market.unwrap_or_default(),
            nonce: q.nonce.unwrap_or(0),
            error: None,
        }));
    }
    Err(StatusCode::NOT_FOUND)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use axum::Router;
    use http_body_util::BodyExt;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tower::ServiceExt;

    fn tmp_dir() -> PathBuf {
        let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let p = std::env::temp_dir().join(format!("cpm-gw-{n}"));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn rejects_private_key_payloads() {
        let v = serde_json::json!({ "tx_b64": "AA==", "private_key": "dead" });
        assert!(reject_secrets(&v));
        let ok = serde_json::json!({ "tx_b64": "AA==", "nonce": 1 });
        assert!(!reject_secrets(&ok));
    }

    #[test]
    fn receipt_survives_reopen() {
        let dir = tmp_dir();
        let a = FileStore::open(&dir).unwrap();
        let rec = Receipt {
            status: "pending".into(),
            sig: String::new(),
            owner: "o".into(),
            market: "m".into(),
            nonce: 1,
            error: None,
        };
        a.put(
            "o:m:1",
            Stored {
                receipt: rec.clone(),
                tx_b64: "dHg=".into(),
                tx_sha: "ab".into(),
            },
        )
        .unwrap();
        drop(a);
        let b = FileStore::open(&dir).unwrap();
        let got = b.get("o:m:1").unwrap();
        assert_eq!(got.receipt.status, "pending");
        assert!(!got.tx_b64.contains("private"));
    }

    fn signed_tx_b64() -> String {
        use solana_sdk::signer::Signer;
        let kp = solana_sdk::signature::Keypair::new();
        let ix = solana_sdk::system_instruction::transfer(&kp.pubkey(), &kp.pubkey(), 0);
        let mut tx = Transaction::new_with_payer(&[ix], Some(&kp.pubkey()));
        tx.sign(&[&kp], solana_sdk::hash::Hash::new_unique());
        base64::engine::general_purpose::STANDARD.encode(bincode::serialize(&tx).unwrap())
    }

    async fn post_submit(app: Router, body: serde_json::Value) -> (StatusCode, serde_json::Value) {
        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/submit")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_else(|_| serde_json::json!({ "raw": String::from_utf8_lossy(&bytes) }));
        (status, v)
    }

    #[tokio::test]
    async fn submit_acks_pending_without_rpc() {
        let dir = tmp_dir();
        let app = router("http://127.0.0.1:1".into(), dir.clone());
        let tx_b64 = signed_tx_b64();
        let body = serde_json::json!({
            "tx_b64": tx_b64,
            "owner": "Own111111111111111111111111111111111111111",
            "market": "Mkt111111111111111111111111111111111111111",
            "nonce": 1
        });
        let (status, v) = post_submit(app, body).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(v["status"], "pending");
        assert!(v.get("tx_b64").is_none());
        let store = FileStore::open(&dir).unwrap();
        let row = store
            .get("Own111111111111111111111111111111111111111:Mkt111111111111111111111111111111111111111:1")
            .unwrap();
        assert_eq!(row.receipt.status, "pending");
    }

    #[tokio::test]
    async fn confirmed_same_nonce_is_not_replaced() {
        let dir = tmp_dir();
        let store = FileStore::open(&dir).unwrap();
        store
            .put(
                "o:m:3",
                Stored {
                    receipt: Receipt {
                        status: "confirmed".into(),
                        sig: "Sig111".into(),
                        owner: "o".into(),
                        market: "m".into(),
                        nonce: 3,
                        error: None,
                    },
                    tx_b64: "old".into(),
                    tx_sha: "aa".into(),
                },
            )
            .unwrap();
        let app = router("http://127.0.0.1:1".into(), dir);
        let body = serde_json::json!({
            "tx_b64": signed_tx_b64(),
            "owner": "o",
            "market": "m",
            "nonce": 3
        });
        let (status, v) = post_submit(app, body).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(v["status"], "confirmed");
        assert_eq!(v["sig"], "Sig111");
    }

    #[tokio::test]
    async fn http_rejects_secrets_and_rate_limit() {
        let dir = tmp_dir();
        let app = router("http://127.0.0.1:1".into(), dir);
        let (st, _) = post_submit(
            app.clone(),
            serde_json::json!({"tx_b64":"AA==","owner":"x","market":"y","nonce":1,"secret":"no"}),
        )
        .await;
        assert_eq!(st, StatusCode::BAD_REQUEST);

        let owner = "rate-owner";
        let mut last = StatusCode::OK;
        for n in 1..=21 {
            let body = serde_json::json!({
                "tx_b64": signed_tx_b64(),
                "owner": owner,
                "market": "m",
                "nonce": n
            });
            let (st, _) = post_submit(app.clone(), body).await;
            last = st;
        }
        assert_eq!(last, StatusCode::TOO_MANY_REQUESTS);
    }
}
