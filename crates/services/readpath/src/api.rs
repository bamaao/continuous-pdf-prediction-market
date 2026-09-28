//! Market API (FR-TRD-02, FR-TRD-10, IR-07, IR-08). Reads projections, never the fill path.

use crate::application::CatalogService;
use crate::compose::{compose_out, ComposeBody};
use crate::domain::{
    normalize_tags, region_blocked, valid_pubkey, CatalogTag, Comment, DomainError, Fill, Listing, ListingApplication,
    ReviewLog, APP_APPROVED, APP_DUPLICATE, APP_PENDING, APP_REJECTED,
};
use crate::peak::{peak_risk, PeakRisk};
use crate::store::{ApplicationRow, CommentRow, FillMeta, ListingMeta, MarketProj, MemoryStore, ReviewLogRow};
use axum::http::HeaderMap;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use client::{ata, decode_user_vault, user_vault};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use math::settle::{layer_loss, usdc};
use math::Q64;
use quote::{decode_mask, q_bps, q_bps_renorm, ticket_from_view, ticket_from_view_ex, QuoteView};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tower_http::cors::CorsLayer;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<MemoryStore>,
    pub catalog: CatalogService,
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
    pub fee_bps: u16,
    pub fee_usdc: u64,
    pub pay_usdc: u64,
    pub face_usdc: u64,
    pub payout_if_hit_usdc: u64,
    pub payout_if_miss_usdc: u64,
    pub net_if_hit: i64,
    pub ev_if_p_s: i64,
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

fn pdf_cells(p: &[Q64], exposure: &[Q64]) -> Vec<PdfCell> {
    let bps = q_bps_renorm(p);
    bps.into_iter()
        .enumerate()
        .map(|(i, p_bps)| PdfCell {
            cell: i,
            p_bps,
            e: usdc(exposure.get(i).copied().unwrap_or(Q64::ZERO)),
        })
        .collect()
}

#[derive(Serialize)]
pub struct MarketListItem {
    pub market: String,
    pub family: u8,
    pub status: u8,
    pub n: u16,
    pub slot: u64,
    pub traders: u64,
    pub stake_usdc: u64,
    pub l_max_usdc: u64,
    pub c_r: u64,
    pub title: String,
    pub tags: Vec<String>,
    pub category: String,
    pub topic: String,
    pub tag: String,
    pub description: String,
    pub event: String,
    pub close_ts: i64,
    pub risk_lock_ts: i64,
    pub report_open_ts: i64,
    pub extensions: u8,
    pub abnormal: String,
    pub final_result: String,
    pub liability: u64,
    pub c_max_usdc: u64,
    pub payable_usdc: u64,
    pub coverage_bps: u64,
    pub peak_risk: PeakRisk,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution_phase: Option<u8>,
}

#[derive(Serialize)]
pub struct MarketListPage {
    pub page: u32,
    pub limit: u32,
    pub total: u64,
    pub pages: u32,
    pub q: String,
    pub family: Option<u8>,
    pub status: Option<u8>,
    pub category: Option<String>,
    pub tag: Option<String>,
    pub items: Vec<MarketListItem>,
}

#[derive(Deserialize)]
pub struct ListQ {
    #[serde(default)]
    pub q: String,
    pub family: Option<u8>,
    pub status: Option<u8>,
    pub category: Option<String>,
    pub tag: Option<String>,
    #[serde(default = "page_one")]
    pub page: u32,
    #[serde(default = "limit_default")]
    pub limit: u32,
}

fn page_one() -> u32 {
    1
}

fn limit_default() -> u32 {
    20
}

fn family_label(family: u8) -> &'static str {
    match family {
        0 => "skellam",
        1 => "gaussian",
        2 => "lognormal",
        3 => "dirichlet",
        4 => "bernoulli",
        _ => "unknown",
    }
}

fn status_label(status: u8) -> &'static str {
    match status {
        1 => "trading",
        2 => "halted",
        3 => "settled",
        4 => "void",
        _ => "unknown",
    }
}

fn market_matches(m: &crate::store::MarketProj, listing: Option<&ListingMeta>, q: &ListQ) -> bool {
    if let Some(f) = q.family {
        if m.family != f {
            return false;
        }
    }
    if let Some(s) = q.status {
        if m.status != s {
            return false;
        }
    }
    let want_tag = q
        .tag
        .as_ref()
        .or(q.category.as_ref())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());
    if let Some(want) = want_tag {
        let have = listing.map(|l| l.resolved_tags()).unwrap_or_default();
        if !have.iter().any(|t| t.eq_ignore_ascii_case(want)) {
            return false;
        }
    }
    let needle = q.q.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return true;
    }
    let title = listing.map(|l| l.title.to_ascii_lowercase()).unwrap_or_default();
    let tags = listing.map(|l| l.tags_label().to_ascii_lowercase()).unwrap_or_default();
    let topic = listing.map(|l| l.topic.to_ascii_lowercase()).unwrap_or_default();
    let tag = listing.map(|l| l.tag.to_ascii_lowercase()).unwrap_or_default();
    let description = listing.map(|l| l.description.to_ascii_lowercase()).unwrap_or_default();
    let event = listing.map(|l| l.event.to_ascii_lowercase()).unwrap_or_default();
    m.market.to_ascii_lowercase().contains(&needle)
        || family_label(m.family).contains(needle.as_str())
        || status_label(m.status).contains(needle.as_str())
        || title.contains(&needle)
        || tags.contains(&needle)
        || topic.contains(&needle)
        || tag.contains(&needle)
        || description.contains(&needle)
        || event.contains(&needle)
}

fn display_title(listing: &ListingMeta, family: u8) -> String {
    let t = listing.title.trim();
    if !t.is_empty() {
        return t.to_string();
    }
    match family {
        0 => "Football prediction market",
        1 => "Gaussian prediction market",
        2 => "Lognormal prediction market",
        3 => "Dirichlet prediction market",
        4 => "Bernoulli prediction market",
        _ => "Prediction market",
    }
    .into()
}

