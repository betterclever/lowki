//! Database access through sqlx's `Any` driver: SQLite by default, Postgres when
//! `LOWKI_DATABASE_URL` starts with `postgres://`.

use anyhow::Context;
use sqlx::{AnyPool, any::AnyPoolOptions};

pub type Db = AnyPool;

const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../migrations/0001_init.sql")),
    (2, include_str!("../migrations/0002_ens_gateway.sql")),
    (3, include_str!("../migrations/0003_home.sql")),
];

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub async fn connect(url: &str) -> anyhow::Result<Db> {
    sqlx::any::install_default_drivers();
    let is_sqlite = url.starts_with("sqlite:");
    let pool = AnyPoolOptions::new()
        .max_connections(if is_sqlite { 4 } else { 16 })
        .after_connect(move |conn, _| {
            Box::pin(async move {
                if is_sqlite {
                    sqlx::query("PRAGMA foreign_keys = ON").execute(&mut *conn).await?;
                    sqlx::query("PRAGMA busy_timeout = 5000").execute(&mut *conn).await?;
                }
                Ok(())
            })
        })
        .connect(url)
        .await
        .context("connecting to the database")?;
    if is_sqlite {
        sqlx::query("PRAGMA journal_mode = WAL").execute(&pool).await.ok();
    }
    migrate(&pool).await?;
    Ok(pool)
}

async fn migrate(pool: &Db) -> anyhow::Result<()> {
    // Serialize migrations across replicas starting at the same time (Postgres only; a SQLite
    // file has a single writer anyway).
    let mut conn = pool.acquire().await?;
    let is_pg = conn.backend_name().eq_ignore_ascii_case("postgresql");
    if is_pg {
        sqlx::query("SELECT pg_advisory_lock(7710001)")
            .execute(&mut *conn)
            .await?;
    }
    let res = migrate_inner(pool).await;
    if is_pg {
        sqlx::query("SELECT pg_advisory_unlock(7710001)")
            .execute(&mut *conn)
            .await?;
    }
    res
}

async fn migrate_inner(pool: &Db) -> anyhow::Result<()> {
    sqlx::query("CREATE TABLE IF NOT EXISTS lowki_schema (version BIGINT PRIMARY KEY, applied_at BIGINT NOT NULL)")
        .execute(pool)
        .await?;
    for (version, sql) in MIGRATIONS {
        let done: Option<(i64,)> = sqlx::query_as("SELECT version FROM lowki_schema WHERE version = $1")
            .bind(*version)
            .fetch_optional(pool)
            .await?;
        if done.is_some() {
            continue;
        }
        let mut tx = pool.begin().await?;
        for stmt in split_sql(sql) {
            sqlx::query(&stmt)
                .execute(&mut *tx)
                .await
                .with_context(|| format!("migration {version}"))?;
        }
        sqlx::query("INSERT INTO lowki_schema (version, applied_at) VALUES ($1, $2)")
            .bind(*version)
            .bind(now())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        tracing::info!(version, "applied migration");
    }
    Ok(())
}

fn split_sql(sql: &str) -> Vec<String> {
    let without_comments: String = sql
        .lines()
        .map(|l| l.split("--").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    without_comments
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}
