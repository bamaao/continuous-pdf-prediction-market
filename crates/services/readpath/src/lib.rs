//! Phase 4 read path. Projections are not the ledger (FR-DUR-04).

pub mod api;
pub mod application;
pub mod compose;
pub mod domain;
pub mod indexer;
pub mod peak;
pub mod infrastructure;
pub mod store;

pub use api::{router, router_with_pool, AppState};
pub use indexer::{poll_once, position_stats, project, reconcile_ledger, spawn_poller};
pub use store::{
    database_url, memory_forbidden_for_env, memory_only, open_required_pool, persist_projections, ApplicationRow, CatalogTagRow, CommentRow,
    FillMeta, LayerRow, ListingMeta, MemoryStore, MarketProj, PositionRow, QuoteRow, ReviewLogRow, LOCAL_DATABASE_URL,
};
