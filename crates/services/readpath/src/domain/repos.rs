use super::{CatalogTag, Comment, DomainError, Fill, Listing, ListingApplication, ReviewLog};

pub trait CatalogTagRepository: Send + Sync {
    type Context<'c>
    where
        Self: 'c;

    fn save(
        &self,
        ctx: &mut Self::Context<'_>,
        tag: &CatalogTag,
    ) -> impl std::future::Future<Output = Result<(), DomainError>> + Send;

    fn delete(
        &self,
        ctx: &mut Self::Context<'_>,
        name: &str,
    ) -> impl std::future::Future<Output = Result<(), DomainError>> + Send;
}

/// Domain port. Infrastructure binds `Context` to a sqlx transaction.
pub trait ListingRepository: Send + Sync {
    type Context<'c>
    where
        Self: 'c;

    fn save(
        &self,
        ctx: &mut Self::Context<'_>,
        listing: &Listing,
    ) -> impl std::future::Future<Output = Result<(), DomainError>> + Send;

    fn get(
        &self,
        ctx: &mut Self::Context<'_>,
        market: &str,
    ) -> impl std::future::Future<Output = Result<Option<Listing>, DomainError>> + Send;
}

pub trait ListingApplicationRepository: Send + Sync {
    type Context<'c>
    where
        Self: 'c;

    fn save(
        &self,
        ctx: &mut Self::Context<'_>,
        row: &ListingApplication,
    ) -> impl std::future::Future<Output = Result<ListingApplication, DomainError>> + Send;

    fn save_log(
        &self,
        ctx: &mut Self::Context<'_>,
        log: &ReviewLog,
    ) -> impl std::future::Future<Output = Result<ReviewLog, DomainError>> + Send;
}

pub trait CommentRepository: Send + Sync {
    type Context<'c>
    where
        Self: 'c;

    fn save(
        &self,
        ctx: &mut Self::Context<'_>,
        comment: &Comment,
    ) -> impl std::future::Future<Output = Result<Comment, DomainError>> + Send;

    fn list(
        &self,
        ctx: &mut Self::Context<'_>,
        market: &str,
        offset: i64,
        limit: i64,
    ) -> impl std::future::Future<Output = Result<Vec<Comment>, DomainError>> + Send;
}

pub trait FillJournalRepository: Send + Sync {
    type Context<'c>
    where
        Self: 'c;

    fn save(
        &self,
        ctx: &mut Self::Context<'_>,
        fill: &Fill,
    ) -> impl std::future::Future<Output = Result<(), DomainError>> + Send;
}