fn hex32(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn recover_mask(n: u16, want: &str) -> Option<String> {
    if n == 0 || n > 16 {
        return None;
    }
    let want = want.trim_start_matches("0x").to_ascii_lowercase();
    let need = (n as usize + 7) / 8;
    let max = 1u32 << n;
    for bits in 1..max {
        let mut bytes = vec![0u8; need];
        for i in 0..n as u32 {
            if bits & (1 << i) != 0 {
                bytes[(i / 8) as usize] |= 1 << (i % 8);
            }
        }
        if hex32(&client::market::ids::set_hash(&bytes)) == want {
            return Some(hex_bytes(&bytes));
        }
    }
    None
}

fn recover_skellam(want: &str) -> Option<(u8, i64, i64)> {
    let want = want.trim_start_matches("0x").to_ascii_lowercase();
    for kind in 0u8..=11 {
        for a in -10i16..=10 {
            for b in 0i16..=10 {
                if hex32(&client::market::ids::skellam_ticket(kind, a, b)) == want {
                    return Some((kind, i64::from(a), i64::from(b)));
                }
            }
        }
    }
    None
}

fn fill_for(st: &AppState, owner: &str, market: &str, set_hash: &str, family: u8, n: u16) -> Option<FillMeta> {
    if let Some(row) = st.store.fill_of(owner, market, set_hash) {
        return Some(row);
    }
    if family == 0 {
        if let Some((kind, a, b)) = recover_skellam(set_hash) {
            return Some(FillMeta {
                owner: owner.into(),
                market: market.into(),
                set_hash: set_hash.trim_start_matches("0x").to_ascii_lowercase(),
                kind: "skellam".into(),
                mask: String::new(),
                skellam_kind: Some(kind),
                a: Some(a),
                b: Some(b),
            });
        }
    }
    recover_mask(n, set_hash).map(|mask| FillMeta {
        owner: owner.into(),
        market: market.into(),
        set_hash: set_hash.trim_start_matches("0x").to_ascii_lowercase(),
        kind: "mask".into(),
        mask,
        skellam_kind: None,
        a: None,
        b: None,
    })
}

fn fill_from_meta(row: &FillMeta) -> Result<Fill, crate::domain::DomainError> {
    let mut fill = Fill::new(&row.owner, &row.market)?;
    fill.set_hash = row.set_hash.trim_start_matches("0x").to_ascii_lowercase();
    fill.kind = row.kind.clone();
    fill.mask = row.mask.clone();
    fill.skellam_kind = row.skellam_kind;
    fill.a = row.a;
    fill.b = row.b;
    Ok(fill)
}

fn remember_compose_fill(st: &AppState, body: &ComposeBody) -> Option<Fill> {
    let Some(market) = body.market.as_deref() else {
        return None;
    };
    let owner = body.owner.trim();
    if owner.is_empty() {
        return None;
    }
    match body.op.as_str() {
        "buy_set" | "sell_set" => {
            let Some(mask) = body.mask.as_deref() else {
                return None;
            };
            let Ok(bytes) = crate::compose::mask_bytes(mask) else {
                return None;
            };
            let row = FillMeta {
                owner: owner.into(),
                market: market.into(),
                set_hash: hex32(&client::market::ids::set_hash(&bytes)),
                kind: "mask".into(),
                mask: hex_bytes(&bytes),
                skellam_kind: None,
                a: None,
                b: None,
            };
            st.store.set_fill(row.clone());
            return fill_from_meta(&row).ok();
        }
        "buy_skellam_set" | "sell_skellam_set" => {
            let kind = body.kind.unwrap_or(0);
            let a = body.value.unwrap_or(0) as i16;
            let b = body.value_b.unwrap_or(0) as i16;
            let row = FillMeta {
                owner: owner.into(),
                market: market.into(),
                set_hash: hex32(&client::market::ids::skellam_ticket(kind, a, b)),
                kind: "skellam".into(),
                mask: body.mask.clone().unwrap_or_default(),
                skellam_kind: Some(kind),
                a: Some(i64::from(a)),
                b: Some(i64::from(b)),
            };
            st.store.set_fill(row.clone());
            return fill_from_meta(&row).ok();
        }
        _ => {}
    }
    None
}

fn hydrate_payout_fill(st: &AppState, body: &mut ComposeBody) {
    if body.op != "payout" && body.op != "payout_skellam" {
        return;
    }
    let Some(market) = body.market.clone() else {
        return;
    };
    let set_hash = body
        .set_hash
        .clone()
        .or_else(|| {
            body.position.as_ref().and_then(|p| {
                st.store
                    .positions_of(&body.owner)
                    .into_iter()
                    .find(|row| row.position == *p)
                    .map(|row| row.set_hash)
            })
        })
        .unwrap_or_default();
    let family = st.store.get(&market).map(|m| m.family).unwrap_or(0);
    let n = st.store.get(&market).map(|m| m.n).unwrap_or(0);
    let Some(fill) = fill_for(st, &body.owner, &market, &set_hash, family, n) else {
        return;
    };
    if body.op == "payout" && body.mask.is_none() && !fill.mask.is_empty() {
        body.mask = Some(fill.mask);
    }
    if body.op == "payout_skellam" && body.kind.is_none() {
        body.kind = fill.skellam_kind;
        body.value = fill.a;
        body.value_b = fill.b;
    }
}

fn abnormal_of(m: &crate::store::MarketProj, res: Option<&crate::store::ResolutionRow>) -> String {
    if m.status == 4 || m.board_phase == 2 {
        return "void_refund".into();
    }
    if let Some(r) = res {
        if r.phase == 4 || r.phase == 5 || r.refunds_due {
            return "resolution_failed_refund".into();
        }
    }
    String::new()
}

fn final_result_of(res: Option<&crate::store::ResolutionRow>) -> String {
    match res {
        Some(r) if r.phase == 3 && !r.final_outcome.label.is_empty() => r.final_outcome.label.clone(),
        _ => String::new(),
    }
}

fn to_list_item(m: crate::store::MarketProj, listing: ListingMeta, res: Option<crate::store::ResolutionRow>) -> MarketListItem {
    let book = m.book();
    let c_max = book.c_max_usdc();
    let l = if m.board_phase >= 1 { m.liability } else { book.l_max_usdc() };
    let resolution_phase = res.as_ref().map(|r| r.phase);
    let extensions = res.as_ref().map(|r| r.extensions).unwrap_or(0);
    let close_ts = if m.close_ts > 0 {
        m.close_ts
    } else {
        res.as_ref().map(|r| r.close_ts).unwrap_or(0)
    };
    let title = display_title(&listing, m.family);
    let abnormal = abnormal_of(&m, res.as_ref());
    let final_result = final_result_of(res.as_ref());
    let liability = if m.board_phase >= 1 { m.liability } else { 0 };
    let peak = peak_risk(&m);
    MarketListItem {
        market: m.market,
        family: m.family,
        status: m.status,
        n: m.n,
        slot: m.slot,
        traders: m.traders,
        stake_usdc: m.stake_usdc,
        l_max_usdc: book.l_max_usdc(),
        c_r: m.c_r,
        title,
        tags: listing.resolved_tags(),
        category: listing.resolved_tags().first().cloned().unwrap_or_default(),
        topic: listing.topic,
        tag: listing.tag,
        description: listing.description,
        event: listing.event,
        close_ts,
        risk_lock_ts: m.risk_lock_ts,
        report_open_ts: close_ts,
        extensions,
        abnormal,
        final_result,
        liability,
        c_max_usdc: c_max,
        payable_usdc: l.min(c_max),
        coverage_bps: quote::q_bps(book.coverage()),
        peak_risk: peak,
        resolution_phase,
    }
}

fn phase_name(phase: u8) -> &'static str {
    match phase {
        0 => "Open",
        1 => "Proposed",
        2 => "Voting",
        3 => "Finalized",
        4 => "Failed",
        5 => "Voided",
        _ => "Unknown",
    }
}

fn default_pk() -> String {
    solana_sdk::pubkey::Pubkey::default().to_string()
}

async fn resolution_one(State(st): State<AppState>, Path(market): Path<String>) -> Result<impl IntoResponse, StatusCode> {
    let row = st.store.resolution_of(&market).ok_or(StatusCode::NOT_FOUND)?;
    let zero = default_pk();
    Ok(Json(serde_json::json!({
        "market": row.market,
        "record": row.record,
        "phase": row.phase,
        "phase_name": phase_name(row.phase),
        "family": row.family,
        "m": row.m,
        "n": row.n,
        "extensions": row.extensions,
        "votes_proposal": row.votes_proposal,
        "votes_challenge": row.votes_challenge,
        "refunds_due": row.refunds_due,
        "early_resolve": row.early_resolve,
        "close_ts": row.close_ts,
        "report_deadline": row.report_deadline,
        "challenge_end": row.challenge_end,
        "vote_end": row.vote_end,
        "report_window_secs": row.report_window_secs,
        "challenge_secs": row.challenge_secs,
        "proposer": row.proposer,
        "challenger": row.challenger,
        "authorized_reporter": row.authorized_reporter,
        "members": row.members,
        "proposed": row.proposed,
        "challenged": row.challenged,
        "final_outcome": row.final_outcome,
        "evidence_hash": row.evidence_hash,
        "has_proposed": row.proposer != zero,
        "has_challenged": row.challenger != zero,
        "has_final": row.phase == 3,
    })))
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

fn view_json(market: &str, v: QuoteView, shares: i64, fee_bps: u16) -> QuoteJson {
    view_json_ex(market, v, shares, fee_bps, 0)
}

fn view_json_ex(market: &str, v: QuoteView, shares: i64, fee_bps: u16, fee_timing: u8) -> QuoteJson {
    let t = ticket_from_view_ex(&v, shares, fee_bps, fee_timing);
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
        fee_bps: t.fee_bps,
        fee_usdc: t.fee_usdc,
        pay_usdc: t.pay_usdc,
        face_usdc: t.face_usdc,
        payout_if_hit_usdc: t.payout_if_hit_usdc,
        payout_if_miss_usdc: t.payout_if_miss_usdc,
        net_if_hit: t.net_if_hit,
        ev_if_p_s: t.ev_if_p_s,
    }
}

#[derive(Deserialize)]
pub struct OwnerPosQ {
    #[serde(default)]
    pub q: String,
    #[serde(default = "page_one")]
    pub page: u32,
    #[serde(default = "limit_default")]
    pub limit: u32,
    #[serde(default)]
    pub filter: String,
}

#[derive(Serialize)]
pub struct PositionJson {
    pub position: String,
    pub market: String,
    pub family: u8,
    pub status: u8,
    pub board_phase: u8,
    pub set_hash: String,
    pub shares: u64,
    pub cost_paid: u64,
    pub claimed: bool,
    pub paid_usdc: u64,
    pub net_usdc: Option<i64>,
    pub rho_hat_bps: u64,
    pub settle_cell: u16,
    pub prompt: String,
    pub bucket: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub event: String,
    #[serde(default)]
    pub mask: String,
    #[serde(default)]
    pub ticket_kind: String,
    pub skellam_kind: Option<u8>,
    pub a: Option<i64>,
    pub b: Option<i64>,
}

#[derive(Serialize)]
pub struct OwnerPositionsPage {
    pub owner: String,
    pub page: u32,
    pub limit: u32,
    pub total: u64,
    pub pages: u32,
    pub claimable: u64,
    pub paid_tickets: u64,
    pub paid_usdc: u64,
    pub net_claimed: i64,
    pub items: Vec<PositionJson>,
}

fn ticket_bucket(prompt: &str) -> String {
    match prompt {
        "unclaimed_settle" | "unclaimed_refund" => "unclaimed",
        "paid" => "won",
        "claimed_zero" => "lost",
        "refunded" => "refunded",
        _ => "open",
    }
    .into()
}

fn ticket_prompt(board_phase: u8, claimed: bool, paid: u64) -> String {
    match (board_phase, claimed) {
        (1, false) => "unclaimed_settle".into(),
        (2, false) => "unclaimed_refund".into(),
        (1, true) if paid > 0 => "paid".into(),
        (1, true) => "claimed_zero".into(),
        (2, true) => "refunded".into(),
        _ => "open".into(),
    }
}

