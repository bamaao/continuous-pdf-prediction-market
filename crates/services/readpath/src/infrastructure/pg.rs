use crate::domain::{
    decode_tags, encode_tags, CatalogTag, CatalogTagRepository, Comment, CommentRepository, DomainError, Fill,
    FillJournalRepository, Listing, ListingApplication, ListingApplicationRepository, ListingRepository, ReviewLog,
    DEFAULT_CATALOG_TAGS,
};
use crate::store::{ApplicationRow, CommentRow, FillMeta, ListingMeta, MemoryStore, ReviewLogRow};
use anyhow::Result;
use sqlx::Row;

pub use sqlx::PgPool;

#[derive(Clone, Copy, Default)]
pub struct ListingRepositoryImpl;
#[derive(Clone, Copy, Default)]
pub struct CommentRepositoryImpl;
#[derive(Clone, Copy, Default)]
pub struct FillJournalRepositoryImpl;
#[derive(Clone, Copy, Default)]
pub struct CatalogTagRepositoryImpl;
#[derive(Clone, Copy, Default)]
pub struct ListingApplicationRepositoryImpl;

async fn upsert_catalog_tags(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, tags: &[String]) -> Result<(), DomainError> {
    for name in tags {
        sqlx::query("INSERT INTO catalog_tag (name) VALUES ($1) ON CONFLICT (name) DO UPDATE SET updated_at = now()")
            .bind(name)
            .execute(&mut **tx)
            .await
            .map_err(|_| DomainError::Storage)?;
    }
    Ok(())
}

impl ListingRepository for ListingRepositoryImpl {
    type Context<'c> = sqlx::Transaction<'c, sqlx::Postgres>;

    async fn save(&self, ctx: &mut Self::Context<'_>, listing: &Listing) -> Result<(), DomainError> {
        sqlx::query(
            r#"
            INSERT INTO listing (market, title, category, tags, topic, tag, description, event)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (market) DO UPDATE SET
                title = EXCLUDED.title,
                category = EXCLUDED.category,
                tags = EXCLUDED.tags,
                topic = EXCLUDED.topic,
                tag = EXCLUDED.tag,
                description = EXCLUDED.description,
                event = EXCLUDED.event,
                updated_at = now()
            "#,
        )
        .bind(&listing.market)
        .bind(&listing.title)
        .bind(&listing.category)
        .bind(encode_tags(&listing.tags))
        .bind(&listing.topic)
        .bind(&listing.tag)
        .bind(&listing.description)
        .bind(&listing.event)
        .execute(&mut **ctx)
        .await
        .map_err(|_| DomainError::Storage)?;
        upsert_catalog_tags(ctx, &listing.tags).await?;
        Ok(())
    }

    async fn get(&self, ctx: &mut Self::Context<'_>, market: &str) -> Result<Option<Listing>, DomainError> {
        let row: Option<(String, String, String, String, String, String, String, String)> = sqlx::query_as(
            "SELECT market, title, category, tags, topic, tag, description, event FROM listing WHERE market = $1",
        )
        .bind(market)
        .fetch_optional(&mut **ctx)
        .await
        .map_err(|_| DomainError::Storage)?;
        Ok(row.map(|(market, title, category, tags, topic, tag, description, event)| {
            let tags = decode_tags(&tags, &category);
            Listing {
                market,
                title,
                category: tags.first().cloned().unwrap_or(category),
                tags,
                topic,
                tag,
                description,
                event,
            }
        }))
    }
}

impl CatalogTagRepository for CatalogTagRepositoryImpl {
    type Context<'c> = sqlx::Transaction<'c, sqlx::Postgres>;

    async fn save(&self, ctx: &mut Self::Context<'_>, tag: &CatalogTag) -> Result<(), DomainError> {
        upsert_catalog_tags(ctx, std::slice::from_ref(&tag.name)).await
    }

