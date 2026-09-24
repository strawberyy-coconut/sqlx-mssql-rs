//! End-to-end walkthrough of the `sqlx-mssql-rs` driver.
//!
//! Run it against a SQL Server reached by `MSSQL_DATABASE_URL` (falling back to
//! `DATABASE_URL`):
//!
//! ```text
//! cargo run -p full
//! ```
//!
//! Note that the compile-time checked macros (`query!`, `query_as!`,
//! `query_scalar!`) need `DATABASE_URL` set while *compiling*, since they
//! describe the query against the live server. Set `SQLX_OFFLINE=true` and run
//! `cargo sqlx prepare` to compile without a database.

use chrono::{DateTime, Utc};
use sqlx_core::row::Row as _;
use sqlx_mssql_rs::{FromRow, MssqlPoolOptions, MssqlRow};
use uuid::Uuid;

/// Maps a row onto a named struct via `#[derive(FromRow)]`.
#[derive(Debug, FromRow)]
struct UserDescription {
    description: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("MSSQL_DATABASE_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .expect("set MSSQL_DATABASE_URL or DATABASE_URL");

    // 1. Connect through a pool.
    let pool = MssqlPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await?;
    println!("1. connected");

    // 2. Apply migrations from ./migrations.
    sqlx::migrate!("./migrations").run(&pool).await?;
    println!("2. migrations applied");

    // 3. Insert with a runtime query and bound parameters.
    let id = Uuid::now_v7();
    let now: DateTime<Utc> = Utc::now();

    let affected = sqlx::query("INSERT INTO users (id, description, add_date) VALUES (?, ?, ?)")
        .bind(id)
        .bind("runtime insert")
        .bind(now)
        .execute(&pool)
        .await?
        .rows_affected();
    println!("3. inserted {affected} row(s)");

    // 4. Fetch it back, reading columns by name.
    let row: MssqlRow = sqlx::query("SELECT id, description, add_date FROM users WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await?;

    let read_id: Uuid = row.try_get("id")?;
    let read_description: Option<String> = row.try_get("description")?;
    let read_date: DateTime<Utc> = row.try_get("add_date")?;

    assert_eq!(read_id, id);
    assert_eq!(read_description.as_deref(), Some("runtime insert"));
    // `datetimeoffset` round-trips at 100 ns resolution, which is the finest
    // TDS stores, so compare after truncating to that.
    let truncated = |value: DateTime<Utc>| {
        DateTime::from_timestamp_nanos(value.timestamp_nanos_opt().expect("in range") / 100 * 100)
    };
    assert_eq!(truncated(read_date), truncated(now));
    println!("4. read back: {read_id} {read_description:?} {read_date}");

    // 5. A compile-time checked query: verified against the schema at build time.
    let rows = sqlx_mssql_rs::query!("SELECT description FROM users")
        .fetch_all(&pool)
        .await?;
    println!("5. compile-time checked query returned {} row(s)", rows.len());

    // 6. Map rows into a named struct.
    let described = sqlx_mssql_rs::query_as!(
        UserDescription,
        "SELECT description FROM users WHERE id = ?",
        id
    )
    .fetch_one(&pool)
    .await?;
    println!("6. query_as! -> {:?}", described.description);

    // 7. A single scalar value with compile-time checking.
    let count = sqlx_mssql_rs::query_scalar!("SELECT COUNT(*) FROM users")
        .fetch_one(&pool)
        .await?;
    println!("7. query_scalar! -> {count:?}");

    // 8. A committed transaction.
    let mut transaction = pool.begin().await?;
    sqlx::query("INSERT INTO users (id, description, add_date) VALUES (?, ?, ?)")
        .bind(Uuid::now_v7())
        .bind("committed")
        .bind(Utc::now())
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    println!("8. committed a transaction");

    // 9. A rolled-back transaction leaves no trace.
    let mut transaction = pool.begin().await?;
    sqlx::query("INSERT INTO users (id, description, add_date) VALUES (?, ?, ?)")
        .bind(Uuid::now_v7())
        .bind("rolled back")
        .bind(Utc::now())
        .execute(&mut *transaction)
        .await?;
    transaction.rollback().await?;

    let after_rollback =
        sqlx_mssql_rs::query_scalar!("SELECT COUNT(*) FROM users WHERE description = ?", "rolled back")
            .fetch_one(&pool)
            .await?;
    assert_eq!(after_rollback, Some(0), "rolled back row should not exist");
    println!("9. rollback left no row");

    // 10. Clean up.
    sqlx::query("DELETE FROM users").execute(&pool).await?;
    let remaining = sqlx_mssql_rs::query_scalar!("SELECT COUNT(*) FROM users")
        .fetch_one(&pool)
        .await?;
    assert_eq!(remaining, Some(0));
    println!("10. cleaned up");

    pool.close().await;
    println!("done");

    Ok(())
}
