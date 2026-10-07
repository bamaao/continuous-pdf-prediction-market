//! Pure catalog / journal types. No sqlx, Axum, or RPC.

mod comment;
mod error;
mod fill;
mod listing;
mod locale;
mod repos;
mod review;

pub use comment::{valid_pubkey, Comment};
pub use error::DomainError;
pub use fill::Fill;
pub use listing::{decode_tags, encode_tags, normalize_tag, normalize_tags, CatalogTag, Listing, DEFAULT_CATALOG_TAGS};
pub use locale::{
    accept_language_chain, i18n_from_json, i18n_to_json, normalize_i18n, normalize_locale, normalize_source_locale,
    pick_display, DisplayListing, I18nMap, LocaleCopy,
};
pub use repos::{
    CatalogTagRepository, CommentRepository, FillJournalRepository, ListingApplicationRepository, ListingRepository,
};
pub use review::{
    duplicate_key, normalize_regions, region_blocked, ListingApplication, ReviewLog, APP_APPROVED, APP_DUPLICATE,
    APP_PENDING, APP_REJECTED,
};