async fn owner_positions(
    State(st): State<AppState>,
    Path(owner): Path<String>,
    Query(q): Query<OwnerPosQ>,
) -> impl IntoResponse {
    let mut rows: Vec<PositionJson> = st
        .store
        .positions_of(&owner)
        .into_iter()
        .map(|p| {
            let m = st.store.get(&p.market);
            let family = m.as_ref().map(|x| x.family).unwrap_or(0);
            let status = m.as_ref().map(|x| x.status).unwrap_or(0);
            let board_phase = m.as_ref().map(|x| x.board_phase).unwrap_or(0);
            let rho_hat_bps = m.as_ref().map(|x| q_bps(Q64::from_raw(x.rho_raw))).unwrap_or(0);
            let settle_cell = m.as_ref().map(|x| x.settle_cell).unwrap_or(0);
            let prompt = ticket_prompt(board_phase, p.claimed, p.paid_usdc);
            let bucket = ticket_bucket(&prompt);
            let net_usdc = if p.claimed {
                Some(p.paid_usdc as i64 - p.cost_paid as i64)
            } else {
                None
            };
            let listing = st.store.listing_of(&p.market).unwrap_or_default();
            let n = m.as_ref().map(|x| x.n).unwrap_or(0);
            let fill = fill_for(&st, &p.owner, &p.market, &p.set_hash, family, n);
            PositionJson {
                position: p.position,
                market: p.market.clone(),
                family,
                status,
                board_phase,
                set_hash: p.set_hash,
                shares: p.shares,
                cost_paid: p.cost_paid,
                claimed: p.claimed,
                paid_usdc: p.paid_usdc,
                net_usdc,
                rho_hat_bps,
                settle_cell,
                prompt,
                bucket,
                title: display_title(&listing, family),
                category: listing.tags_label(),
                event: listing.event,
                mask: fill.as_ref().map(|f| f.mask.clone()).unwrap_or_default(),
                ticket_kind: fill.as_ref().map(|f| f.kind.clone()).unwrap_or_default(),
                skellam_kind: fill.as_ref().and_then(|f| f.skellam_kind),
                a: fill.as_ref().and_then(|f| f.a),
                b: fill.as_ref().and_then(|f| f.b),
            }
        })
        .filter(|p| {
            if !q.filter.is_empty() && p.bucket != q.filter {
                return false;
            }
            let needle = q.q.trim().to_ascii_lowercase();
            if needle.is_empty() {
                return true;
            }
            p.market.to_ascii_lowercase().contains(&needle)
                || p.title.to_ascii_lowercase().contains(&needle)
                || p.event.to_ascii_lowercase().contains(&needle)
                || p.category.to_ascii_lowercase().contains(&needle)
                || family_label(p.family).contains(needle.as_str())
                || p.prompt.contains(&needle)
                || p.bucket.contains(&needle)
                || status_label(p.status).contains(needle.as_str())
        })
        .collect();
    rows.sort_by(|a, b| a.market.cmp(&b.market).then(a.position.cmp(&b.position)));
    let claimable = rows
        .iter()
        .filter(|p| p.prompt == "unclaimed_settle" || p.prompt == "unclaimed_refund")
        .count() as u64;
    let paid_tickets = rows.iter().filter(|p| p.prompt == "paid" || p.prompt == "refunded").count() as u64;
    let paid_usdc = rows.iter().map(|p| p.paid_usdc).sum();
    let net_claimed = rows.iter().filter_map(|p| p.net_usdc).sum();
    let total = rows.len() as u64;
    let limit = q.limit.clamp(1, 100);
    let page = q.page.max(1);
    let pages = if total == 0 { 1 } else { ((total + u64::from(limit) - 1) / u64::from(limit)) as u32 };
    let start = (u64::from(page.saturating_sub(1)).saturating_mul(u64::from(limit))) as usize;
    let items = rows.into_iter().skip(start).take(limit as usize).collect();
    Json(OwnerPositionsPage {
        owner,
        page,
        limit,
        total,
        pages,
        claimable,
        paid_tickets,
        paid_usdc,
        net_claimed,
        items,
    })
}

async fn health(State(st): State<AppState>) -> impl IntoResponse {
    Json(serde_json::json!({
        "ok": true,
        "slot": st.store.slot(),
        "pg": st.catalog.has_pg(),
    }))
}

async fn list_markets(State(st): State<AppState>, Query(q): Query<ListQ>, headers: HeaderMap) -> impl IntoResponse {
    let country = request_country(&headers);
    let mut rows: Vec<_> = st
        .store
        .list()
        .into_iter()
        .filter(|m| {
            let listing = st.store.listing_of(&m.market);
            catalog_listed(&st, &m.market)
                && !listing_geo_hidden(listing.as_ref(), country.as_deref())
                && market_matches(m, listing.as_ref(), &q)
        })
        .collect();
    rows.sort_by(|a, b| b.slot.cmp(&a.slot).then_with(|| a.market.cmp(&b.market)));
    let total = rows.len() as u64;
    let limit = q.limit.clamp(1, 100);
    let page = q.page.max(1);
    let pages = if total == 0 {
        1
    } else {
        ((total + u64::from(limit) - 1) / u64::from(limit)) as u32
    };
    let start = (u64::from(page.saturating_sub(1)).saturating_mul(u64::from(limit))) as usize;
    let items = rows
        .into_iter()
        .skip(start)
        .take(limit as usize)
        .map(|m| {
            let res = st.store.resolution_of(&m.market);
            let listing = st.store.listing_of(&m.market).unwrap_or_default();
            to_list_item(m, listing, res)
        })
        .collect();
    Json(MarketListPage {
        page,
        limit,
        total,
        pages,
        q: q.q,
        family: q.family,
        status: q.status,
        category: q.category,
        tag: q.tag,
        items,
    })
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
    Ok(Json(view_json_ex(
        &market,
        book.view(&in_set, Q64::from_int(shares)),
        shares,
        row.fee_bps,
        row.fee_timing,
    )))
}

#[derive(Serialize)]
pub struct BookJson {
    pub market: String,
    pub family: u8,
    pub status: u8,
    pub n: u16,
    pub slot: u64,
    pub beta_raw: String,
    pub p0_raw: Vec<String>,
    pub theta_raw: Vec<String>,
    pub exposure_raw: Vec<String>,
    pub trading_revenue: u64,
    pub premium_payable: u64,
    pub c_m: u64,
    pub c_r: u64,
    pub fee_bps: u16,
}

async fn book_one(State(st): State<AppState>, Path(market): Path<String>) -> Result<Json<BookJson>, StatusCode> {
    let row = st.store.get(&market).ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(BookJson {
        market,
        family: row.family,
        status: row.status,
        n: row.n,
        slot: row.slot,
        beta_raw: row.beta.to_string(),
        p0_raw: row.p0.iter().map(|x| x.to_string()).collect(),
        theta_raw: row.theta.iter().map(|x| x.to_string()).collect(),
        exposure_raw: row.exposure.iter().map(|x| x.to_string()).collect(),
        trading_revenue: row.trading_revenue,
        premium_payable: row.premium_payable,
        c_m: row.c_m,
        c_r: row.c_r,
        fee_bps: row.fee_bps,
    }))
}

async fn preview_one(
    State(st): State<AppState>,
    Path(market): Path<String>,
    Query(q): Query<QuoteQ>,
) -> Result<Json<math_wasm::Preview>, StatusCode> {
    let row = st.store.get(&market).ok_or(StatusCode::NOT_FOUND)?;
    let snap = math_wasm::BookSnap {
        beta_raw: row.beta.to_string(),
        p0_raw: row.p0.iter().map(|x| x.to_string()).collect(),
        theta_raw: row.theta.iter().map(|x| x.to_string()).collect(),
        exposure_raw: row.exposure.iter().map(|x| x.to_string()).collect(),
        trading_revenue: row.trading_revenue,
        premium_payable: row.premium_payable,
        c_m: row.c_m,
        c_r: row.c_r,
        slot: row.slot,
        fee_bps: row.fee_bps,
    };
    math_wasm::preview_mask(&snap, &q.mask, q.shares)
        .map(Json)
        .map_err(|_| StatusCode::BAD_REQUEST)
}

#[derive(Serialize)]
pub struct InfoJson {
    pub market: String,
    pub title: String,
    pub tags: Vec<String>,
    pub category: String,
    pub topic: String,
    pub tag: String,
    pub description: String,
    pub event: String,
    pub close_ts: i64,
    pub risk_lock_ts: i64,
    pub report_open_ts: i64,
    pub extensions: u8,
    pub abnormal: String,
    pub final_result: String,
    pub payable_usdc: u64,
    pub family: u8,
    pub status: u8,
    pub n: u16,
    pub slot: u64,
    pub traders: u64,
    pub tickets: u64,
    pub stake_usdc: u64,
    pub trading_revenue: u64,
    pub l_max_usdc: u64,
    pub c_r: u64,
    pub c_m: u64,
    pub r_net: u64,
    pub c_max_usdc: u64,
    pub coverage_bps: u64,
    pub rho_hat_bps: u64,
    pub fee_bps: u16,
    pub fee_timing: u8,
    pub board_phase: u8,
    pub rho_raw: String,
    pub rho_bps: u64,
    pub settle_cell: u16,
    pub liability: u64,
    pub c_p_board: u64,
    pub c_p_alloc: u64,
    pub c_p_pool: u64,
    pub peak_risk: PeakRisk,
    pub cells: Vec<PdfCell>,
}

