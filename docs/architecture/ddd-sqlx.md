# DDD + sqlx persistence — application software architecture

**Continuous PDF Prediction Market — read-path and catalog services**

| Item | Content |
| --- | --- |
| Status | Locked application architecture |
| Normative for | `crates/services/readpath` (Market API / Indexer), later Gateway query sides |
| Stack | Rust, Axum (HTTP), **sqlx → PostgreSQL 16**, DDD layers |
| Transactions | Domain **associated type** `Context`; Infrastructure binds `sqlx::Transaction` |
| Ledger | Still Solana (FR-DUR-04). Postgres is S2 projection + off-chain catalog/journal only |

This is the software architecture for **this application’s** Rust services that serve the Next.js desk. It does not replace on-chain programs.

---

## 1. What is in Postgres today vs what must be

| Data | Today (as shipped) | Target under this architecture | Ledger? |
| --- | --- | --- | --- |
| Book projection $\theta,E,p0$, slot, $C_R$, $\rho$, $L$, $C_P$, close | `market_proj` (full row, after settle/stats/taps) | Same table, written inside an Indexer unit of work | No — rebuild from chain / fill journal |
| Listing title / tags / event / description | Postgres `listing` (JSON file is import-only) | Postgres `listing` after review approve | No — off-chain identity (FR-UI-36) |
| Listing application + review | — | Postgres `listing_application` + `review_log` | No — SIWS apply / `R-REVIEW` (FR-UI-43–45); not the ledger |
| Board comments | Postgres `market_comment` (after the board is indexed) | Same table; 404 if `market_proj` has no row or not `OPEN` | No — off-chain thread (FR-UI-42); not settlement or $C_P$ |
| Fill journal $S$ (mask / Skellam kind) | Postgres `fill_journal` (JSON file is import-only) | Postgres `fill_journal` | No — claim helper; chain has `set_hash` only |
| Positions / quotes / layers / resolution / pool | `position_proj`, `quote_proj`, `layer_proj`, `resolution_proj`, `pool_proj` | Same tables, one transaction per Indexer slot | No |
| Vault `available` / `reserved` | L1 `UserVault` + ATA (`source=l1`) | Stay L1; PG may cache a **view** | **L1 is truth** |
| Fills / $\rho$ / Vault USDC | On-chain | On-chain | **Yes** |

`DATABASE_URL` defaults to the machine Postgres role created by `infra/local-pg.sql` (`postgres://cpm:cpm@127.0.0.1:5432/cpm`). Production and local test **SHALL** run Postgres. If `:5432` is already a local service, do not start Docker on that port. Memory is a cache: crash / restart reloads from PG. `ALLOW_MEMORY_ONLY=1` is emergency / unit-test only. Next.js **SHALL NOT** open Postgres.

---

## 2. Who talks to the database

```text
Next.js (apps/web)          packages/sdk
        │ HTTPS / WSS
        ▼
Market API / Indexer        crates/services/readpath   ← sqlx lives HERE
        │
        ▼
PostgreSQL 16
```

- **Web UI** = Next.js. It calls `/v1/*`. It does not embed sqlx, does not hold `DATABASE_URL`, and does not run migrations.
- **Web-facing backend** = Rust Axum Market API. This is the “web end” that uses sqlx.
- CLI / Keeper compose via `crates/client` and the chain. They do not write catalog rows except through the same Market API or a shared application service.

---

## 3. DDD layers (locked)

Each Rust service that owns Postgres is split so Domain stays free of sqlx / Axum.

```text
crates/services/readpath/
  domain/           entities, value objects, repository traits, DomainError
  application/      use-cases: begin tx, call repos, commit / rollback
  infrastructure/   sqlx pool, migrations, *RepositoryImpl, memory adapters for tests
  interface/        Axum routes (today: api.rs) — HTTP in, command/query out
```

| Layer | May depend on | Must not depend on |
| --- | --- | --- |
| Domain | Standard library, `crates/math` types if they are pure | sqlx, Axum, Redis, Solana RPC |
| Application | Domain traits, a port for `begin` / `commit` | SQL strings, HTTP extractors |
| Infrastructure | Domain, sqlx, RPC, files (dev only) | Axum handlers |
| Interface | Application, Domain DTOs | SQL |

Hot-path **compose / fill** stays `crates/client` + chain. Application services here are **catalog, journal, and projections** — not a second matching engine.

---

## 4. Transactions via associated types

Domain defines a **context** without naming a database. Infrastructure binds the real sqlx transaction. Application starts and commits that transaction.

Generic associated type (GAT) is the production form so the transaction is not forced to `'static`:

```rust
// domain — pure business, no sqlx
#[async_trait]
pub trait ListingRepository {
    type Context<'c>;

    async fn save(
        &self,
        ctx: &mut Self::Context<'_>,
        listing: &Listing,
    ) -> Result<(), DomainError>;

    async fn get(
        &self,
        ctx: &mut Self::Context<'_>,
        market: &MarketId,
    ) -> Result<Option<Listing>, DomainError>;
}

#[async_trait]
pub trait FillJournalRepository {
    type Context<'c>;

    async fn save(
        &self,
        ctx: &mut Self::Context<'_>,
        fill: &FillMeta,
    ) -> Result<(), DomainError>;
}
```