    async fn delete(&self, ctx: &mut Self::Context<'_>, name: &str) -> Result<(), DomainError> {
        sqlx::query("DELETE FROM catalog_tag WHERE name = $1")
            .bind(name)
            .execute(&mut **ctx)
            .await
            .map_err(|_| DomainError::Storage)?;
        Ok(())
    }
}

impl CommentRepository for CommentRepositoryImpl {
    type Context<'c> = sqlx::Transaction<'c, sqlx::Postgres>;

    async fn save(&self, ctx: &mut Self::Context<'_>, comment: &Comment) -> Result<Comment, DomainError> {
        let row: (i64, i64) = sqlx::query_as(
            r#"
            INSERT INTO market_comment (market, author, body)
            VALUES ($1, $2, $3)
            RETURNING id, (EXTRACT(EPOCH FROM created_at))::bigint
            "#,
        )
        .bind(&comment.market)
        .bind(&comment.author)
        .bind(&comment.body)
        .fetch_one(&mut **ctx)
        .await
        .map_err(|_| DomainError::Storage)?;
        Ok(Comment {
            id: row.0,
            market: comment.market.clone(),
            author: comment.author.clone(),
            body: comment.body.clone(),
            created_at: row.1,
        })
    }

    async fn list(
        &self,
        ctx: &mut Self::Context<'_>,
        market: &str,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<Comment>, DomainError> {
        let rows: Vec<(i64, String, String, String, i64)> = sqlx::query_as(
            r#"
            SELECT id, market, author, body, (EXTRACT(EPOCH FROM created_at))::bigint
            FROM market_comment
            WHERE market = $1
            ORDER BY created_at ASC, id ASC
            OFFSET $2 LIMIT $3
            "#,
        )
        .bind(market)
        .bind(offset)
        .bind(limit)
        .fetch_all(&mut **ctx)
        .await
        .map_err(|_| DomainError::Storage)?;
        Ok(rows
            .into_iter()
            .map(|(id, market, author, body, created_at)| Comment {
                id,
                market,
                author,
                body,
                created_at,
            })
            .collect())
    }
}

impl ListingApplicationRepository for ListingApplicationRepositoryImpl {
    type Context<'c> = sqlx::Transaction<'c, sqlx::Postgres>;

    async fn save(&self, ctx: &mut Self::Context<'_>, row: &ListingApplication) -> Result<ListingApplication, DomainError> {
        let tags = encode_tags(&row.tags);
        let regions = encode_tags(&row.blocked_regions);
        if row.id > 0 {
            sqlx::query(
                r#"
                UPDATE listing_application SET
                    applicant=$2, family=$3, title=$4, tags=$5, event=$6, description=$7,
                    topic=$8, tag=$9, blocked_regions=$10, dup_key=$11, status=$12,
                    reviewer=$13, reason=$14, compose_json=$15, market=$16,
                    reviewed_at=CASE WHEN $12=0 THEN reviewed_at ELSE now() END
                WHERE id=$1
                "#,
            )
            .bind(row.id)
            .bind(&row.applicant)
            .bind(i16::from(row.family))
            .bind(&row.title)
            .bind(&tags)
            .bind(&row.event)
            .bind(&row.description)
            .bind(&row.topic)
            .bind(&row.tag)
            .bind(&regions)
            .bind(&row.dup_key)
            .bind(i16::from(row.status))
            .bind(&row.reviewer)
            .bind(&row.reason)
            .bind(&row.compose_json)
            .bind(&row.market)
            .execute(&mut **ctx)
            .await
            .map_err(|_| DomainError::Storage)?;
            return Ok(row.clone());
        }
        let saved: (i64, i64) = sqlx::query_as(
            r#"
            INSERT INTO listing_application (
                applicant, family, title, tags, event, description, topic, tag,
                blocked_regions, dup_key, status, reviewer, reason, compose_json, market
            ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)
            RETURNING id, (EXTRACT(EPOCH FROM created_at))::bigint
            "#,
        )
        .bind(&row.applicant)
        .bind(i16::from(row.family))
        .bind(&row.title)
        .bind(&tags)
        .bind(&row.event)
        .bind(&row.description)
        .bind(&row.topic)
        .bind(&row.tag)
        .bind(&regions)
        .bind(&row.dup_key)
        .bind(i16::from(row.status))
        .bind(&row.reviewer)
        .bind(&row.reason)
        .bind(&row.compose_json)
        .bind(&row.market)
        .fetch_one(&mut **ctx)
        .await
        .map_err(|_| DomainError::Storage)?;
        let mut out = row.clone();
        out.id = saved.0;
        out.created_at = saved.1;
        Ok(out)
    }

