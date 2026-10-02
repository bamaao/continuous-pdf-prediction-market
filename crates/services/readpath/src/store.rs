//! Projections of on-chain θ / E / capital. Postgres is the restart store; memory is the hot cache.

use crate::domain::{normalize_tag, CatalogTag, DomainError, DEFAULT_CATALOG_TAGS};
use anyhow::Result;
use quote::Book;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

#[derive(Clone, Debug, Serialize)]
pub struct CatalogTagRow {
    pub name: String,
    pub used: u64,
}

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
    pub fee_bps: u16,
    #[serde(default)]
    pub fee_timing: u8,
    pub slot: u64,
    /// Distinct position owners with q > 0.
    pub traders: u64,
    /// Open position accounts (one per owner × set).
    pub tickets: u64,
    /// Sum of `position.cost_paid` (USDC actually paid into the book).
    pub stake_usdc: u64,
    pub board_phase: u8,
    pub rho_raw: i128,
    pub settle_cell: u16,
    pub liability: u64,
    pub c_p_board: u64,
    pub c_p_alloc: u64,
    #[serde(default)]
    pub close_ts: i64,
    #[serde(default)]
    pub risk_lock_ts: i64,
    #[serde(default)]
    pub report_window_secs: i64,
    /// FamilyExtra.a — Gaussian/lognormal $x_{\min}$ (Q64 raw).
    #[serde(default)]
    pub extra_a: i128,
    /// FamilyExtra.b — Gaussian/lognormal $x_{\max}$ (Q64 raw).
    #[serde(default)]
    pub extra_b: i128,
    /// FamilyExtra.u2 — Skellam $k_{\max}$.
    #[serde(default)]
    pub extra_u2: u8,
    /// True while MagicBlock owns the market PDA (FR-TRD-01 fills go to ER).
    #[serde(default)]
    pub delegated: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PositionRow {
    pub position: String,
    pub owner: String,
    pub market: String,
    pub set_hash: String,
    pub q_raw: i128,
    pub shares: u64,
    pub cost_paid: u64,
    pub claimed: bool,
    pub paid_usdc: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuoteRow {
    pub quote: String,
    pub market: String,
    pub lp: String,
    pub layer_id: u8,
    pub capacity: u64,
    pub filled: u64,
    pub premium: u64,
    pub premium_owed: u64,
    pub profit_share_bps: u16,
    pub cancelled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LayerRow {
    pub layer: String,
    pub market: String,
    pub layer_id: u8,
    pub attachment: u64,
    pub thickness: u64,
    pub filled: u64,
    pub quote_count: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutcomeSnap {
    pub kind: u8,
    pub a: String,
    pub b: String,
    pub label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResolutionRow {
    pub market: String,
    pub record: String,
    pub phase: u8,
    pub family: u8,
    pub m: u8,
    pub n: u8,
    pub extensions: u8,
    pub votes_proposal: u8,
    pub votes_challenge: u8,
    pub refunds_due: bool,
    pub early_resolve: bool,
    pub close_ts: i64,
    pub report_deadline: i64,
    pub challenge_end: i64,
    pub vote_end: i64,
    pub report_window_secs: i64,
    pub challenge_secs: i64,
    pub proposer: String,
    pub challenger: String,
    pub authorized_reporter: String,
    pub members: Vec<String>,
    pub proposed: OutcomeSnap,
    pub challenged: OutcomeSnap,
    pub final_outcome: OutcomeSnap,
    pub evidence_hash: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CommitteeSnap {
    pub authority: String,
    pub members: Vec<String>,
    pub m: u8,
    pub n: u8,
    pub epoch: u32,
}

impl MarketProj {
    pub fn book(&self) -> Book {
        let mut book = Book::from_grid(
            self.beta,
            &self.p0,
            &self.theta,
            &self.exposure,
            self.trading_revenue,
            self.premium_payable,
            self.c_m,
            self.c_r,
            self.slot,
        );
        book.c_p = self.c_p_alloc;
        book
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FillMeta {
    pub owner: String,
    pub market: String,
    #[serde(default)]
    pub set_hash: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub mask: String,
    pub skellam_kind: Option<u8>,
    pub a: Option<i64>,
    pub b: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommentRow {
    pub id: i64,
    pub market: String,
    pub author: String,
    pub body: String,
    pub created_at: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ListingMeta {
    pub title: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub category: String,
    pub topic: String,
    pub tag: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub event: String,
    #[serde(default)]
    pub blocked_regions: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApplicationRow {
    pub id: i64,
    pub applicant: String,
    pub family: u8,
    pub title: String,
    pub tags: Vec<String>,
    pub event: String,
    pub description: String,
    pub topic: String,
    pub tag: String,
    pub blocked_regions: Vec<String>,
    pub dup_key: String,
    pub status: u8,
    pub reviewer: String,
    pub reason: String,
    pub created_at: i64,
    pub reviewed_at: i64,
    pub compose_json: String,
    pub market: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReviewLogRow {
    pub id: i64,
    pub application_id: i64,
    pub reviewer: String,
    pub action: String,
    pub reason: String,
    pub created_at: i64,
}

impl ListingMeta {
    pub fn resolved_tags(&self) -> Vec<String> {
        if !self.tags.is_empty() {
            return self.tags.clone();
        }
        if self.category.trim().is_empty() {
            Vec::new()
        } else {
            vec![self.category.trim().to_string()]
        }
    }

    pub fn tags_label(&self) -> String {
        let tags = self.resolved_tags();
        if tags.is_empty() {
            String::new()
        } else {
            tags.join(" · ")
        }
    }
}

#[derive(Clone, Default)]
pub struct MemoryStore {
    inner: Arc<RwLock<Inner>>,
}

#[derive(Default)]
struct Inner {
    markets: HashMap<String, MarketProj>,
    listings: HashMap<String, ListingMeta>,
    listings_path: Option<PathBuf>,
    catalog_tags: HashSet<String>,
    fills: HashMap<String, FillMeta>,
    fills_path: Option<PathBuf>,
    comments: Vec<CommentRow>,
    next_comment_id: i64,
    applications: Vec<ApplicationRow>,
    review_logs: Vec<ReviewLogRow>,
    next_application_id: i64,
    next_review_log_id: i64,
    positions: Vec<PositionRow>,
    quotes: Vec<QuoteRow>,
    layers: Vec<LayerRow>,
    resolutions: Vec<ResolutionRow>,
    committee: Option<CommitteeSnap>,
    pool_available: u64,
    slot: u64,
    ledger_genesis: Option<String>,
}

impl MemoryStore {
    pub fn new() -> Self {
        let store = Self::default();
        let _ = store.ensure_catalog_tags(DEFAULT_CATALOG_TAGS.iter().copied());
        store
    }

    pub fn upsert(&self, mut row: MarketProj) {
        let mut g = self.inner.write().expect("store");
        g.slot = g.slot.max(row.slot);
        if let Some(old) = g.markets.get(&row.market) {
            if row.c_p_board == 0 {
                row.c_p_board = old.c_p_board;
            }
            if row.c_p_alloc == 0 {
                row.c_p_alloc = old.c_p_alloc;
            }
        }
        g.markets.insert(row.market.clone(), row);
    }

    pub fn get(&self, market: &str) -> Option<MarketProj> {
        self.inner.read().expect("store").markets.get(market).cloned()
    }

    pub fn market_ids(&self) -> Vec<String> {
        self.inner.read().expect("store").markets.keys().cloned().collect()
    }

    pub fn persist_listings(&self, path: impl Into<PathBuf>) {
        let path = path.into();
        if let Ok(raw) = std::fs::read_to_string(&path) {
            if let Ok(map) = serde_json::from_str::<HashMap<String, ListingMeta>>(&raw) {
                let mut g = self.inner.write().expect("store");
                for (k, v) in map {
                    if !v.title.trim().is_empty() {
                        g.listings.entry(k).or_insert(v);
                    }
                }
            }
        }
        self.inner.write().expect("store").listings_path = Some(path);
    }

    fn flush_listings(g: &Inner) {
        let Some(path) = g.listings_path.as_ref() else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(raw) = serde_json::to_string_pretty(&g.listings) {
            let _ = std::fs::write(path, raw);
        }
    }

    pub fn set_listing(&self, market: &str, meta: ListingMeta) {
        let tags = meta.resolved_tags();
        let mut g = self.inner.write().expect("store");
        g.listings.insert(market.to_string(), meta);
        for t in tags {
            g.catalog_tags.insert(t);
        }
        Self::flush_listings(&g);
    }

    pub fn ensure_catalog_tags<I, S>(&self, names: I) -> Result<Vec<String>, DomainError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut out = Vec::new();
        let mut g = self.inner.write().expect("store");
        for raw in names {
            let tag = CatalogTag::new(raw)?;
            g.catalog_tags.insert(tag.name.clone());
            out.push(tag.name);
        }
        Ok(out)
    }

    pub fn add_catalog_tag(&self, raw: &str) -> Result<String, DomainError> {
        let tag = CatalogTag::new(raw)?;
        self.inner.write().expect("store").catalog_tags.insert(tag.name.clone());
        Ok(tag.name)
    }

    pub fn delete_catalog_tag(&self, raw: &str) -> Result<String, DomainError> {
        let name = normalize_tag(raw)?;
        if name.is_empty() {
            return Err(DomainError::Invalid("tag required"));
        }
        let used = self.tag_used(&name);
        if used > 0 {
            return Err(DomainError::Conflict("tag in use"));
        }
        let mut g = self.inner.write().expect("store");
        if !g.catalog_tags.remove(&name) {
            return Err(DomainError::Invalid("unknown tag"));
        }
        Ok(name)
    }

    pub fn tag_used(&self, name: &str) -> u64 {
        let needle = name.trim();
        self.inner
            .read()
            .expect("store")
            .listings
            .values()
            .filter(|l| l.resolved_tags().iter().any(|t| t.eq_ignore_ascii_case(needle)))
            .count() as u64
    }

    pub fn catalog_tags(&self) -> Vec<CatalogTagRow> {
        let g = self.inner.read().expect("store");
        let mut rows: Vec<CatalogTagRow> = g
            .catalog_tags
            .iter()
            .map(|name| CatalogTagRow {
                name: name.clone(),
                used: g
                    .listings
                    .values()
                    .filter(|l| l.resolved_tags().iter().any(|t| t.eq_ignore_ascii_case(name)))
                    .count() as u64,
            })
            .collect();
        rows.sort_by(|a, b| a.name.cmp(&b.name));
        rows
    }

    pub fn listing_of(&self, market: &str) -> Option<ListingMeta> {
        self.inner.read().expect("store").listings.get(market).cloned()
    }

    pub fn listings(&self) -> Vec<(String, ListingMeta)> {
        self.inner
            .read()
            .expect("store")
            .listings
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    pub fn fills(&self) -> Vec<FillMeta> {
        self.inner.read().expect("store").fills.values().cloned().collect()
    }

    fn fill_key(owner: &str, market: &str, set_hash: &str) -> String {
        format!(
            "{}|{}|{}",
            owner,
            market,
            set_hash.trim_start_matches("0x").to_ascii_lowercase()
        )
    }

    pub fn persist_fills(&self, path: impl Into<PathBuf>) {
        let path = path.into();
        if let Ok(raw) = std::fs::read_to_string(&path) {
            if let Ok(map) = serde_json::from_str::<HashMap<String, FillMeta>>(&raw) {
                let mut g = self.inner.write().expect("store");
                for (k, v) in map {
                    if !v.owner.is_empty() && !v.market.is_empty() {
                        g.fills.entry(k).or_insert(v);
                    }
                }
            }
        }
        self.inner.write().expect("store").fills_path = Some(path);
    }

    fn flush_fills(g: &Inner) {
        let Some(path) = g.fills_path.as_ref() else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(raw) = serde_json::to_string_pretty(&g.fills) {
            let _ = std::fs::write(path, raw);
        }
    }

    pub fn set_fill(&self, meta: FillMeta) {
        let key = Self::fill_key(&meta.owner, &meta.market, &meta.set_hash);
        let mut g = self.inner.write().expect("store");
        g.fills.insert(key, meta);
        Self::flush_fills(&g);
    }

    pub fn fill_of(&self, owner: &str, market: &str, set_hash: &str) -> Option<FillMeta> {
        let key = Self::fill_key(owner, market, set_hash);
        self.inner.read().expect("store").fills.get(&key).cloned()
    }

    pub fn comments(&self) -> Vec<CommentRow> {
        self.inner.read().expect("store").comments.clone()
    }

    pub fn push_comment(&self, mut row: CommentRow) -> CommentRow {
        let mut g = self.inner.write().expect("store");
        if g.next_comment_id < 1 {
            g.next_comment_id = 1;
        }
        if row.id <= 0 {
            row.id = g.next_comment_id;
            g.next_comment_id += 1;
        } else {
            g.next_comment_id = g.next_comment_id.max(row.id + 1);
        }
        if row.created_at <= 0 {
            row.created_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
        }
        g.comments.push(row.clone());
        row
    }

    fn next_unix() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    pub fn applications(&self) -> Vec<ApplicationRow> {
        self.inner.read().expect("store").applications.clone()
    }

    pub fn application_of(&self, id: i64) -> Option<ApplicationRow> {
        self.inner
            .read()
            .expect("store")
            .applications
            .iter()
            .find(|r| r.id == id)
            .cloned()
    }

    pub fn application_by_market(&self, market: &str) -> Option<ApplicationRow> {
        let m = market.trim();
        if m.is_empty() {
            return None;
        }
        self.inner
            .read()
            .expect("store")
            .applications
            .iter()
            .find(|r| r.market == m)
            .cloned()
    }

    pub fn approved_create_spec(&self, family: u8, topic: &str, tag: &str) -> Option<ApplicationRow> {
        self.inner
            .read()
            .expect("store")
            .applications
            .iter()
            .find(|r| {
                r.status == 1
                    && r.family == family
                    && r.topic == topic
                    && r.tag == tag
            })
            .cloned()
    }

    pub fn application_by_dup(&self, dup_key: &str) -> Option<ApplicationRow> {
        self.inner
            .read()
            .expect("store")
            .applications
            .iter()
            .filter(|r| r.dup_key == dup_key && (r.status == 0 || r.status == 1))
            .cloned()
            .next()
    }

    pub fn push_application(&self, mut row: ApplicationRow) -> ApplicationRow {
        let mut g = self.inner.write().expect("store");
        if g.next_application_id < 1 {
            g.next_application_id = 1;
        }
        if row.id <= 0 {
            row.id = g.next_application_id;
            g.next_application_id += 1;
        } else {
            g.next_application_id = g.next_application_id.max(row.id + 1);
        }
        if row.created_at <= 0 {
            row.created_at = Self::next_unix();
        }
        if let Some(cur) = g.applications.iter_mut().find(|r| r.id == row.id) {
            *cur = row.clone();
        } else {
            g.applications.push(row.clone());
        }
        row
    }

    pub fn list_applications(&self, status: Option<u8>, applicant: Option<&str>, page: u32, limit: u32) -> (u64, Vec<ApplicationRow>) {
        let mut rows: Vec<ApplicationRow> = self
            .inner
            .read()
            .expect("store")
            .applications
            .iter()
            .filter(|r| status.map(|s| r.status == s).unwrap_or(true))
            .filter(|r| applicant.map(|a| r.applicant == a).unwrap_or(true))
            .cloned()
            .collect();
        rows.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.id.cmp(&a.id)));
        let total = rows.len() as u64;
        let limit = limit.clamp(1, 100);
        let page = page.max(1);
        let start = (u64::from(page.saturating_sub(1)).saturating_mul(u64::from(limit))) as usize;
        let items = rows.into_iter().skip(start).take(limit as usize).collect();
        (total, items)
    }

    pub fn push_review_log(&self, mut row: ReviewLogRow) -> ReviewLogRow {
        let mut g = self.inner.write().expect("store");
        if g.next_review_log_id < 1 {
            g.next_review_log_id = 1;
        }
        if row.id <= 0 {
            row.id = g.next_review_log_id;
            g.next_review_log_id += 1;
        } else {
            g.next_review_log_id = g.next_review_log_id.max(row.id + 1);
        }
        if row.created_at <= 0 {
            row.created_at = Self::next_unix();
        }
        g.review_logs.push(row.clone());
        row
    }

    pub fn review_logs_of(&self, application_id: i64) -> Vec<ReviewLogRow> {
        let mut rows: Vec<ReviewLogRow> = self
            .inner
            .read()
            .expect("store")
            .review_logs
            .iter()
            .filter(|r| r.application_id == application_id)
            .cloned()
            .collect();
        rows.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        rows
    }

    pub fn comments_of(&self, market: &str, page: u32, limit: u32) -> (u64, Vec<CommentRow>) {
        let mut rows: Vec<CommentRow> = self
            .inner
            .read()
            .expect("store")
            .comments
            .iter()
            .filter(|c| c.market == market)
            .cloned()
            .collect();
        rows.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        let total = rows.len() as u64;
        let limit = limit.clamp(1, 100);
        let page = page.max(1);
        let start = (u64::from(page.saturating_sub(1)).saturating_mul(u64::from(limit))) as usize;
        let items = rows.into_iter().skip(start).take(limit as usize).collect();
        (total, items)
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

    pub fn patch_stats(&self, market: &str, traders: u64, tickets: u64, stake_usdc: u64) {
        let mut g = self.inner.write().expect("store");
        if let Some(row) = g.markets.get_mut(market) {
            row.traders = traders;
            row.tickets = tickets;
            row.stake_usdc = stake_usdc;
        }
    }

    pub fn patch_settle(&self, market: &str, phase: u8, rho_raw: i128, cell: u16, liability: u64) {
        let mut g = self.inner.write().expect("store");
        if let Some(row) = g.markets.get_mut(market) {
            row.board_phase = phase;
            row.rho_raw = rho_raw;
            row.settle_cell = cell;
            row.liability = liability;
        }
    }

    pub fn replace_positions(&self, rows: Vec<PositionRow>) {
        self.inner.write().expect("store").positions = rows;
    }

    pub fn positions_of(&self, owner: &str) -> Vec<PositionRow> {
        self.inner
            .read()
            .expect("store")
            .positions
            .iter()
            .filter(|p| p.owner == owner)
            .cloned()
            .collect()
    }

    pub fn replace_risk(&self, quotes: Vec<QuoteRow>, layers: Vec<LayerRow>) {
        let mut g = self.inner.write().expect("store");
        g.quotes = quotes;
        g.layers = layers;
    }

    pub fn quotes_of(&self, owner: &str) -> Vec<QuoteRow> {
        self.inner
            .read()
            .expect("store")
            .quotes
            .iter()
            .filter(|q| q.lp == owner)
            .cloned()
            .collect()
    }

    pub fn quotes_on(&self, market: &str) -> Vec<QuoteRow> {
        self.inner
            .read()
            .expect("store")
            .quotes
            .iter()
            .filter(|q| q.market == market && !q.cancelled)
            .cloned()
            .collect()
    }

    pub fn layers_of(&self, market: &str) -> Vec<LayerRow> {
        self.inner
            .read()
            .expect("store")
            .layers
            .iter()
            .filter(|l| l.market == market)
            .cloned()
            .collect()
    }

    pub fn replace_resolutions(&self, rows: Vec<ResolutionRow>) {
        self.inner.write().expect("store").resolutions = rows;
    }

    pub fn set_committee(&self, row: CommitteeSnap) {
        self.inner.write().expect("store").committee = Some(row);
    }

    pub fn clear_committee(&self) {
        self.inner.write().expect("store").committee = None;
    }

    pub fn committee(&self) -> Option<CommitteeSnap> {
        self.inner.read().expect("store").committee.clone()
    }

    pub fn ledger_genesis(&self) -> Option<String> {
        self.inner.read().expect("store").ledger_genesis.clone()
    }

    pub fn set_ledger_genesis(&self, genesis: impl Into<String>) {
        self.inner.write().expect("store").ledger_genesis = Some(genesis.into());
    }

    /// Drop derived state that cannot outlive the current ledger.
    pub fn wipe_derived(&self) {
        let mut g = self.inner.write().expect("store");
        g.markets.clear();
        g.listings.clear();
        g.fills.clear();
        g.positions.clear();
        g.quotes.clear();
        g.layers.clear();
        g.resolutions.clear();
        g.committee = None;
        g.pool_available = 0;
        g.slot = 0;
        Self::flush_listings(&g);
        Self::flush_fills(&g);
    }

    pub fn retain_markets(&self, live: &std::collections::HashSet<String>) {
        let mut g = self.inner.write().expect("store");
        g.markets.retain(|k, _| live.contains(k));
    }

    pub fn resolution_of(&self, market: &str) -> Option<ResolutionRow> {
        self.inner
            .read()
            .expect("store")
            .resolutions
            .iter()
            .find(|r| r.market == market)
            .cloned()
    }

    pub fn weight_sum(&self, market: &str) -> u64 {
        self.inner
            .read()
            .expect("store")
            .quotes
            .iter()
            .filter(|q| q.market == market)
            .map(|q| u64::from(q.profit_share_bps).saturating_mul(q.filled))
            .sum()
    }

    pub fn set_pool(&self, available: u64) {
        self.inner.write().expect("store").pool_available = available;
    }

    pub fn pool_available(&self) -> u64 {
        self.inner.read().expect("store").pool_available
    }

    pub fn all_positions(&self) -> Vec<PositionRow> {
        self.inner.read().expect("store").positions.clone()
    }

    pub fn all_quotes(&self) -> Vec<QuoteRow> {
        self.inner.read().expect("store").quotes.clone()
    }

    pub fn all_layers(&self) -> Vec<LayerRow> {
        self.inner.read().expect("store").layers.clone()
    }

    pub fn all_resolutions(&self) -> Vec<ResolutionRow> {
        self.inner.read().expect("store").resolutions.clone()
    }

    pub fn patch_tap(&self, market: &str, cap: u64, allocated: u64) {
        let mut g = self.inner.write().expect("store");
        if let Some(row) = g.markets.get_mut(market) {
            row.c_p_board = cap;
            row.c_p_alloc = allocated;
        }
    }
}

/// Local default: machine PostgreSQL after `infra/local-pg.sql` (`cpm` / `cpm`).
pub const LOCAL_DATABASE_URL: &str = "postgres://cpm:cpm@127.0.0.1:5432/cpm";

fn load_dotenv() {
    let Ok(raw) = std::fs::read_to_string(".env") else {
        return;
    };
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let k = k.trim();
        if std::env::var_os(k).is_none() {
            std::env::set_var(k, v.trim());
        }
    }
}

pub fn database_url() -> String {
    load_dotenv();
    std::env::var("DATABASE_URL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| LOCAL_DATABASE_URL.into())
}

pub fn memory_only() -> bool {
    std::env::var("ALLOW_MEMORY_ONLY").ok().as_deref() == Some("1")
}

/// Staging / production must keep Postgres (listing + fill journal).
pub fn memory_forbidden_for_env() -> bool {
    let env = std::env::var("CPM_ENV").unwrap_or_else(|_| "local".into());
    env != "local" && memory_only()
}

/// Listing + fill journal + projections. Memory is a cache. Refuse to run without PG
/// unless `ALLOW_MEMORY_ONLY=1` (unit tests / emergency).
pub async fn open_required_pool(mem: &MemoryStore) -> Result<sqlx::PgPool> {
    let url = database_url();
    pg_migrate_and_load(&url, mem).await.map_err(|e| {
        anyhow::anyhow!(
            "Postgres is required so listing / fill journal / projections survive restart. \
             Local service (preferred): psql -U postgres -h 127.0.0.1 -f infra/local-pg.sql \
             then DATABASE_URL={url} ({e})"
        )
    })
}

pub async fn pg_migrate_and_load(url: &str, mem: &MemoryStore) -> Result<sqlx::PgPool> {
    let pool = sqlx::PgPool::connect(url).await?;
    migrate_projections(&pool).await?;
    crate::infrastructure::migrate_journals(&pool).await?;
    if let Ok(rpc) = std::env::var("RPC_URL") {
        crate::indexer::reconcile_ledger(&rpc, mem, &pool).await?;
    }
    load_projections(&pool, mem).await?;
    crate::infrastructure::load_journals(&pool, mem).await?;
    crate::infrastructure::seed_journals_from_memory(&pool, mem).await?;
    Ok(pool)
}

pub async fn wipe_projection_tables(pool: &sqlx::PgPool) -> Result<()> {
    sqlx::query(
        "TRUNCATE market_proj, position_proj, quote_proj, layer_proj, resolution_proj, pool_proj, listing, fill_journal, market_comment, listing_application, review_log",
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn write_ledger_genesis(pool: &sqlx::PgPool, genesis: &str) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO ledger_proj (id, genesis) VALUES (1, $1)
        ON CONFLICT (id) DO UPDATE SET genesis = EXCLUDED.genesis, updated_at = now()
        "#,
    )
    .bind(genesis)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn stored_ledger_genesis(pool: &sqlx::PgPool) -> Result<Option<String>> {
    Ok(sqlx::query_scalar("SELECT genesis FROM ledger_proj WHERE id = 1")
        .fetch_optional(pool)
        .await?)
}

async fn migrate_projections(pool: &sqlx::PgPool) -> Result<()> {
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
            fee_bps INTEGER NOT NULL DEFAULT 0,
            traders BIGINT NOT NULL DEFAULT 0,
            tickets BIGINT NOT NULL DEFAULT 0,
            stake_usdc BIGINT NOT NULL DEFAULT 0,
            board_phase SMALLINT NOT NULL DEFAULT 0,
            rho_raw TEXT NOT NULL DEFAULT '0',
            settle_cell INTEGER NOT NULL DEFAULT 0,
            liability BIGINT NOT NULL DEFAULT 0,
            c_p_board BIGINT NOT NULL DEFAULT 0,
            c_p_alloc BIGINT NOT NULL DEFAULT 0,
            close_ts BIGINT NOT NULL DEFAULT 0,
            risk_lock_ts BIGINT NOT NULL DEFAULT 0,
            report_window_secs BIGINT NOT NULL DEFAULT 0,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )
        "#,
    )
    .execute(pool)
    .await?;
    for sql in [
        "ALTER TABLE market_proj ADD COLUMN IF NOT EXISTS fee_bps INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE market_proj ADD COLUMN IF NOT EXISTS fee_timing SMALLINT NOT NULL DEFAULT 0",
        "ALTER TABLE market_proj ADD COLUMN IF NOT EXISTS traders BIGINT NOT NULL DEFAULT 0",
        "ALTER TABLE market_proj ADD COLUMN IF NOT EXISTS tickets BIGINT NOT NULL DEFAULT 0",
        "ALTER TABLE market_proj ADD COLUMN IF NOT EXISTS stake_usdc BIGINT NOT NULL DEFAULT 0",
        "ALTER TABLE market_proj ADD COLUMN IF NOT EXISTS board_phase SMALLINT NOT NULL DEFAULT 0",
        "ALTER TABLE market_proj ADD COLUMN IF NOT EXISTS rho_raw TEXT NOT NULL DEFAULT '0'",
        "ALTER TABLE market_proj ADD COLUMN IF NOT EXISTS settle_cell INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE market_proj ADD COLUMN IF NOT EXISTS liability BIGINT NOT NULL DEFAULT 0",
        "ALTER TABLE market_proj ADD COLUMN IF NOT EXISTS c_p_board BIGINT NOT NULL DEFAULT 0",
        "ALTER TABLE market_proj ADD COLUMN IF NOT EXISTS c_p_alloc BIGINT NOT NULL DEFAULT 0",
        "ALTER TABLE market_proj ADD COLUMN IF NOT EXISTS close_ts BIGINT NOT NULL DEFAULT 0",
        "ALTER TABLE market_proj ADD COLUMN IF NOT EXISTS risk_lock_ts BIGINT NOT NULL DEFAULT 0",
        "ALTER TABLE market_proj ADD COLUMN IF NOT EXISTS report_window_secs BIGINT NOT NULL DEFAULT 0",
        "ALTER TABLE market_proj ADD COLUMN IF NOT EXISTS extra_a TEXT NOT NULL DEFAULT '0'",
        "ALTER TABLE market_proj ADD COLUMN IF NOT EXISTS extra_b TEXT NOT NULL DEFAULT '0'",
        "ALTER TABLE market_proj ADD COLUMN IF NOT EXISTS extra_u2 SMALLINT NOT NULL DEFAULT 0",
        "ALTER TABLE market_proj ADD COLUMN IF NOT EXISTS delegated BOOLEAN NOT NULL DEFAULT FALSE",
    ] {
        sqlx::query(sql).execute(pool).await?;
    }
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS position_proj (
            position TEXT PRIMARY KEY,
            owner TEXT NOT NULL,
            market TEXT NOT NULL,
            set_hash TEXT NOT NULL,
            q_raw TEXT NOT NULL,
            shares BIGINT NOT NULL,
            cost_paid BIGINT NOT NULL,
            claimed BOOLEAN NOT NULL,
            paid_usdc BIGINT NOT NULL,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS quote_proj (
            quote TEXT PRIMARY KEY,
            market TEXT NOT NULL,
            lp TEXT NOT NULL,
            layer_id SMALLINT NOT NULL,
            capacity BIGINT NOT NULL,
            filled BIGINT NOT NULL,
            premium BIGINT NOT NULL,
            premium_owed BIGINT NOT NULL,
            profit_share_bps INTEGER NOT NULL,
            cancelled BOOLEAN NOT NULL,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS layer_proj (
            layer TEXT PRIMARY KEY,
            market TEXT NOT NULL,
            layer_id SMALLINT NOT NULL,
            attachment BIGINT NOT NULL,
            thickness BIGINT NOT NULL,
            filled BIGINT NOT NULL,
            quote_count SMALLINT NOT NULL,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS resolution_proj (
            market TEXT PRIMARY KEY,
            record TEXT NOT NULL,
            phase SMALLINT NOT NULL,
            family SMALLINT NOT NULL,
            m SMALLINT NOT NULL,
            n SMALLINT NOT NULL,
            extensions SMALLINT NOT NULL,
            votes_proposal SMALLINT NOT NULL,
            votes_challenge SMALLINT NOT NULL,
            refunds_due BOOLEAN NOT NULL,
            early_resolve BOOLEAN NOT NULL,
            close_ts BIGINT NOT NULL,
            report_deadline BIGINT NOT NULL,
            challenge_end BIGINT NOT NULL,
            vote_end BIGINT NOT NULL,
            report_window_secs BIGINT NOT NULL,
            challenge_secs BIGINT NOT NULL,
            proposer TEXT NOT NULL,
            challenger TEXT NOT NULL,
            authorized_reporter TEXT NOT NULL,
            members JSONB NOT NULL,
            proposed JSONB NOT NULL,
            challenged JSONB NOT NULL,
            final_outcome JSONB NOT NULL,
            evidence_hash TEXT NOT NULL,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS pool_proj (
            id SMALLINT PRIMARY KEY,
            available BIGINT NOT NULL,
            slot BIGINT NOT NULL,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS ledger_proj (
            id SMALLINT PRIMARY KEY,
            genesis TEXT NOT NULL,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )
        "#,
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn load_projections(pool: &sqlx::PgPool, mem: &MemoryStore) -> Result<()> {
    use sqlx::Row;
    let markets = sqlx::query(
        "SELECT market, slot, family, status, n, beta, p0, theta, exposure,
                trading_revenue, premium_payable, c_m, c_r, fee_bps, fee_timing, traders, tickets,
                stake_usdc, board_phase, rho_raw, settle_cell, liability, c_p_board,
                c_p_alloc, close_ts, risk_lock_ts, report_window_secs, extra_a, extra_b, extra_u2
         FROM market_proj",
    )
    .fetch_all(pool)
    .await?;
    for r in markets {
        let beta: String = r.try_get("beta")?;
        let rho_raw: String = r.try_get("rho_raw")?;
        let fee_bps: i32 = r.try_get("fee_bps")?;
        let fee_timing: i16 = r.try_get("fee_timing").unwrap_or(0);
        let settle_cell: i32 = r.try_get("settle_cell")?;
        mem.upsert(MarketProj {
            market: r.try_get("market")?,
            family: r.try_get::<i16, _>("family")? as u8,
            status: r.try_get::<i16, _>("status")? as u8,
            n: r.try_get::<i32, _>("n")? as u16,
            beta: beta.parse().unwrap_or(0),
            p0: json_i128(r.try_get("p0")?),
            theta: json_i128(r.try_get("theta")?),
            exposure: json_i128(r.try_get("exposure")?),
            trading_revenue: r.try_get::<i64, _>("trading_revenue")? as u64,
            premium_payable: r.try_get::<i64, _>("premium_payable")? as u64,
            c_m: r.try_get::<i64, _>("c_m")? as u64,
            c_r: r.try_get::<i64, _>("c_r")? as u64,
            fee_bps: fee_bps.max(0) as u16,
            fee_timing: fee_timing.clamp(0, 1) as u8,
            slot: r.try_get::<i64, _>("slot")? as u64,
            traders: r.try_get::<i64, _>("traders")? as u64,
            tickets: r.try_get::<i64, _>("tickets")? as u64,
            stake_usdc: r.try_get::<i64, _>("stake_usdc")? as u64,
            board_phase: r.try_get::<i16, _>("board_phase")? as u8,
            rho_raw: rho_raw.parse().unwrap_or(0),
            settle_cell: settle_cell.max(0) as u16,
            liability: r.try_get::<i64, _>("liability")? as u64,
            c_p_board: r.try_get::<i64, _>("c_p_board")? as u64,
            c_p_alloc: r.try_get::<i64, _>("c_p_alloc")? as u64,
            close_ts: r.try_get("close_ts")?,
            risk_lock_ts: r.try_get("risk_lock_ts")?,
            report_window_secs: r.try_get("report_window_secs")?,
            extra_a: r.try_get::<String, _>("extra_a").ok().and_then(|s| s.parse().ok()).unwrap_or(0),
            extra_b: r.try_get::<String, _>("extra_b").ok().and_then(|s| s.parse().ok()).unwrap_or(0),
            extra_u2: r.try_get::<i16, _>("extra_u2").unwrap_or(0).clamp(0, 255) as u8,
            delegated: r.try_get::<bool, _>("delegated").unwrap_or(false),
        });
    }
    let positions = sqlx::query(
        "SELECT position, owner, market, set_hash, q_raw, shares, cost_paid, claimed, paid_usdc
         FROM position_proj",
    )
    .fetch_all(pool)
    .await?;
    mem.replace_positions(
        positions
            .into_iter()
            .map(|r| {
                let q_raw: String = r.try_get("q_raw").unwrap_or_default();
                PositionRow {
                    position: r.try_get("position").unwrap_or_default(),
                    owner: r.try_get("owner").unwrap_or_default(),
                    market: r.try_get("market").unwrap_or_default(),
                    set_hash: r.try_get("set_hash").unwrap_or_default(),
                    q_raw: q_raw.parse().unwrap_or(0),
                    shares: r.try_get::<i64, _>("shares").unwrap_or(0) as u64,
                    cost_paid: r.try_get::<i64, _>("cost_paid").unwrap_or(0) as u64,
                    claimed: r.try_get("claimed").unwrap_or(false),
                    paid_usdc: r.try_get::<i64, _>("paid_usdc").unwrap_or(0) as u64,
                }
            })
            .collect(),
    );
    let quotes = sqlx::query(
        "SELECT quote, market, lp, layer_id, capacity, filled, premium, premium_owed,
                profit_share_bps, cancelled FROM quote_proj",
    )
    .fetch_all(pool)
    .await?;
    let layers = sqlx::query(
        "SELECT layer, market, layer_id, attachment, thickness, filled, quote_count FROM layer_proj",
    )
    .fetch_all(pool)
    .await?;
    mem.replace_risk(
        quotes
            .into_iter()
            .map(|r| QuoteRow {
                quote: r.try_get("quote").unwrap_or_default(),
                market: r.try_get("market").unwrap_or_default(),
                lp: r.try_get("lp").unwrap_or_default(),
                layer_id: r.try_get::<i16, _>("layer_id").unwrap_or(0) as u8,
                capacity: r.try_get::<i64, _>("capacity").unwrap_or(0) as u64,
                filled: r.try_get::<i64, _>("filled").unwrap_or(0) as u64,
                premium: r.try_get::<i64, _>("premium").unwrap_or(0) as u64,
                premium_owed: r.try_get::<i64, _>("premium_owed").unwrap_or(0) as u64,
                profit_share_bps: r.try_get::<i32, _>("profit_share_bps").unwrap_or(0).max(0) as u16,
                cancelled: r.try_get("cancelled").unwrap_or(false),
            })
            .collect(),
        layers
            .into_iter()
            .map(|r| LayerRow {
                layer: r.try_get("layer").unwrap_or_default(),
                market: r.try_get("market").unwrap_or_default(),
                layer_id: r.try_get::<i16, _>("layer_id").unwrap_or(0) as u8,
                attachment: r.try_get::<i64, _>("attachment").unwrap_or(0) as u64,
                thickness: r.try_get::<i64, _>("thickness").unwrap_or(0) as u64,
                filled: r.try_get::<i64, _>("filled").unwrap_or(0) as u64,
                quote_count: r.try_get::<i16, _>("quote_count").unwrap_or(0) as u8,
            })
            .collect(),
    );
    let resolutions = sqlx::query(
        "SELECT market, record, phase, family, m, n, extensions, votes_proposal, votes_challenge,
                refunds_due, early_resolve, close_ts, report_deadline, challenge_end, vote_end,
                report_window_secs, challenge_secs, proposer, challenger, authorized_reporter,
                members, proposed, challenged, final_outcome, evidence_hash
         FROM resolution_proj",
    )
    .fetch_all(pool)
    .await?;
    mem.replace_resolutions(
        resolutions
            .into_iter()
            .map(|r| ResolutionRow {
                market: r.try_get("market").unwrap_or_default(),
                record: r.try_get("record").unwrap_or_default(),
                phase: r.try_get::<i16, _>("phase").unwrap_or(0) as u8,
                family: r.try_get::<i16, _>("family").unwrap_or(0) as u8,
                m: r.try_get::<i16, _>("m").unwrap_or(0) as u8,
                n: r.try_get::<i16, _>("n").unwrap_or(0) as u8,
                extensions: r.try_get::<i16, _>("extensions").unwrap_or(0) as u8,
                votes_proposal: r.try_get::<i16, _>("votes_proposal").unwrap_or(0) as u8,
                votes_challenge: r.try_get::<i16, _>("votes_challenge").unwrap_or(0) as u8,
                refunds_due: r.try_get("refunds_due").unwrap_or(false),
                early_resolve: r.try_get("early_resolve").unwrap_or(false),
                close_ts: r.try_get("close_ts").unwrap_or(0),
                report_deadline: r.try_get("report_deadline").unwrap_or(0),
                challenge_end: r.try_get("challenge_end").unwrap_or(0),
                vote_end: r.try_get("vote_end").unwrap_or(0),
                report_window_secs: r.try_get("report_window_secs").unwrap_or(0),
                challenge_secs: r.try_get("challenge_secs").unwrap_or(0),
                proposer: r.try_get("proposer").unwrap_or_default(),
                challenger: r.try_get("challenger").unwrap_or_default(),
                authorized_reporter: r.try_get("authorized_reporter").unwrap_or_default(),
                members: json_strings(r.try_get("members").unwrap_or(serde_json::Value::Null)),
                proposed: json_outcome(r.try_get("proposed").unwrap_or(serde_json::Value::Null)),
                challenged: json_outcome(r.try_get("challenged").unwrap_or(serde_json::Value::Null)),
                final_outcome: json_outcome(r.try_get("final_outcome").unwrap_or(serde_json::Value::Null)),
                evidence_hash: r.try_get("evidence_hash").unwrap_or_default(),
            })
            .collect(),
    );
    if let Ok(Some(row)) = sqlx::query("SELECT available, slot FROM pool_proj WHERE id = 1")
        .fetch_optional(pool)
        .await
    {
        mem.set_pool(row.try_get::<i64, _>("available").unwrap_or(0) as u64);
        mem.set_slot(row.try_get::<i64, _>("slot").unwrap_or(0) as u64);
    }
    Ok(())
}

fn json_vec(xs: &[i128]) -> Result<serde_json::Value> {
    Ok(serde_json::to_value(xs.iter().map(|x| x.to_string()).collect::<Vec<_>>())?)
}

async fn upsert_market_tx(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, row: &MarketProj) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO market_proj (
            market, slot, family, status, n, beta, p0, theta, exposure,
            trading_revenue, premium_payable, c_m, c_r, fee_bps, fee_timing, traders, tickets,
            stake_usdc, board_phase, rho_raw, settle_cell, liability, c_p_board,
            c_p_alloc, close_ts, risk_lock_ts, report_window_secs, extra_a, extra_b, extra_u2, delegated
        ) VALUES (
            $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,
            $17,$18,$19,$20,$21,$22,$23,$24,$25,$26,$27,$28,$29,$30,$31
        )
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
            fee_bps = EXCLUDED.fee_bps,
            fee_timing = EXCLUDED.fee_timing,
            traders = EXCLUDED.traders,
            tickets = EXCLUDED.tickets,
            stake_usdc = EXCLUDED.stake_usdc,
            board_phase = EXCLUDED.board_phase,
            rho_raw = EXCLUDED.rho_raw,
            settle_cell = EXCLUDED.settle_cell,
            liability = EXCLUDED.liability,
            c_p_board = EXCLUDED.c_p_board,
            c_p_alloc = EXCLUDED.c_p_alloc,
            close_ts = EXCLUDED.close_ts,
            risk_lock_ts = EXCLUDED.risk_lock_ts,
            report_window_secs = EXCLUDED.report_window_secs,
            extra_a = EXCLUDED.extra_a,
            extra_b = EXCLUDED.extra_b,
            extra_u2 = EXCLUDED.extra_u2,
            delegated = EXCLUDED.delegated,
            updated_at = now()
        "#,
    )
    .bind(&row.market)
    .bind(row.slot as i64)
    .bind(row.family as i16)
    .bind(row.status as i16)
    .bind(row.n as i32)
    .bind(row.beta.to_string())
    .bind(json_vec(&row.p0)?)
    .bind(json_vec(&row.theta)?)
    .bind(json_vec(&row.exposure)?)
    .bind(row.trading_revenue as i64)
    .bind(row.premium_payable as i64)
    .bind(row.c_m as i64)
    .bind(row.c_r as i64)
    .bind(i32::from(row.fee_bps))
    .bind(i16::from(row.fee_timing))
    .bind(row.traders as i64)
    .bind(row.tickets as i64)
    .bind(row.stake_usdc as i64)
    .bind(row.board_phase as i16)
    .bind(row.rho_raw.to_string())
    .bind(i32::from(row.settle_cell))
    .bind(row.liability as i64)
    .bind(row.c_p_board as i64)
    .bind(row.c_p_alloc as i64)
    .bind(row.close_ts)
    .bind(row.risk_lock_ts)
    .bind(row.report_window_secs)
    .bind(row.extra_a.to_string())
    .bind(row.extra_b.to_string())
    .bind(i16::from(row.extra_u2))
    .bind(row.delegated)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// One Indexer slot: book + positions + risk + resolution + pool in one transaction.
pub async fn persist_projections(pool: &sqlx::PgPool, store: &MemoryStore) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM market_proj").execute(&mut *tx).await?;
    for row in store.list() {
        upsert_market_tx(&mut tx, &row).await?;
    }
    sqlx::query("DELETE FROM position_proj").execute(&mut *tx).await?;
    for p in store.all_positions() {
        sqlx::query(
            r#"
            INSERT INTO position_proj (
                position, owner, market, set_hash, q_raw, shares, cost_paid, claimed, paid_usdc
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
            "#,
        )
        .bind(&p.position)
        .bind(&p.owner)
        .bind(&p.market)
        .bind(&p.set_hash)
        .bind(p.q_raw.to_string())
        .bind(p.shares as i64)
        .bind(p.cost_paid as i64)
        .bind(p.claimed)
        .bind(p.paid_usdc as i64)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query("DELETE FROM quote_proj").execute(&mut *tx).await?;
    for q in store.all_quotes() {
        sqlx::query(
            r#"
            INSERT INTO quote_proj (
                quote, market, lp, layer_id, capacity, filled, premium, premium_owed,
                profit_share_bps, cancelled
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
            "#,
        )
        .bind(&q.quote)
        .bind(&q.market)
        .bind(&q.lp)
        .bind(q.layer_id as i16)
        .bind(q.capacity as i64)
        .bind(q.filled as i64)
        .bind(q.premium as i64)
        .bind(q.premium_owed as i64)
        .bind(i32::from(q.profit_share_bps))
        .bind(q.cancelled)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query("DELETE FROM layer_proj").execute(&mut *tx).await?;
    for l in store.all_layers() {
        sqlx::query(
            r#"
            INSERT INTO layer_proj (
                layer, market, layer_id, attachment, thickness, filled, quote_count
            ) VALUES ($1,$2,$3,$4,$5,$6,$7)
            "#,
        )
        .bind(&l.layer)
        .bind(&l.market)
        .bind(l.layer_id as i16)
        .bind(l.attachment as i64)
        .bind(l.thickness as i64)
        .bind(l.filled as i64)
        .bind(l.quote_count as i16)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query("DELETE FROM resolution_proj").execute(&mut *tx).await?;
    for r in store.all_resolutions() {
        sqlx::query(
            r#"
            INSERT INTO resolution_proj (
                market, record, phase, family, m, n, extensions, votes_proposal, votes_challenge,
                refunds_due, early_resolve, close_ts, report_deadline, challenge_end, vote_end,
                report_window_secs, challenge_secs, proposer, challenger, authorized_reporter,
                members, proposed, challenged, final_outcome, evidence_hash
            ) VALUES (
                $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,
                $21,$22,$23,$24,$25
            )
            "#,
        )
        .bind(&r.market)
        .bind(&r.record)
        .bind(r.phase as i16)
        .bind(r.family as i16)
        .bind(r.m as i16)
        .bind(r.n as i16)
        .bind(r.extensions as i16)
        .bind(r.votes_proposal as i16)
        .bind(r.votes_challenge as i16)
        .bind(r.refunds_due)
        .bind(r.early_resolve)
        .bind(r.close_ts)
        .bind(r.report_deadline)
        .bind(r.challenge_end)
        .bind(r.vote_end)
        .bind(r.report_window_secs)
        .bind(r.challenge_secs)
        .bind(&r.proposer)
        .bind(&r.challenger)
        .bind(&r.authorized_reporter)
        .bind(serde_json::to_value(&r.members)?)
        .bind(serde_json::to_value(&r.proposed)?)
        .bind(serde_json::to_value(&r.challenged)?)
        .bind(serde_json::to_value(&r.final_outcome)?)
        .bind(&r.evidence_hash)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query(
        r#"
        INSERT INTO pool_proj (id, available, slot)
        VALUES (1, $1, $2)
        ON CONFLICT (id) DO UPDATE SET
            available = EXCLUDED.available,
            slot = EXCLUDED.slot,
            updated_at = now()
        "#,
    )
    .bind(store.pool_available() as i64)
    .bind(store.slot() as i64)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn pg_upsert(pool: &sqlx::PgPool, row: &MarketProj) -> Result<()> {
    let mut tx = pool.begin().await?;
    upsert_market_tx(&mut tx, row).await?;
    tx.commit().await?;
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

fn json_strings(v: serde_json::Value) -> Vec<String> {
    match v {
        serde_json::Value::Array(items) => items
            .into_iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

fn json_outcome(v: serde_json::Value) -> OutcomeSnap {
    serde_json::from_value(v).unwrap_or_else(|_| OutcomeSnap {
        kind: 0,
        a: "0".into(),
        b: "0".into(),
        label: String::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn row(id: &str) -> MarketProj {
        MarketProj {
            market: id.into(),
            family: 1,
            status: 1,
            n: 2,
            beta: 1,
            p0: vec![1, 1],
            theta: vec![0, 0],
            exposure: vec![0, 0],
            trading_revenue: 0,
            premium_payable: 0,
            c_m: 0,
            c_r: 0,
            fee_bps: 0,
            fee_timing: 0,
            slot: 1,
            traders: 0,
            tickets: 0,
            stake_usdc: 0,
            board_phase: 0,
            rho_raw: 0,
            settle_cell: 0,
            liability: 0,
            c_p_board: 0,
            c_p_alloc: 0,
            close_ts: 0,
            risk_lock_ts: 0,
            report_window_secs: 0,
            extra_a: 0,
            extra_b: 0,
            extra_u2: 0,
            delegated: false,
        }
    }

    #[test]
    fn retain_drops_markets_not_on_chain() {
        let store = MemoryStore::new();
        store.upsert(row("a"));
        store.upsert(row("b"));
        store.retain_markets(&HashSet::from(["a".into()]));
        let left = store.list();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].market, "a");
    }

    #[test]
    fn wipe_clears_catalog() {
        let store = MemoryStore::new();
        store.upsert(row("a"));
        store.set_listing(
            "a",
            ListingMeta {
                title: "old".into(),
                tags: vec!["macro".into()],
                category: "macro".into(),
                topic: String::new(),
                tag: String::new(),
                description: String::new(),
                event: String::new(),
                blocked_regions: Vec::new(),
            },
        );
        store.wipe_derived();
        assert!(store.list().is_empty());
        assert!(store.listings().is_empty());
        assert!(store.catalog_tags().iter().any(|t| t.name == "macro" && t.used == 0));
    }

    #[test]
    fn local_env_allows_memory_flag() {
        let env = std::env::var("CPM_ENV").unwrap_or_else(|_| "local".into());
        if env == "local" && !memory_only() {
            assert!(!memory_forbidden_for_env());
        }
    }
}