async fn info_one(
    State(st): State<AppState>,
    Path(market): Path<String>,
    headers: HeaderMap,
) -> Result<Json<InfoJson>, StatusCode> {
    let row = st.store.get(&market).ok_or(StatusCode::NOT_FOUND)?;
    if listing_geo_hidden(st.store.listing_of(&market).as_ref(), request_country(&headers).as_deref()) {
        return Err(StatusCode::FORBIDDEN);
    }
    let book = row.book();
    let p = book.pdf();
    let cells = pdf_cells(&p, &book.state.exposure);
    let listing = st.store.listing_of(&market).unwrap_or_default();
    let res = st.store.resolution_of(&market);
    let close_ts = if row.close_ts > 0 {
        row.close_ts
    } else {
        res.as_ref().map(|r| r.close_ts).unwrap_or(0)
    };
    let c_max = book.c_max_usdc();
    let l = if row.board_phase >= 1 { row.liability } else { book.l_max_usdc() };
    Ok(Json(InfoJson {
        market,
        title: display_title(&listing, row.family),
        tags: listing.resolved_tags(),
        category: listing.resolved_tags().first().cloned().unwrap_or_default(),
        topic: listing.topic,
        tag: listing.tag,
        description: listing.description,
        event: listing.event,
        close_ts,
        risk_lock_ts: row.risk_lock_ts,
        report_open_ts: close_ts,
        extensions: res.as_ref().map(|r| r.extensions).unwrap_or(0),
        abnormal: abnormal_of(&row, res.as_ref()),
        final_result: final_result_of(res.as_ref()),
        payable_usdc: l.min(c_max),
        family: row.family,
        status: row.status,
        n: row.n,
        slot: row.slot,
        traders: row.traders,
        tickets: row.tickets,
        stake_usdc: row.stake_usdc,
        trading_revenue: row.trading_revenue,
        l_max_usdc: book.l_max_usdc(),
        c_r: row.c_r,
        c_m: row.c_m,
        r_net: book.r_net(),
        c_max_usdc: c_max,
        coverage_bps: q_bps(book.coverage()),
        rho_hat_bps: q_bps(book.rho_hat()),
        fee_bps: row.fee_bps,
        fee_timing: row.fee_timing,
        board_phase: row.board_phase,
        rho_raw: row.rho_raw.to_string(),
        rho_bps: q_bps(Q64::from_raw(row.rho_raw)),
        settle_cell: row.settle_cell,
        liability: row.liability,
        c_p_board: row.c_p_board,
        c_p_alloc: row.c_p_alloc,
        c_p_pool: st.store.pool_available(),
        peak_risk: peak_risk(&row),
        cells,
    }))
}

async fn pdf_one(State(st): State<AppState>, Path(market): Path<String>) -> Result<Json<PdfJson>, StatusCode> {
    let row = st.store.get(&market).ok_or(StatusCode::NOT_FOUND)?;
    let book = row.book();
    let p = book.pdf();
    let cells = pdf_cells(&p, &book.state.exposure);
    Ok(Json(PdfJson {
        market,
        slot: row.slot,
        cells,
    }))
}

async fn ws_market(ws: WebSocketUpgrade, State(st): State<AppState>, Path(market): Path<String>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| ws_loop(socket, st, market))
}

fn pdf_push(market: &str, row: &MarketProj) -> serde_json::Value {
    let book = row.book();
    let p = book.pdf();
    serde_json::json!({
        "market": market,
        "slot": row.slot,
        "p_bps": q_bps_renorm(&p),
        "e": book.state.exposure.iter().map(|x| usdc(*x)).collect::<Vec<_>>(),
        "coverage_bps": q_bps(book.coverage()),
        "rho_hat_bps": q_bps(book.rho_hat()),
        "l_max_usdc": book.l_max_usdc(),
        "c_max_usdc": book.c_max_usdc(),
        "peak_risk": peak_risk(row),
    })
}

async fn ws_loop(socket: WebSocket, st: AppState, market: String) {
    let (mut tx, mut rx) = socket.split();
    let mut last_theta = Vec::new();
    if let Some(row) = st.store.get(&market) {
        last_theta = row.theta.clone();
        if tx.send(Message::Text(pdf_push(&market, &row).to_string().into())).await.is_err() {
            return;
        }
    }
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    loop {
        tokio::select! {
            _ = tick.tick() => {
                let Some(row) = st.store.get(&market) else { continue };
                if row.theta == last_theta {
                    continue;
                }
                last_theta = row.theta.clone();
                if tx.send(Message::Text(pdf_push(&market, &row).to_string().into())).await.is_err() {
                    break;
                }
            }
            msg = rx.next() => {
                if msg.is_none() { break; }
            }
        }
    }
}

async fn list_auctions(State(st): State<AppState>, Query(q): Query<ListQ>, headers: HeaderMap) -> impl IntoResponse {
    let country = request_country(&headers);
    let mut rows: Vec<_> = st
        .store
        .list()
        .into_iter()
        .filter(|m| {
            let listing = st.store.listing_of(&m.market);
            catalog_listed(&st, &m.market)
                && !listing_geo_hidden(listing.as_ref(), country.as_deref())
                && market_matches(m, listing.as_ref(), &q)
        })
        .collect();
    rows.sort_by(|a, b| b.c_r.cmp(&a.c_r).then_with(|| a.market.cmp(&b.market)));
    let total = rows.len() as u64;
    let limit = q.limit.clamp(1, 100);
    let page = q.page.max(1);
    let pages = if total == 0 { 1 } else { ((total + u64::from(limit) - 1) / u64::from(limit)) as u32 };
    let start = (u64::from(page.saturating_sub(1)).saturating_mul(u64::from(limit))) as usize;
    let items: Vec<_> = rows
        .into_iter()
        .skip(start)
        .take(limit as usize)
        .map(|m| {
            let listing = st.store.listing_of(&m.market).unwrap_or_default();
            serde_json::json!({
                "market": m.market,
                "family": m.family,
                "status": m.status,
                "title": display_title(&listing, m.family),
                "tags": listing.resolved_tags(),
                "category": listing.resolved_tags().first().cloned().unwrap_or_default(),
                "topic": listing.topic,
                "tag": listing.tag,
                "description": listing.description,
                "event": listing.event,
                "close_ts": m.close_ts,
                "c_r": m.c_r,
                "l_max_usdc": m.book().l_max_usdc(),
                "coverage_bps": quote::q_bps(m.book().coverage()),
                "peak_risk": peak_risk(&m),
                "layers": st.store.layers_of(&m.market).len().max(1) as u8,
            })
        })
        .collect();
    Json(serde_json::json!({
        "page": page,
        "limit": limit,
        "total": total,
        "pages": pages,
        "items": items,
    }))
}

fn unit_prem(premium: u64, capacity: u64) -> u128 {
    if capacity == 0 {
        0
    } else {
        (premium as u128).saturating_mul(1_000_000) / capacity as u128
    }
}

async fn layers_one(State(st): State<AppState>, Path(market): Path<String>) -> Result<impl IntoResponse, StatusCode> {
    let row = st.store.get(&market).ok_or(StatusCode::NOT_FOUND)?;
    let book = row.book();
    let indexed = st.store.layers_of(&market);
    let standing = st.store.quotes_on(&market);
    let layers: Vec<_> = if indexed.is_empty() {
        vec![serde_json::json!({
            "id": 1,
            "attachment": 0,
            "remaining": row.c_r.max(10),
            "unit_premium": 1,
            "gamma_bps": 1000,
            "quotes": standing.iter().filter(|q| q.layer_id == 1).count(),
            "filled": 0,
            "thickness": row.c_r.max(10),
        })]
    } else {
        indexed
            .into_iter()
            .map(|l| {
                let best = standing
                    .iter()
                    .filter(|q| q.layer_id == l.layer_id)
                    .map(|q| unit_prem(q.premium, q.capacity))
                    .min()
                    .unwrap_or(1);
                serde_json::json!({
                    "id": l.layer_id,
                    "attachment": l.attachment,
                    "remaining": l.thickness.saturating_sub(l.filled),
                    "unit_premium": best,
                    "gamma_bps": 1000,
                    "quotes": l.quote_count,
                    "filled": l.filled,
                    "thickness": l.thickness,
                })
            })
            .collect()
    };
    let quotes: Vec<_> = standing
        .into_iter()
        .map(|q| {
            serde_json::json!({
                "quote": q.quote,
                "lp": q.lp,
                "layer": q.layer_id,
                "capacity": q.capacity,
                "filled": q.filled,
                "premium": q.premium,
                "unit_premium": unit_prem(q.premium, q.capacity),
                "profit_share_bps": q.profit_share_bps,
                "premium_owed": q.premium_owed,
                "cancelled": q.cancelled,
            })
        })
        .collect();
    let listing = st.store.listing_of(&market).unwrap_or_default();
    Ok(Json(serde_json::json!({
        "market": market,
        "title": display_title(&listing, row.family),
        "tags": listing.resolved_tags(),
        "category": listing.resolved_tags().first().cloned().unwrap_or_default(),
        "topic": listing.topic,
        "tag": listing.tag,
        "description": listing.description,
        "event": listing.event,
        "close_ts": row.close_ts,
        "family": row.family,
        "status": row.status,
        "c_r": row.c_r,
        "l_max_usdc": book.l_max_usdc(),
        "coverage_bps": q_bps(book.coverage()),
        "peak_risk": peak_risk(&row),
        "layers": layers,
        "quotes": quotes,
    })))
}

#[derive(Deserialize)]
struct ListingBody {
    market: String,
    title: String,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    topic: String,
    #[serde(default)]
    tag: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    event: String,
    #[serde(default)]
    blocked_regions: Vec<String>,
}

fn tags_from_body(body: &ListingBody) -> Result<Vec<String>, StatusCode> {
    let mut raw = body.tags.clone();
    if raw.is_empty() {
        if let Some(c) = body.category.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
            raw.push(c.to_string());
        }
    }
    normalize_tags(raw).map_err(|_| StatusCode::BAD_REQUEST)
}