    async fn save_log(&self, ctx: &mut Self::Context<'_>, log: &ReviewLog) -> Result<ReviewLog, DomainError> {
        let saved: (i64, i64) = sqlx::query_as(
            r#"
            INSERT INTO review_log (application_id, reviewer, action, reason)
            VALUES ($1, $2, $3, $4)
            RETURNING id, (EXTRACT(EPOCH FROM created_at))::bigint
            "#,
        )
        .bind(log.application_id)
        .bind(&log.reviewer)
        .bind(&log.action)
        .bind(&log.reason)
        .fetch_one(&mut **ctx)
        .await
        .map_err(|_| DomainError::Storage)?;
        Ok(ReviewLog {
            id: saved.0,
            application_id: log.application_id,
            reviewer: log.reviewer.clone(),
            action: log.action.clone(),
            reason: log.reason.clone(),
            created_at: saved.1,
        })
    }
}

impl FillJournalRepository for FillJournalRepositoryImpl {
    type Context<'c> = sqlx::Transaction<'c, sqlx::Postgres>;

    async fn save(&self, ctx: &mut Self::Context<'_>, fill: &Fill) -> Result<(), DomainError> {
        sqlx::query(
            r#"
            INSERT INTO fill_journal (owner, market, set_hash, kind, mask, skellam_kind, a, b)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (owner, market, set_hash) DO UPDATE SET
                kind = EXCLUDED.kind,
                mask = EXCLUDED.mask,
                skellam_kind = EXCLUDED.skellam_kind,
                a = EXCLUDED.a,
                b = EXCLUDED.b,
                updated_at = now()
            "#,
        )
        .bind(&fill.owner)
        .bind(&fill.market)
        .bind(&fill.set_hash)
        .bind(&fill.kind)
        .bind(&fill.mask)
        .bind(fill.skellam_kind.map(i16::from))
        .bind(fill.a)
        .bind(fill.b)
        .execute(&mut **ctx)
        .await
        .map_err(|_| DomainError::Storage)?;
        Ok(())
    }
}

pub async fn migrate_journals(pool: &PgPool) -> Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS listing (
            market TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            category TEXT NOT NULL,
            topic TEXT NOT NULL DEFAULT '',
            tag TEXT NOT NULL DEFAULT '',
            description TEXT NOT NULL DEFAULT '',
            event TEXT NOT NULL DEFAULT '',
            updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query("ALTER TABLE listing ADD COLUMN IF NOT EXISTS tags TEXT NOT NULL DEFAULT ''")
        .execute(pool)
        .await?;
    sqlx::query("UPDATE listing SET tags = category WHERE tags = '' AND category <> ''")
        .execute(pool)
        .await?;
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS catalog_tag (
            name TEXT PRIMARY KEY,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )
        "#,
    )
    .execute(pool)
    .await?;
    for name in DEFAULT_CATALOG_TAGS {
        sqlx::query("INSERT INTO catalog_tag (name) VALUES ($1) ON CONFLICT (name) DO NOTHING")
            .bind(*name)
            .execute(pool)
            .await?;
    }
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS fill_journal (
            owner TEXT NOT NULL,
            market TEXT NOT NULL,
            set_hash TEXT NOT NULL,
            kind TEXT NOT NULL,
            mask TEXT NOT NULL DEFAULT '',
            skellam_kind SMALLINT,
            a BIGINT,
            b BIGINT,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            PRIMARY KEY (owner, market, set_hash)
        )
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS market_comment (
            id BIGSERIAL PRIMARY KEY,
            market TEXT NOT NULL,
            author TEXT NOT NULL,
            body TEXT NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS market_comment_market_created ON market_comment (market, created_at DESC)",
    )
    .execute(pool)
    .await?;
    sqlx::query("ALTER TABLE listing ADD COLUMN IF NOT EXISTS blocked_regions TEXT NOT NULL DEFAULT ''")
        .execute(pool)
        .await?;
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS listing_application (
            id BIGSERIAL PRIMARY KEY,
            applicant TEXT NOT NULL,
            family SMALLINT NOT NULL,
            title TEXT NOT NULL,
            tags TEXT NOT NULL,
            event TEXT NOT NULL,
            description TEXT NOT NULL,
            topic TEXT NOT NULL DEFAULT '',
            tag TEXT NOT NULL DEFAULT '',
            blocked_regions TEXT NOT NULL DEFAULT '',
            dup_key TEXT NOT NULL,
            status SMALLINT NOT NULL,
            reviewer TEXT NOT NULL DEFAULT '',
            reason TEXT NOT NULL DEFAULT '',
            created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            reviewed_at TIMESTAMPTZ
        )
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS listing_application_dup ON listing_application (dup_key, status)")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE listing_application ADD COLUMN IF NOT EXISTS compose_json TEXT NOT NULL DEFAULT '{}'")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE listing_application ADD COLUMN IF NOT EXISTS market TEXT NOT NULL DEFAULT ''")
        .execute(pool)
        .await?;
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS review_log (
            id BIGSERIAL PRIMARY KEY,
            application_id BIGINT NOT NULL,
            reviewer TEXT NOT NULL,
            action TEXT NOT NULL,
            reason TEXT NOT NULL DEFAULT '',
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )
        "#,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn load_journals(pool: &PgPool, mem: &MemoryStore) -> Result<()> {
    let listings: Vec<(String, String, String, String, String, String, String, String)> = sqlx::query_as(
        "SELECT market, title, category, tags, topic, tag, description, event FROM listing",
    )
    .fetch_all(pool)
    .await?;
    for (market, title, category, tags, topic, tag, description, event) in listings {
        if title.trim().is_empty() {
            continue;
        }
        let tags = decode_tags(&tags, &category);
        mem.set_listing(
            &market,
            ListingMeta {
                title,
                category: tags.first().cloned().unwrap_or(category),
                tags,
                topic,
                tag,
                description,
                event,
                blocked_regions: Vec::new(),
            },
        );
    }
    let catalog: Vec<(String,)> = sqlx::query_as("SELECT name FROM catalog_tag").fetch_all(pool).await?;
    let _ = mem.ensure_catalog_tags(catalog.into_iter().map(|(n,)| n));
    let fills: Vec<(
        String,
        String,
        String,
        String,
        String,
        Option<i16>,
        Option<i64>,
        Option<i64>,
    )> = sqlx::query_as(
        "SELECT owner, market, set_hash, kind, mask, skellam_kind, a, b FROM fill_journal",
    )
    .fetch_all(pool)
    .await?;
    for (owner, market, set_hash, kind, mask, skellam_kind, a, b) in fills {
        mem.set_fill(FillMeta {
            owner,
            market,
            set_hash,
            kind,
            mask,
            skellam_kind: skellam_kind.and_then(|v| u8::try_from(v).ok()),
            a,
            b,
        });
    }
    let comments: Vec<(i64, String, String, String, i64)> = sqlx::query_as(
        r#"
        SELECT id, market, author, body, (EXTRACT(EPOCH FROM created_at))::bigint
        FROM market_comment
        ORDER BY id ASC
        "#,
    )
    .fetch_all(pool)
    .await?;
    for (id, market, author, body, created_at) in comments {
        mem.push_comment(CommentRow {
            id,
            market,
            author,
            body,
            created_at,
        });
    }
    let apps = sqlx::query(
        r#"
        SELECT id, applicant, family, title, tags, event, description, topic, tag, blocked_regions, dup_key, status,
               reviewer, reason, (EXTRACT(EPOCH FROM created_at))::bigint AS created_epoch,
               (EXTRACT(EPOCH FROM reviewed_at))::bigint AS reviewed_epoch, compose_json, market
        FROM listing_application
        ORDER BY id ASC
        "#,
    )
    .fetch_all(pool)
    .await?;
    for r in apps {
        let family: i16 = r.try_get("family")?;
        let status: i16 = r.try_get("status")?;
        let tags: String = r.try_get("tags")?;
        let regions: String = r.try_get("blocked_regions")?;
        let reviewed_at: Option<i64> = r.try_get("reviewed_epoch")?;
        mem.push_application(ApplicationRow {
            id: r.try_get("id")?,
            applicant: r.try_get("applicant")?,
            family: u8::try_from(family).unwrap_or(0),
            title: r.try_get("title")?,
            tags: decode_tags(&tags, ""),
            event: r.try_get("event")?,
            description: r.try_get("description")?,
            topic: r.try_get("topic")?,
            tag: r.try_get("tag")?,
            blocked_regions: decode_tags(&regions, ""),
            dup_key: r.try_get("dup_key")?,
            status: u8::try_from(status).unwrap_or(0),
            reviewer: r.try_get("reviewer")?,
            reason: r.try_get("reason")?,
            created_at: r.try_get("created_epoch")?,
            reviewed_at: reviewed_at.unwrap_or(0),
            compose_json: r.try_get("compose_json")?,
            market: r.try_get("market")?,
        });
    }
    let logs: Vec<(i64, i64, String, String, String, i64)> = sqlx::query_as(
        r#"
        SELECT id, application_id, reviewer, action, reason, (EXTRACT(EPOCH FROM created_at))::bigint
        FROM review_log
        ORDER BY id ASC
        "#,
    )
    .fetch_all(pool)
    .await?;
    for (id, application_id, reviewer, action, reason, created_at) in logs {
        mem.push_review_log(ReviewLogRow {
            id,
            application_id,
            reviewer,
            action,
            reason,
            created_at,
        });
    }
    Ok(())
}