Pedagogical binding (same idea as `OrderRepository::Context = sqlx::Transaction<'static, Postgres>`):

```rust
// infrastructure
pub struct ListingRepositoryImpl;

#[async_trait]
impl ListingRepository for ListingRepositoryImpl {
    type Context<'c> = sqlx::Transaction<'c, sqlx::Postgres>;

    async fn save(
        &self,
        ctx: &mut Self::Context<'_>,
        listing: &Listing,
    ) -> Result<(), DomainError> {
        sqlx::query!(
            r#"
            INSERT INTO listing (market, title, category, tags, topic, tag, description, event)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (market) DO UPDATE SET
                title = EXCLUDED.title,
                category = EXCLUDED.category,
                tags = EXCLUDED.tags,
                description = EXCLUDED.description,
                event = EXCLUDED.event
            "#,
            listing.market.as_str(),
            listing.title.as_str(),
            listing.category.as_str(),
            encode_tags(&listing.tags),
            listing.topic.as_str(),
            listing.tag.as_str(),
            listing.description.as_str(),
            listing.event.as_str(),
        )
        .execute(&mut **ctx)
        .await
        .map_err(|_| DomainError::Storage)?;
        Ok(())
    }
}
```

Application orchestrates one unit of work:

```rust
// application
impl CatalogService<R: ListingRepository<Context<'c> = sqlx::Transaction<'c, sqlx::Postgres>>> {
    pub async fn put_listing(&self, cmd: PutListing) -> Result<(), AppError> {
        let mut tx = self.pool.begin().await?;
        let listing = Listing::new(cmd)?;
        self.listings.save(&mut tx, &listing).await?;
        tx.commit().await?;
        Ok(())
    }
}
```

Rules:

1. Domain traits never mention `sqlx`, `PgPool`, or table names.
2. One Application use-case that must be atomic (e.g. Indexer writing `market_proj` + `position` + `quote` for the same slot) **SHALL** share one `Context` (one transaction).
3. Tests bind `Context` to an in-memory handle (or `sqlx::Transaction` against a test database). Do not put SQL in Domain tests.
4. Raw `sqlx::query` in Axum handlers is forbidden once the repository exists.
5. Parameterized queries only (system architecture §12.6).

---

## 5. Bounded contexts and tables

| Context | Aggregate / journal | Tables (target) | Writer |
| --- | --- | --- | --- |
| Catalog | `Listing` | `listing` | Market API `POST /v1/listings` |
| Catalog | `CatalogTag` | `catalog_tag` | Market API `POST/DELETE /v1/tags` |
| Ticket journal | `FillMeta` | `fill_journal` | compose `buy_*` + `POST /v1/tickets` |
| Book projection | `MarketBook` | `market_proj`, `position_proj`, `quote_proj`, `layer_proj`, `resolution_proj`, `pool_proj` | Indexer |
| Ops view | none (query) | same projection tables | read-only |

`market_proj`, `listing`, and `fill_journal` are created on boot (`open_required_pool`). JSON files are imported once if present, then PG is the store.

Suggested DDL (target; migrate with sqlx):

```sql
CREATE TABLE IF NOT EXISTS listing (
    market      TEXT PRIMARY KEY,
    title       TEXT NOT NULL,
    category    TEXT NOT NULL,
    tags        TEXT NOT NULL DEFAULT '',
    topic       TEXT NOT NULL DEFAULT '',
    tag         TEXT NOT NULL DEFAULT '',
    description TEXT NOT NULL DEFAULT '',
    event       TEXT NOT NULL DEFAULT '',
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS catalog_tag (
    name        TEXT PRIMARY KEY,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS fill_journal (
    owner       TEXT NOT NULL,
    market      TEXT NOT NULL,
    set_hash    TEXT NOT NULL,
    kind        TEXT NOT NULL,
    mask        TEXT NOT NULL DEFAULT '',
    skellam_kind SMALLINT,
    a           BIGINT,
    b           BIGINT,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (owner, market, set_hash)
);
```

---

## 6. Read path after persist

1. Interface (Axum) maps HTTP → Application command / query.
2. Application opens `pool.begin()`, calls Domain via repository traits, `commit`.
3. After commit, the in-process `MemoryStore` MAY be updated so WS / quote stay hot (cache, not ledger).
4. Next.js keeps calling the same `/v1/listings`, `/v1/tickets`, `/v1/markets` — no schema knowledge.

Indexer: one slot’s upserts of book + positions **in one transaction**. A half-written slot is a Domain error, not a silent memory/PG split.

---

## 7. What this architecture does not do

- Postgres is not $C_{\max}$, $\rho$, or Vault balances (FR-DUR-04).
- sqlx is not used from `apps/web`.
- Application services do not `buy_set` off-chain.
- Domain does not import `solana_client` (Indexer ports live as Application/Infrastructure adapters).

---

## 8. Related documents

- `technical-architecture.md` §3–4 — stack; this file is the DDD/sqlx scheme
- `system-architecture.md` §10 — S2 projection; recovery still L1 → journal → rebuild PG
- SRS FR-DUR-04, FR-UI-36, FR-UI-41, IR-07