async fn put_listing(State(st): State<AppState>, Json(body): Json<ListingBody>) -> Result<impl IntoResponse, StatusCode> {
    let market = body.market.trim();
    let title = body.title.trim();
    if market.is_empty() || title.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if !listing_write_allowed(&st, market, body.topic.trim(), body.tag.trim()) {
        return Err(StatusCode::FORBIDDEN);
    }
    let tags = tags_from_body(&body)?;
    let listing = Listing::new(
        market,
        title,
        tags,
        body.topic.trim(),
        body.tag.trim(),
        body.description.trim(),
        body.event.trim(),
    )
    .map_err(|_| StatusCode::BAD_REQUEST)?;
    st.catalog
        .put_listing(&listing)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    st.store.set_listing(
        market,
        ListingMeta {
            title: listing.title.clone(),
            tags: listing.tags.clone(),
            category: listing.category.clone(),
            topic: listing.topic,
            tag: listing.tag,
            description: listing.description,
            event: listing.event,
            blocked_regions: body.blocked_regions.clone(),
        },
    );
    Ok(Json(serde_json::json!({
        "ok": true,
        "market": market,
        "title": title,
        "tags": listing.tags,
        "category": listing.category,
    })))
}

#[derive(Deserialize)]
struct TagBody {
    name: String,
}

async fn list_tags(State(st): State<AppState>) -> impl IntoResponse {
    Json(serde_json::json!({ "items": st.store.catalog_tags() }))
}

async fn put_tag(State(st): State<AppState>, Json(body): Json<TagBody>) -> Result<impl IntoResponse, StatusCode> {
    let tag = CatalogTag::new(body.name).map_err(|_| StatusCode::BAD_REQUEST)?;
    st.store.add_catalog_tag(&tag.name).map_err(|_| StatusCode::BAD_REQUEST)?;
    st.catalog.put_tag(&tag).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "name": tag.name,
        "used": st.store.tag_used(&tag.name),
    })))
}

async fn delete_tag(State(st): State<AppState>, Path(name): Path<String>) -> Result<impl IntoResponse, StatusCode> {
    let tag = CatalogTag::new(name).map_err(|_| StatusCode::BAD_REQUEST)?;
    match st.store.delete_catalog_tag(&tag.name) {
        Ok(name) => {
            st.catalog
                .delete_tag(&name)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            Ok(Json(serde_json::json!({ "ok": true, "name": name })))
        }
        Err(DomainError::Conflict(_)) => Err(StatusCode::CONFLICT),
        Err(DomainError::Invalid("unknown tag")) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::BAD_REQUEST),
    }
}

