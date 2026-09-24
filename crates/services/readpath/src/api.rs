//! Market API (FR-TRD-02, FR-TRD-10, IR-07, IR-08). Reads projections, never the fill path.

use crate::store::MemoryStore;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use math::settle::usdc;
use math::Q64;
use quote::{decode_mask, q_bps, QuoteView};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tower_http::cors::CorsLayer;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<MemoryStore>,
}

#[derive(Deserialize)]
pub struct QuoteQ {
    pub mask: String,
    #[serde(default = "one")]
    pub shares: i64,
}

fn one() -> i64 {
    1
}

#[derive(Serialize)]
pub struct QuoteJson {
    pub market: String,
    pub slot: u64,
    pub n: u16,
    pub p_s_raw: String,
    pub p_s_bps: u64,
    pub c_s_raw: String,
    pub c_s_usdc: u64,
    pub coverage_bps: u64,
    pub rho_hat_bps: u64,
    pub l_max_usdc: u64,
    pub c_max_usdc: u64,
    pub r_net: u64,
}

#[derive(Serialize)]
pub struct PdfJson {
    pub market: String,
    pub slot: u64,
    pub cells: Vec<PdfCell>,
}

#[derive(Serialize)]
pub struct PdfCell {
    pub cell: usize,
    pub p_bps: u64,
    pub e: u64,
}

#[derive(Serialize)]
pub struct MarketListItem {
    pub market: String,
    pub family: u8,
    pub n: u16,
    pub slot: u64,
}

fn parse_mask_hex(hex: &str) -> Result<Vec<u8>, StatusCode> {
    let h = hex.trim().trim_start_matches("0x");
    if h.len() % 2 != 0 {
        return Err(StatusCode::BAD_REQUEST);
    }
    (0..h.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&h[i..i + 2], 16).map_err(|_| StatusCode::BAD_REQUEST))
        .collect()
}

fn view_json(market: &str, v: QuoteView) -> QuoteJson {
    QuoteJson {
        market: market.to_string(),
        slot: v.slot,
        n: v.n,
        p_s_raw: v.p_s.raw().to_string(),
        p_s_bps: q_bps(v.p_s),
        c_s_raw: v.c_s.raw().to_string(),
        c_s_usdc: usdc(v.c_s),
        coverage_bps: q_bps(v.coverage),
        rho_hat_bps: q_bps(v.rho_hat),
        l_max_usdc: v.l_max_usdc,
        c_max_usdc: v.c_max_usdc,
        r_net: v.r_net,
    }
}

async fn health(State(st): State<AppState>) -> impl IntoResponse {
    Json(serde_json::json!({ "ok": true, "slot": st.store.slot() }))
}

async fn list_markets(State(st): State<AppState>) -> impl IntoResponse {
    let rows: Vec<MarketListItem> = st
        .store
        .list()
        .into_iter()
        .map(|m| MarketListItem {
            market: m.market,
            family: m.family,
            n: m.n,
            slot: m.slot,
        })
        .collect();
    Json(rows)
}

async fn quote_one(
    State(st): State<AppState>,
    Path(market): Path<String>,
    Query(q): Query<QuoteQ>,
) -> Result<Json<QuoteJson>, StatusCode> {
    let row = st.store.get(&market).ok_or(StatusCode::NOT_FOUND)?;
    let mask = parse_mask_hex(&q.mask)?;
    let in_set = decode_mask(&mask, row.n as usize).map_err(|_| StatusCode::BAD_REQUEST)?;
    let book = row.book();
    let shares = if q.shares > 0 { q.shares } else { 1 };
    Ok(Json(view_json(&market, book.view(&in_set, Q64::from_int(shares)))))
}

async fn pdf_one(State(st): State<AppState>, Path(market): Path<String>) -> Result<Json<PdfJson>, StatusCode> {
    let row = st.store.get(&market).ok_or(StatusCode::NOT_FOUND)?;
    let book = row.book();
    let p = book.pdf();
    let cells = p
        .iter()
        .enumerate()
        .map(|(i, pk)| PdfCell {
            cell: i,
            p_bps: q_bps(*pk),
            e: usdc(book.state.exposure[i]),
        })
        .collect();
    Ok(Json(PdfJson {
        market,
        slot: row.slot,
        cells,
    }))
}

async fn ws_market(ws: WebSocketUpgrade, State(st): State<AppState>, Path(market): Path<String>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| ws_loop(socket, st, market))
}

async fn ws_loop(socket: WebSocket, st: AppState, market: String) {
    let (mut tx, mut rx) = socket.split();
    let mut last_theta = Vec::new();
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    loop {
        tokio::select! {
            _ = tick.tick() => {
                let Some(row) = st.store.get(&market) else { continue };
                if row.theta == last_theta {
                    continue;
                }
                last_theta = row.theta.clone();
                let book = row.book();
                let p = book.pdf();
                let body = serde_json::json!({
                    "market": market,
                    "slot": row.slot,
                    "p_bps": p.iter().map(|q| q_bps(*q)).collect::<Vec<_>>(),
                    "coverage_bps": q_bps(book.coverage()),
                    "rho_hat_bps": q_bps(book.rho_hat()),
                    "l_max_usdc": book.l_max_usdc(),
                    "c_max_usdc": book.c_max_usdc(),
                });
                if tx.send(Message::Text(body.to_string().into())).await.is_err() {
                    break;
                }
            }
            msg = rx.next() => {
                if msg.is_none() { break; }
            }
        }
    }
}

pub fn router(store: Arc<MemoryStore>) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/markets", get(list_markets))
        .route("/v1/markets/{market}/quote", get(quote_one))
        .route("/v1/markets/{market}/pdf", get(pdf_one))
        .route("/v1/markets/{market}/ws", get(ws_market))
        .layer(CorsLayer::permissive())
        .with_state(AppState { store })
}