/// Copy in-memory / JSON journals into PG so a first connect does not drop names.
pub async fn seed_journals_from_memory(pool: &PgPool, mem: &MemoryStore) -> Result<()> {
    let listings = ListingRepositoryImpl;
    let fills = FillJournalRepositoryImpl;
    let tags = CatalogTagRepositoryImpl;
    for row in mem.catalog_tags() {
        let Ok(tag) = CatalogTag::new(&row.name) else {
            continue;
        };
        let mut tx = pool.begin().await?;
        tags.save(&mut tx, &tag).await.map_err(|_| anyhow::anyhow!("tag seed"))?;
        tx.commit().await?;
    }
    for (market, meta) in mem.listings() {
        let tags = meta.resolved_tags();
        let Ok(row) = Listing::new(
            market,
            meta.title,
            tags,
            meta.topic,
            meta.tag,
            meta.description,
            meta.event,
        ) else {
            continue;
        };
        let mut tx = pool.begin().await?;
        listings.save(&mut tx, &row).await.map_err(|_| anyhow::anyhow!("listing seed"))?;
        tx.commit().await?;
    }
    for meta in mem.fills() {
        let Ok(mut row) = Fill::new(&meta.owner, &meta.market) else {
            continue;
        };
        row.set_hash = meta.set_hash;
        row.kind = meta.kind;
        row.mask = meta.mask;
        row.skellam_kind = meta.skellam_kind;
        row.a = meta.a;
        row.b = meta.b;
        if row.set_hash.is_empty() {
            continue;
        }
        let mut tx = pool.begin().await?;
        fills.save(&mut tx, &row).await.map_err(|_| anyhow::anyhow!("fill seed"))?;
        tx.commit().await?;
    }
    Ok(())
}