async fn put_ticket(State(st): State<AppState>, Json(body): Json<FillMeta>) -> Result<impl IntoResponse, StatusCode> {
    if body.owner.trim().is_empty() || body.market.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if body.mask.trim().is_empty() && body.skellam_kind.is_none() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut row = body;
    if row.set_hash.is_empty() {
        if row.kind == "skellam" {
            if let Some(kind) = row.skellam_kind {
                let a = row.a.unwrap_or(0) as i16;
                let b = row.b.unwrap_or(0) as i16;
                row.set_hash = hex32(&client::market::ids::skellam_ticket(kind, a, b));
            }
        } else if let Ok(bytes) = crate::compose::mask_bytes(&row.mask) {
            row.set_hash = hex32(&client::market::ids::set_hash(&bytes));
        }
    }
    if let Ok(fill) = fill_from_meta(&row) {
        st.catalog
            .put_fill(&fill)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    st.store.set_fill(row.clone());
    Ok(Json(serde_json::json!({ "ok": true, "set_hash": row.set_hash })))
}

#[derive(Deserialize)]
struct CommentPageQ {
    #[serde(default = "page_one")]
    page: u32,
    #[serde(default = "limit_default")]
    limit: u32,
}

#[derive(Deserialize)]
struct CommentBody {
    author: String,
    body: String,
}

fn comment_json(row: &CommentRow) -> serde_json::Value {
    serde_json::json!({
        "id": row.id,
        "market": row.market,
        "author": row.author,
        "body": row.body,
        "created_at": row.created_at,
    })
}

async fn list_comments(
    State(st): State<AppState>,
    Path(market): Path<String>,
    Query(q): Query<CommentPageQ>,
) -> Result<impl IntoResponse, StatusCode> {
    if st.store.get(&market).is_none() {
        return Err(StatusCode::NOT_FOUND);
    }
    let limit = q.limit.clamp(1, 100);
    let page = q.page.max(1);
    let (total, items) = st.store.comments_of(&market, page, limit);
    let pages = if total == 0 {
        1
    } else {
        ((total + u64::from(limit) - 1) / u64::from(limit)) as u32
    };
    Ok(Json(serde_json::json!({
        "market": market,
        "page": page,
        "limit": limit,
        "total": total,
        "pages": pages,
        "items": items.iter().map(comment_json).collect::<Vec<_>>(),
    })))
}

async fn put_comment(
    State(st): State<AppState>,
    Path(market): Path<String>,
    Json(body): Json<CommentBody>,
) -> Result<impl IntoResponse, StatusCode> {
    if st.store.get(&market).is_none() {
        return Err(StatusCode::NOT_FOUND);
    }
    let comment = Comment::new(&market, body.author, body.body).map_err(|_| StatusCode::BAD_REQUEST)?;
    let saved = st
        .catalog
        .put_comment(&comment)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let row = st.store.push_comment(CommentRow {
        id: saved.id,
        market: saved.market,
        author: saved.author,
        body: saved.body,
        created_at: saved.created_at,
    });
    Ok(Json(comment_json(&row)))
}

fn request_country(headers: &HeaderMap) -> Option<String> {
    for name in ["cf-ipcountry", "x-app-country", "x-country"] {
        if let Some(v) = headers.get(name).and_then(|v| v.to_str().ok()) {
            let t = v.trim();
            if !t.is_empty() && !t.eq_ignore_ascii_case("xx") {
                return Some(t.to_ascii_uppercase());
            }
        }
    }
    None
}

fn listing_geo_hidden(listing: Option<&ListingMeta>, country: Option<&str>) -> bool {
    listing
        .map(|l| region_blocked(&l.blocked_regions, country))
        .unwrap_or(false)
}

fn allow_unreviewed_create() -> bool {
    std::env::var("ALLOW_UNREVIEWED_CREATE").ok().as_deref() == Some("1")
}

fn is_create_op(op: &str) -> bool {
    matches!(
        op,
        "create_skellam" | "create_gaussian" | "create_lognormal" | "create_dirichlet" | "create_bernoulli"
    )
}

/// Lobby / auctions: reviewed OPEN boards, or a named listing with no application (grandfather).
fn catalog_listed(st: &AppState, market: &str) -> bool {
    if let Some(app) = st.store.application_by_market(market) {
        return app.status == APP_APPROVED && !app.market.trim().is_empty();
    }
    if st.store.applications().is_empty() {
        return true;
    }
    st.store.listing_of(market).is_some()
}

fn listing_write_allowed(st: &AppState, market: &str, topic: &str, tag: &str) -> bool {
    if allow_unreviewed_create() || st.store.applications().is_empty() {
        return true;
    }
    if st.store.application_by_market(market).is_some() {
        return true;
    }
    (0u8..=4).any(|f| st.store.approved_create_spec(f, topic, tag).is_some())
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn is_prediction_fill_op(op: &str) -> bool {
    matches!(
        op,
        "buy_set" | "sell_set" | "buy_skellam_set" | "sell_skellam_set"
    )
}

fn is_auction_write_op(op: &str) -> bool {
    matches!(op, "risk_quote" | "fill_next")
}

/// Indexed `close_ts` is a pre-check. The chain still rejects `now ≥ close_ts`.
fn require_trading_open(st: &AppState, body: &ComposeBody) -> Result<(), StatusCode> {
    if !is_prediction_fill_op(&body.op) && !is_auction_write_op(&body.op) {
        return Ok(());
    }
    let Some(market) = body.market.as_deref() else {
        return Ok(());
    };
    let Some(row) = st.store.get(market) else {
        return Ok(());
    };
    let now = unix_now();
    if is_prediction_fill_op(&body.op) && row.close_ts > 0 && now >= row.close_ts {
        return Err(StatusCode::FORBIDDEN);
    }
    if is_auction_write_op(&body.op) {
        let auction_lock = if row.risk_lock_ts > 0 {
            row.risk_lock_ts
        } else {
            row.close_ts
        };
        if (auction_lock > 0 && now >= auction_lock) || (row.close_ts > 0 && now >= row.close_ts) {
            return Err(StatusCode::FORBIDDEN);
        }
    }
    Ok(())
}

fn require_approved_create(st: &AppState, body: &ComposeBody) -> Result<(), StatusCode> {
    if !is_create_op(&body.op) || allow_unreviewed_create() {
        return Ok(());
    }
    if st.store.applications().is_empty() {
        return Ok(());
    }
    let family = body.family.unwrap_or(match body.op.as_str() {
        "create_skellam" => 0,
        "create_lognormal" => 2,
        "create_dirichlet" => 3,
        "create_bernoulli" => 4,
        _ => 1,
    });
    let topic = body.topic.as_deref().unwrap_or("").trim();
    let tag = body.tag.as_deref().unwrap_or("").trim();
    if topic.is_empty() || tag.is_empty() {
        return Err(StatusCode::FORBIDDEN);
    }
    st.store
        .approved_create_spec(family, topic, tag)
        .ok_or(StatusCode::FORBIDDEN)?;
    Ok(())
}

fn reviewer_allowlist() -> Vec<String> {
    std::env::var("REVIEWER_PUBKEYS")
        .unwrap_or_default()
        .split(|c| c == ',' || c == ' ' || c == ';')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn is_reviewer(pk: &str) -> bool {
    if !valid_pubkey(pk) {
        return false;
    }
    let list = reviewer_allowlist();
    list.is_empty() || list.iter().any(|x| x == pk)
}

fn app_status_name(row: &ApplicationRow) -> &'static str {
    if row.status == APP_APPROVED && !row.market.trim().is_empty() {
        return "open";
    }
    match row.status {
        APP_APPROVED => "approved",
        APP_REJECTED => "rejected",
        APP_DUPLICATE => "duplicate",
        _ => "pending_review",
    }
}

fn application_json(row: &ApplicationRow, logs: &[ReviewLogRow]) -> serde_json::Value {
    serde_json::json!({
        "id": row.id,
        "applicant": row.applicant,
        "family": row.family,
        "title": row.title,
        "tags": row.tags,
        "event": row.event,
        "description": row.description,
        "topic": row.topic,
        "tag": row.tag,
        "blocked_regions": row.blocked_regions,
        "dup_key": row.dup_key,
        "status": row.status,
        "status_name": app_status_name(row),
        "reviewer": row.reviewer,
        "reason": row.reason,
        "created_at": row.created_at,
        "reviewed_at": row.reviewed_at,
        "compose": serde_json::from_str::<serde_json::Value>(&row.compose_json).unwrap_or_else(|_| serde_json::json!({})),
        "market": row.market,
        "logs": logs.iter().map(|l| serde_json::json!({
            "id": l.id,
            "application_id": l.application_id,
            "reviewer": l.reviewer,
            "action": l.action,
            "reason": l.reason,
            "created_at": l.created_at,
        })).collect::<Vec<_>>(),
    })
}

fn row_from_app(app: ListingApplication) -> ApplicationRow {
    ApplicationRow {
        id: app.id,
        applicant: app.applicant,
        family: app.family,
        title: app.title,
        tags: app.tags,
        event: app.event,
        description: app.description,
        topic: app.topic,
        tag: app.tag,
        blocked_regions: app.blocked_regions,
        dup_key: app.dup_key,
        status: app.status,
        reviewer: app.reviewer,
        reason: app.reason,
        created_at: app.created_at,
        reviewed_at: app.reviewed_at,
        compose_json: app.compose_json,
        market: app.market,
    }
}

fn app_from_row(row: &ApplicationRow) -> ListingApplication {
    ListingApplication {
        id: row.id,
        applicant: row.applicant.clone(),
        family: row.family,
        title: row.title.clone(),
        tags: row.tags.clone(),
        event: row.event.clone(),
        description: row.description.clone(),
        topic: row.topic.clone(),
        tag: row.tag.clone(),
        blocked_regions: row.blocked_regions.clone(),
        dup_key: row.dup_key.clone(),
        status: row.status,
        reviewer: row.reviewer.clone(),
        reason: row.reason.clone(),
        created_at: row.created_at,
        reviewed_at: row.reviewed_at,
        compose_json: row.compose_json.clone(),
        market: row.market.clone(),
    }
}

#[derive(Deserialize)]
struct ApplicationPageQ {
    #[serde(default)]
    status: Option<u8>,
    #[serde(default)]
    applicant: Option<String>,
    #[serde(default = "page_one")]
    page: u32,
    #[serde(default = "limit_default")]
    limit: u32,
}

#[derive(Deserialize)]
struct ApplicationBody {
    applicant: String,
    #[serde(default)]
    family: u8,
    title: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    event: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    topic: String,
    #[serde(default)]
    tag: String,
    #[serde(default)]
    blocked_regions: Vec<String>,
    #[serde(default)]
    compose: serde_json::Value,
}

#[derive(Deserialize)]
struct ReviewBody {
    id: i64,
    reviewer: String,
    action: String,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    market: String,
}

async fn persist_app(
    st: &AppState,
    mut row: ListingApplication,
    action: &str,
    actor: &str,
    reason: &str,
) -> Result<ApplicationRow, StatusCode> {
    let log = ReviewLog {
        id: 0,
        application_id: row.id,
        reviewer: actor.to_string(),
        action: action.into(),
        reason: reason.to_string(),
        created_at: 0,
    };
    let (saved, log) = st
        .catalog
        .record_application(&row, &log)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    row = saved;
    let stored = st.store.push_application(row_from_app(row));
    st.store.push_review_log(ReviewLogRow {
        id: log.id,
        application_id: stored.id,
        reviewer: log.reviewer,
        action: log.action,
        reason: log.reason,
        created_at: log.created_at,
    });
    Ok(stored)
}

async fn put_application(State(st): State<AppState>, Json(body): Json<ApplicationBody>) -> Result<impl IntoResponse, StatusCode> {
    let compose_json = if body.compose.is_null() {
        String::new()
    } else {
        body.compose.to_string()
    };
    let mut app = ListingApplication::submit(
        body.applicant,
        body.family,
        body.title,
        body.tags,
        body.event,
        body.description,
        body.topic,
        body.tag,
        body.blocked_regions,
        compose_json,
    )
    .map_err(|_| StatusCode::BAD_REQUEST)?;
    if let Some(hit) = st.store.application_by_dup(&app.dup_key) {
        app.status = APP_DUPLICATE;
        app.reason = format!("duplicate of application {}", hit.id);
        let stored = persist_app(&st, app, "auto_duplicate", "system", &format!("dup {}", hit.id)).await?;
        return Ok((
            StatusCode::CONFLICT,
            Json(application_json(&stored, &st.store.review_logs_of(stored.id))),
        ));
    }
    let stored = persist_app(&st, app, "submit", "system", "queued for review").await?;
    Ok((
        StatusCode::OK,
        Json(application_json(&stored, &st.store.review_logs_of(stored.id))),
    ))
}

async fn list_applications(State(st): State<AppState>, Query(q): Query<ApplicationPageQ>) -> impl IntoResponse {
    let (total, items) = st.store.list_applications(q.status, q.applicant.as_deref(), q.page, q.limit);
    let limit = q.limit.clamp(1, 100);
    let page = q.page.max(1);
    let pages = if total == 0 {
        1
    } else {
        ((total + u64::from(limit) - 1) / u64::from(limit)) as u32
    };
    Json(serde_json::json!({
        "page": page,
        "limit": limit,
        "total": total,
        "pages": pages,
        "items": items.iter().map(|r| application_json(r, &st.store.review_logs_of(r.id))).collect::<Vec<_>>(),
    }))
}

async fn application_one(State(st): State<AppState>, Path(id): Path<i64>) -> Result<impl IntoResponse, StatusCode> {
    let row = st.store.application_of(id).ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(application_json(&row, &st.store.review_logs_of(row.id))))
}

async fn review_application(State(st): State<AppState>, Json(body): Json<ReviewBody>) -> Result<impl IntoResponse, StatusCode> {
    if !is_reviewer(&body.reviewer) {
        return Err(StatusCode::FORBIDDEN);
    }
    let mut row = st.store.application_of(body.id).ok_or(StatusCode::NOT_FOUND)?;
    let action = body.action.trim().to_ascii_lowercase();
    if action == "opened" {
        if row.status != APP_APPROVED {
            return Err(StatusCode::CONFLICT);
        }
        if !row.market.trim().is_empty() {
            return Err(StatusCode::CONFLICT);
        }
        if !valid_pubkey(body.market.trim()) {
            return Err(StatusCode::BAD_REQUEST);
        }
        row.market = body.market.trim().to_string();
        row.reviewer = body.reviewer.clone();
        let stored = persist_app(&st, app_from_row(&row), "opened", &body.reviewer, body.market.trim()).await?;
        return Ok(Json(application_json(&stored, &st.store.review_logs_of(stored.id))));
    }
    if row.status != APP_PENDING {
        return Err(StatusCode::CONFLICT);
    }
    let (status, action) = match action.as_str() {
        "approve" => (APP_APPROVED, "approve"),
        "reject" => (APP_REJECTED, "reject"),
        "duplicate" => (APP_DUPLICATE, "duplicate"),
        _ => return Err(StatusCode::BAD_REQUEST),
    };
    if action != "approve" && body.reason.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    row.status = status;
    row.reviewer = body.reviewer.clone();
    row.reason = body.reason.trim().to_string();
    row.reviewed_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let stored = persist_app(&st, app_from_row(&row), action, &body.reviewer, &row.reason).await?;
    Ok(Json(application_json(&stored, &st.store.review_logs_of(stored.id))))
}

async fn listing_one(State(st): State<AppState>, Path(market): Path<String>) -> Result<impl IntoResponse, StatusCode> {
    let listing = st.store.listing_of(&market).ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(serde_json::json!({
        "market": market,
        "title": listing.title,
        "tags": listing.resolved_tags(),
        "category": listing.resolved_tags().first().cloned().unwrap_or_default(),
        "topic": listing.topic,
        "tag": listing.tag,
        "description": listing.description,
        "event": listing.event,
    })))
}

async fn owner_risk(State(st): State<AppState>, Path(owner): Path<String>) -> impl IntoResponse {
    let quotes = st.store.quotes_of(&owner);
    let total = quotes.len();
    let items: Vec<_> = quotes
        .into_iter()
        .map(|q| {
            let weight_sum = st.store.weight_sum(&q.market);
            let (expected_h, attachment, board_phase) = match st.store.get(&q.market) {
                Some(m) => {
                    let layers = st.store.layers_of(&q.market);
                    let att = layers
                        .iter()
                        .find(|l| l.layer_id == q.layer_id)
                        .map(|l| l.attachment)
                        .unwrap_or(0);
                    let l = if m.board_phase >= 1 {
                        m.liability
                    } else {
                        m.book().l_max_usdc()
                    };
                    let h = usdc(layer_loss(
                        Q64::from_int(l as i64),
                        Q64::from_int(att as i64),
                        Q64::from_int(q.filled as i64),
                    ));
                    (h, att, m.board_phase)
                }
                None => (0, 0, 0),
            };
            let listing = st.store.listing_of(&q.market).unwrap_or_default();
            let family = st.store.get(&q.market).map(|m| m.family).unwrap_or(0);
            serde_json::json!({
                "quote": q.quote,
                "market": q.market,
                "title": display_title(&listing, family),
                "category": listing.category,
                "layer": q.layer_id,
                "capacity": q.capacity,
                "filled": q.filled,
                "d_i": q.filled,
                "premium": q.premium,
                "premium_owed": q.premium_owed,
                "profit_share_bps": q.profit_share_bps,
                "cancelled": q.cancelled,
                "expected_h": expected_h,
                "attachment": attachment,
                "weight_sum": weight_sum,
                "board_phase": board_phase,
            })
        })
        .collect();
    Json(serde_json::json!({
        "owner": owner,
        "page": 1,
        "limit": 20,
        "total": total,
        "pages": 1,
        "items": items,
    }))
}

