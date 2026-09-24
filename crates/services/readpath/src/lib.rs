//! Phase 4 read path. Projections are not the ledger (FR-DUR-04).

pub mod api;
pub mod indexer;
pub mod store;

pub use api::{router, AppState};
pub use indexer::{poll_once, project, spawn_poller};
pub use store::{MemoryStore, MarketProj};
