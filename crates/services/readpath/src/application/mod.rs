//! Use-cases. Opens a sqlx transaction and passes it as Domain `Context`.

use crate::domain::{
    CatalogTag, CatalogTagRepository, Comment, CommentRepository, DomainError, Fill, FillJournalRepository, Listing,
    ListingApplication, ListingApplicationRepository, ListingRepository, ReviewLog,
};
use crate::infrastructure::{
    CatalogTagRepositoryImpl, CommentRepositoryImpl, FillJournalRepositoryImpl, ListingApplicationRepositoryImpl,
    ListingRepositoryImpl, PgPool,
};

#[derive(Clone)]
pub struct CatalogService {
    pool: Option<PgPool>,
    listings: ListingRepositoryImpl,
    fills: FillJournalRepositoryImpl,
    tags: CatalogTagRepositoryImpl,
    comments: CommentRepositoryImpl,
    applications: ListingApplicationRepositoryImpl,
}

impl CatalogService {
    pub fn new(pool: Option<PgPool>) -> Self {
        Self {
            pool,
            listings: ListingRepositoryImpl,
            fills: FillJournalRepositoryImpl,
            tags: CatalogTagRepositoryImpl,
            comments: CommentRepositoryImpl,
            applications: ListingApplicationRepositoryImpl,
        }
    }

    pub fn has_pg(&self) -> bool {
        self.pool.is_some()
    }

    pub async fn put_listing(&self, listing: &Listing) -> Result<(), DomainError> {
        let Some(pool) = &self.pool else {
            return Ok(());
        };
        let mut tx = pool.begin().await.map_err(|_| DomainError::Storage)?;
        self.listings.save(&mut tx, listing).await?;
        tx.commit().await.map_err(|_| DomainError::Storage)?;
        Ok(())
    }

    pub async fn put_tag(&self, tag: &CatalogTag) -> Result<(), DomainError> {
        let Some(pool) = &self.pool else {
            return Ok(());
        };
        let mut tx = pool.begin().await.map_err(|_| DomainError::Storage)?;
        self.tags.save(&mut tx, tag).await?;
        tx.commit().await.map_err(|_| DomainError::Storage)?;
        Ok(())
    }

    pub async fn delete_tag(&self, name: &str) -> Result<(), DomainError> {
        let Some(pool) = &self.pool else {
            return Ok(());
        };
        let mut tx = pool.begin().await.map_err(|_| DomainError::Storage)?;
        self.tags.delete(&mut tx, name).await?;
        tx.commit().await.map_err(|_| DomainError::Storage)?;
        Ok(())
    }

    pub async fn put_comment(&self, comment: &Comment) -> Result<Comment, DomainError> {
        let Some(pool) = &self.pool else {
            return Ok(comment.clone());
        };
        let mut tx = pool.begin().await.map_err(|_| DomainError::Storage)?;
        let saved = self.comments.save(&mut tx, comment).await?;
        tx.commit().await.map_err(|_| DomainError::Storage)?;
        Ok(saved)
    }

    pub async fn list_comments(&self, market: &str, offset: i64, limit: i64) -> Result<Option<Vec<Comment>>, DomainError> {
        let Some(pool) = &self.pool else {
            return Ok(None);
        };
        let mut tx = pool.begin().await.map_err(|_| DomainError::Storage)?;
        let rows = self.comments.list(&mut tx, market, offset, limit).await?;
        tx.commit().await.map_err(|_| DomainError::Storage)?;
        Ok(Some(rows))
    }

    pub async fn record_application(
        &self,
        row: &ListingApplication,
        log: &ReviewLog,
    ) -> Result<(ListingApplication, ReviewLog), DomainError> {
        let Some(pool) = &self.pool else {
            return Ok((row.clone(), log.clone()));
        };
        let mut tx = pool.begin().await.map_err(|_| DomainError::Storage)?;
        let saved = self.applications.save(&mut tx, row).await?;
        let mut log = log.clone();
        log.application_id = saved.id;
        let log = self.applications.save_log(&mut tx, &log).await?;
        tx.commit().await.map_err(|_| DomainError::Storage)?;
        Ok((saved, log))
    }

    pub async fn put_fill(&self, fill: &Fill) -> Result<(), DomainError> {
        let Some(pool) = &self.pool else {
            return Ok(());
        };
        if fill.set_hash.trim().is_empty() {
            return Ok(());
        }
        let mut tx = pool.begin().await.map_err(|_| DomainError::Storage)?;
        self.fills.save(&mut tx, fill).await?;
        tx.commit().await.map_err(|_| DomainError::Storage)?;
        Ok(())
    }
}