fn empty_vault(owner: &str, wallet_usdc: u64) -> serde_json::Value {
    serde_json::json!({
        "owner": owner,
        "exists": false,
        "available": 0,
        "reserved": 0,
        "free": 0,
        "wallet_usdc": wallet_usdc,
        "mint": "Circle SPL USDC",
        "source": "l1",
    })
}

async fn owner_vault(Path(owner): Path<String>) -> impl IntoResponse {
    Json(match load_owner_vault(&owner).await {
        Ok(v) => v,
        Err(_) => empty_vault(&owner, 0),
    })
}

async fn load_owner_vault(owner: &str) -> Result<serde_json::Value, ()> {
    let pk: Pubkey = owner.parse().map_err(|_| ())?;
    let url = std::env::var("RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8899".into());
    let rpc = RpcClient::new_with_commitment(url, CommitmentConfig::confirmed());
    let vault_pda = user_vault(&pk);
    let ata_pda = ata(&pk);
    let vault_acc = rpc.get_account(&vault_pda).await;
    let wallet = rpc.get_token_account_balance(&ata_pda).await.ok();
    let wallet_usdc = wallet
        .and_then(|t| t.amount.parse::<u64>().ok())
        .unwrap_or(0);
    let Ok(acc) = vault_acc else {
        return Ok(empty_vault(owner, wallet_usdc));
    };
    let Ok(u) = decode_user_vault(&acc.data) else {
        return Ok(empty_vault(owner, wallet_usdc));
    };
    let free = u.available.saturating_sub(u.reserved);
    Ok(serde_json::json!({
        "owner": owner,
        "exists": true,
        "available": u.available,
        "reserved": u.reserved,
        "free": free,
        "wallet_usdc": wallet_usdc,
        "mint": "Circle SPL USDC",
        "source": "l1",
    }))
}

async fn ops_status(State(st): State<AppState>) -> impl IntoResponse {
    let rows = st.store.list();
    let n = rows.len();
    let covered = rows.iter().filter(|m| m.book().coverage().raw() > 0).count();
    let c_r: u64 = rows.iter().map(|m| m.c_r).sum();
    Json(serde_json::json!({
        "slot": st.store.slot(),
        "boards": n,
        "boards_with_coverage": covered,
        "c_r_total": c_r,
        "c_p_pool": st.store.pool_available(),
        "vault_mint": "Circle SPL USDC",
        "keeper_heartbeat_slot": st.store.slot(),
        "index_lag_slots": 0,
        "pg": st.catalog.has_pg(),
        "read_only": true,
        "withdraw_disabled": true,
    }))
}

async fn pool_one(State(st): State<AppState>) -> impl IntoResponse {
    let boards: Vec<_> = st
        .store
        .list()
        .into_iter()
        .map(|m| {
            let listing = st.store.listing_of(&m.market).unwrap_or_default();
            serde_json::json!({
                "market": m.market,
                "title": display_title(&listing, m.family),
                "category": listing.category,
                "c_p_board": m.c_p_board,
                "c_p_alloc": m.c_p_alloc,
                "c_m": m.c_m,
                "c_r": m.c_r,
            })
        })
        .collect();
    Json(serde_json::json!({
        "c_p_pool": st.store.pool_available(),
        "boards": boards,
    }))
}

#[derive(Deserialize)]
struct PriorQ {
    #[serde(default = "prior_family_default")]
    pub family: u8,
    #[serde(default = "prior_n_default")]
    pub n: u16,
    pub x_min: Option<i64>,
    pub x_max: Option<i64>,
    pub mu: Option<i64>,
    pub sigma: Option<i64>,
    pub lambda_home: Option<i64>,
    pub lambda_away: Option<i64>,
    #[serde(default)]
    pub milli: bool,
    #[serde(default)]
    pub layout: u8,
    pub k: Option<u16>,
    pub bins: Option<u16>,
    pub top_n: Option<u8>,
}

fn prior_family_default() -> u8 {
    1
}

fn prior_n_default() -> u16 {
    256
}

fn q_prior(v: i64, milli: bool) -> Q64 {
    if milli {
        Q64::from_ratio(v, 1000)
    } else {
        Q64::from_int(v)
    }
}

fn q_as_f64(q: Q64) -> f64 {
    q.raw() as f64 / ((1u128 << 64) as f64)
}

fn mass_in(xs: &[(f64, u64)], lo: Option<f64>, hi: Option<f64>) -> u64 {
    xs.iter()
        .filter(|(x, _)| lo.map(|a| *x >= a).unwrap_or(true) && hi.map(|b| *x < b).unwrap_or(true))
        .map(|(_, p)| *p)
        .sum()
}

fn interval_cuts(family: u8, xmin: f64, xmax: f64, mu: f64, sigma: f64, xs: &[(f64, u64)]) -> Vec<serde_json::Value> {
    let cuts: Vec<(String, Option<f64>, Option<f64>)> = if family == 1 && xmin >= -20.0 && xmax >= 8.0 {
        vec![
            ("X < 2".into(), None, Some(2.0)),
            ("2 ≤ X < 2.5".into(), Some(2.0), Some(2.5)),
            ("2.5 ≤ X < 3".into(), Some(2.5), Some(3.0)),
            ("X ≥ 3".into(), Some(3.0), None),
        ]
    } else if family == 1 {
        let a = (mu - sigma).clamp(xmin, xmax);
        let b = mu.clamp(xmin, xmax);
        let c = (mu + sigma).clamp(xmin, xmax);
        vec![
            (format!("X < {a:.2}"), None, Some(a)),
            (format!("{a:.2} ≤ X < {b:.2}"), Some(a), Some(b)),
            (format!("{b:.2} ≤ X < {c:.2}"), Some(b), Some(c)),
            (format!("X ≥ {c:.2}"), Some(c), None),
        ]
    } else if family == 2 && xmax > 1_000.0 {
        vec![
            ("X < 80k".into(), None, Some(80_000.0)),
            ("80k ≤ X < 100k".into(), Some(80_000.0), Some(100_000.0)),
            ("X ≥ 100k".into(), Some(100_000.0), None),
            ("X ≥ 120k".into(), Some(120_000.0), None),
        ]
    } else {
        let a = xmin + (xmax - xmin) * 0.25;
        let b = xmin + (xmax - xmin) * 0.50;
        let c = xmin + (xmax - xmin) * 0.75;
        return vec![
            serde_json::json!({"label": format!("X < {a:.2}"), "p_bps": mass_in(xs, None, Some(a))}),
            serde_json::json!({"label": format!("{a:.2} ≤ X < {b:.2}"), "p_bps": mass_in(xs, Some(a), Some(b))}),
            serde_json::json!({"label": format!("{b:.2} ≤ X < {c:.2}"), "p_bps": mass_in(xs, Some(b), Some(c))}),
            serde_json::json!({"label": format!("X ≥ {c:.2}"), "p_bps": mass_in(xs, Some(c), None)}),
        ];
    };
    cuts.into_iter()
        .map(|(label, lo, hi)| serde_json::json!({"label": label, "p_bps": mass_in(xs, lo, hi)}))
        .collect()
}

fn skellam_lines(p0: &[Q64]) -> Vec<serde_json::Value> {
    let n = 11usize;
    if p0.len() != n * n {
        return vec![];
    }
    let mut home = 0u64;
    let mut draw = 0u64;
    let mut away = 0u64;
    let mut over = 0u64;
    for i in 0..n {
        for j in 0..n {
            let p = q_bps(p0[i * n + j]);
            if i > j {
                home += p;
            } else if i == j {
                draw += p;
            } else {
                away += p;
            }
            if i + j >= 3 {
                over += p;
            }
        }
    }
    vec![
        serde_json::json!({"label": "Home", "p_bps": home}),
        serde_json::json!({"label": "Draw", "p_bps": draw}),
        serde_json::json!({"label": "Away", "p_bps": away}),
        serde_json::json!({"label": "Over 2.5", "p_bps": over}),
    ]
}

async fn prior_one(Query(q): Query<PriorQ>) -> Result<impl IntoResponse, StatusCode> {
    let milli = q.milli;
    let n = q.n.clamp(2, client::market::state::MAX_N) as usize;
    let mut warnings: Vec<String> = Vec::new();
    let (xmin_f, xmax_f, mu_f, sigma_f) = match q.family {
        1 => {
            let xmin = q.x_min.unwrap_or(if milli { -2_000 } else { 0 });
            let xmax = q.x_max.unwrap_or(if milli { 12_000 } else { n as i64 });
            let mu = q.mu.unwrap_or(if milli { 2_400 } else { n as i64 / 2 });
            let sigma = q.sigma.unwrap_or(if milli { 350 } else { 2 });
            (
                q_as_f64(q_prior(xmin, milli)),
                q_as_f64(q_prior(xmax, milli)),
                q_as_f64(q_prior(mu, milli)),
                q_as_f64(q_prior(sigma, milli)),
            )
        }
        2 => {
            let xmin = q.x_min.unwrap_or(if milli { 10_000_000 } else { 1 });
            let xmax = q.x_max.unwrap_or(if milli { 250_000_000 } else { 100 });
            let mu = q.mu.unwrap_or(if milli { 11_082 } else { 4 });
            let sigma = q.sigma.unwrap_or(if milli { 250 } else { 1 });
            (
                q_as_f64(q_prior(xmin, milli)),
                q_as_f64(q_prior(xmax, milli)),
                q_as_f64(q_prior(mu, milli)),
                q_as_f64(q_prior(sigma, milli)),
            )
        }
        _ => (0.0, n as f64, 0.0, 0.0),
    };
    let p0 = match q.family {
        0 => math::prior::independent_poisson_2d(
            10,
            q_prior(q.lambda_home.unwrap_or(if milli { 1_400 } else { 1 }), milli),
            q_prior(q.lambda_away.unwrap_or(if milli { 1_100 } else { 1 }), milli),
        ),
        1 => {
            let xmin = q_prior(q.x_min.unwrap_or(if milli { -2_000 } else { 0 }), milli);
            let xmax = q_prior(q.x_max.unwrap_or(if milli { 12_000 } else { n as i64 }), milli);
            let mu = q_prior(q.mu.unwrap_or(if milli { 2_400 } else { n as i64 / 2 }), milli);
            let sigma = q_prior(q.sigma.unwrap_or(if milli { 350 } else { 2 }), milli);
            if xmax <= xmin || sigma.raw() <= 0 {
                return Err(StatusCode::BAD_REQUEST);
            }
            math::prior::truncated_normal(n, xmin, xmax, mu, sigma)
        }
        2 => {
            let xmin = q_prior(q.x_min.unwrap_or(if milli { 10_000_000 } else { 1 }), milli);
            let xmax = q_prior(q.x_max.unwrap_or(if milli { 250_000_000 } else { 100 }), milli);
            let mu = q_prior(q.mu.unwrap_or(if milli { 11_082 } else { 4 }), milli);
            let sigma = q_prior(q.sigma.unwrap_or(if milli { 250 } else { 1 }), milli);
            if xmin.raw() <= 0 || xmax <= xmin || sigma.raw() <= 0 {
                return Err(StatusCode::BAD_REQUEST);
            }
            math::prior::truncated_lognormal(n, xmin, xmax, mu, sigma)
        }
        3 => {
            let layout = q.layout;
            let k = q.k.unwrap_or(2).clamp(2, 16) as usize;
            match layout {
                2 => {
                    let bins = q.bins.unwrap_or(0) as usize;
                    math::prior::simplex_cell_count(k as u32, bins as u32).ok_or(StatusCode::BAD_REQUEST)?;
                    math::prior::vote_share_simplex(k, bins, &vec![Q64::ONE; k])
                }
                1 => {
                    let top = q.top_n.unwrap_or(1) as u32;
                    let atoms = math::prior::binom(k as u32, top).ok_or(StatusCode::BAD_REQUEST)? as usize;
                    math::uniform_prior(atoms)
                }
                _ => math::prior::dirichlet(&vec![Q64::ONE; n.max(2)]),
            }
        }
        4 => math::prior::binary(Q64::ONE, Q64::ONE),
        _ => return Err(StatusCode::BAD_REQUEST),
    };
    if q.family == 1 || q.family == 2 {
        if mu_f < xmin_f || mu_f > xmax_f {
            warnings.push("μ is outside Ω — most P0 mass sits on the nearer edge".into());
        }
        let span = xmax_f - xmin_f;
        let dx = span / ((n.max(2) - 1) as f64);
        if sigma_f > 0.0 && sigma_f < dx {
            warnings.push("σ is smaller than one grid step — P0 spikes on 1–2 cells".into());
        }
        if sigma_f > span / 2.0 {
            warnings.push("σ is wide vs Ω — truncated P0 approaches uniform".into());
        }
        if q.family == 1 && (mu_f - 3.0 * sigma_f < xmin_f || mu_f + 3.0 * sigma_f > xmax_f) {
            warnings.push("Ω cuts the ~3σ tails; masses are renormalized (θ=0 prices equal this P0)".into());
        }
        if q.family == 1 && n < 32 {
            warnings.push("CPI n_grid 256 is recommended; coarse n lumps interval prices".into());
        }
        if q.family == 2 && xmin_f <= 0.0 {
            warnings.push("lognormal Ω must be > 0".into());
        }
    }
    if q.family == 0 {
        let lh = q_as_f64(q_prior(q.lambda_home.unwrap_or(if milli { 1_400 } else { 1 }), milli));
        let la = q_as_f64(q_prior(q.lambda_away.unwrap_or(if milli { 1_100 } else { 1 }), milli));
        if lh > 6.0 || la > 6.0 {
            warnings.push("λ is high vs k_max=10 — overflow 10+ will hold real mass".into());
        }
    }
    let grid_n = if q.family == 0 { 121 } else { p0.len() };
    let bps = q_bps_renorm(&p0);
    let xs: Vec<(f64, u64)> = p0
        .iter()
        .enumerate()
        .map(|(i, _)| {
            let x = if q.family == 1 || q.family == 2 {
                xmin_f + (xmax_f - xmin_f) * (i as f64) / ((n.max(2) - 1) as f64)
            } else {
                i as f64
            };
            (x, bps[i])
        })
        .collect();
    let cells: Vec<_> = xs
        .iter()
        .enumerate()
        .map(|(i, (x, p))| serde_json::json!({"i": i, "x": x, "p_bps": p}))
        .collect();
    let peak = p0
        .iter()
        .enumerate()
        .max_by_key(|(_, p)| p.raw())
        .map(|(i, _)| i)
        .unwrap_or(0);
    let peak_x = xs.get(peak).map(|(x, _)| *x).unwrap_or(0.0);
    let intervals = if q.family == 1 || q.family == 2 {
        interval_cuts(q.family, xmin_f, xmax_f, mu_f, sigma_f, &xs)
    } else {
        vec![]
    };
    let lines = if q.family == 0 {
        skellam_lines(&p0)
    } else {
        vec![]
    };
    let units = match q.family {
        1 => "percentage points",
        2 => "price units",
        0 => "goals",
        _ => "atoms",
    };
    Ok(Json(serde_json::json!({
        "family": q.family,
        "n": grid_n,
        "peak": peak,
        "peak_x": peak_x,
        "omega": { "x_min": xmin_f, "x_max": xmax_f },
        "mu": mu_f,
        "sigma": sigma_f,
        "units": units,
        "cells": cells,
        "intervals": intervals,
        "lines": lines,
        "warnings": warnings,
    })))
}

async fn committee_one(State(st): State<AppState>) -> Result<impl IntoResponse, StatusCode> {
    let row = st.store.committee().ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(serde_json::json!({
        "authority": row.authority,
        "members": row.members,
        "m": row.m,
        "n": row.n,
        "epoch": row.epoch,
        "shared": true,
    })))
}

async fn compose_tracked(State(st): State<AppState>, Json(mut body): Json<ComposeBody>) -> Result<impl IntoResponse, StatusCode> {
    require_approved_create(&st, &body)?;
    require_trading_open(&st, &body)?;
    hydrate_payout_fill(&st, &mut body);
    let out = compose_out(&body)?;
    if let Some(fill) = remember_compose_fill(&st, &body) {
        st.catalog
            .put_fill(&fill)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    Ok(Json(out))
}

pub fn router(store: Arc<MemoryStore>) -> Router {
    router_with_pool(store, None)
}

pub fn router_with_pool(store: Arc<MemoryStore>, pool: Option<sqlx::PgPool>) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/markets", get(list_markets))
        .route("/v1/markets/{market}/quote", get(quote_one))
        .route("/v1/markets/{market}/book", get(book_one))
        .route("/v1/markets/{market}/pdf", get(pdf_one))
        .route("/v1/markets/{market}/preview", get(preview_one))
        .route("/v1/markets/{market}/info", get(info_one))
        .route("/v1/markets/{market}/layers", get(layers_one))
        .route("/v1/markets/{market}/resolution", get(resolution_one))
        .route("/v1/markets/{market}/comments", get(list_comments).post(put_comment))
        .route("/v1/markets/{market}/ws", get(ws_market))
        .route("/v1/owners/{owner}/positions", get(owner_positions))
        .route("/v1/owners/{owner}/vault", get(owner_vault))
        .route("/v1/owners/{owner}/risk", get(owner_risk))
        .route("/v1/auctions", get(list_auctions))
        .route("/v1/listings", post(put_listing))
        .route("/v1/listings/applications", get(list_applications).post(put_application))
        .route("/v1/listings/applications/{id}", get(application_one))
        .route("/v1/review", post(review_application))
        .route("/v1/listings/{market}", get(listing_one))
        .route("/v1/tags", get(list_tags).post(put_tag))
        .route("/v1/tags/{name}", delete(delete_tag))
        .route("/v1/tickets", post(put_ticket))
        .route("/v1/ops/status", get(ops_status))
        .route("/v1/pool", get(pool_one))
        .route("/v1/prior", get(prior_one))
        .route("/v1/committee", get(committee_one))
        .route("/v1/compose", post(compose_tracked))
        .layer(CorsLayer::permissive())
        .with_state(AppState {
            store,
            catalog: CatalogService::new(pool),
        })
}
