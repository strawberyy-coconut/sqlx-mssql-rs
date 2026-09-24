//! Integration tests for the MSSQL driver.
//!
//! These run against a live SQL Server. `DATABASE_URL` selects it:
//!
//! ```text
//! mssql://sa:Password1!@localhost:1433/master?trust_certificate=true
//! ```
//!
//! Tables are created with unique names and dropped again, so the suite can be
//! run against a shared server.

use std::sync::atomic::{AtomicU64, Ordering};

use futures_util::StreamExt;
use sqlx_core::Either;
use sqlx_core::column::Column;
use sqlx_core::connection::Connection;
use sqlx_core::executor::Executor;
use sqlx_core::query::query;
use sqlx_core::query_scalar::query_scalar;
use sqlx_core::row::Row;
use sqlx_core::sql_str::{AssertSqlSafe, SqlSafeStr};
use sqlx_core::statement::Statement;

use sqlx_mssql_rs::{MssqlConnectOptions, MssqlConnection, MssqlEncryption, MssqlPool};

fn database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "mssql://sa:Password1!@localhost:1433/master?trust_certificate=true".to_owned()
    })
}

async fn get_test_conn() -> MssqlConnection {
    MssqlConnection::connect(&database_url())
        .await
        .expect("failed to connect to SQL Server")
}

/// Marks dynamically built SQL as reviewed.
///
/// SQLx requires an explicit opt-in for non-literal SQL so that string
/// interpolation is a deliberate choice. The only interpolated values in this
/// file are locally generated table names.
fn dynamic(sql: impl Into<String>) -> AssertSqlSafe<String> {
    AssertSqlSafe(sql.into())
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Builds a table name that cannot collide with another test run.
fn test_table_name(prefix: &str) -> String {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("{prefix}_{}_{}", std::process::id(), id)
}

async fn create_table(conn: &mut MssqlConnection, name: &str, definition: &str) {
    conn.execute(dynamic(format!("CREATE TABLE [{name}] ({definition})")))
        .await
        .expect("failed to create test table");
}

async fn drop_table(conn: &mut MssqlConnection, name: &str) {
    let _ = conn
        .execute(dynamic(format!("DROP TABLE IF EXISTS [{name}]")))
        .await;
}

#[tokio::test]
async fn pool_acquires_and_queries() {
    let pool = MssqlPool::connect(&database_url())
        .await
        .expect("failed to create pool");

    let value: i32 = query_scalar("SELECT 1")
        .fetch_one(&pool)
        .await
        .expect("query failed");
    assert_eq!(value, 1);

    pool.close().await;
}

#[tokio::test]
async fn fetches_basic_row_and_reads_columns_by_name() {
    let mut conn = get_test_conn().await;

    let row = query("SELECT 1 AS one, N'hello' AS greeting")
        .fetch_one(&mut conn)
        .await
        .expect("query failed");

    assert_eq!(row.try_get::<i32, _>("one").unwrap(), 1);
    // Column lookup is case-insensitive.
    assert_eq!(row.try_get::<String, _>("GREETING").unwrap(), "hello");

    conn.close().await.unwrap();
}

#[tokio::test]
async fn streams_multiple_rows() {
    let mut conn = get_test_conn().await;

    let rows = query("SELECT value FROM (VALUES (1), (2), (3)) AS t(value)")
        .fetch_all(&mut conn)
        .await
        .expect("query failed");

    assert_eq!(rows.len(), 3);
    let values: Vec<i32> = rows
        .iter()
        .map(|row| row.try_get::<i32, _>(0).unwrap())
        .collect();
    assert_eq!(values, vec![1, 2, 3]);

    conn.close().await.unwrap();
}

#[tokio::test]
async fn fetch_many_emits_rows_then_a_query_result() {
    let mut conn = get_test_conn().await;

    let mut rows = 0;
    let mut results = 0;

    {
        let mut stream = (&mut conn).fetch_many(query("SELECT 1 AS v, 2 AS w"));

        while let Some(item) = stream.next().await {
            match item.expect("stream failed") {
                Either::Right(_) => rows += 1,
                Either::Left(_) => results += 1,
            }
        }
    }

    assert_eq!(rows, 1, "expected one row");
    assert_eq!(results, 1, "expected one query result");

    conn.close().await.unwrap();
}

#[tokio::test]
async fn fetch_optional_returns_none_for_empty_result() {
    let mut conn = get_test_conn().await;

    let row = query("SELECT 1 AS v WHERE 1 = 0")
        .fetch_optional(&mut conn)
        .await
        .expect("query failed");

    assert!(row.is_none());

    conn.close().await.unwrap();
}

#[tokio::test]
async fn binds_positional_parameters() {
    let mut conn = get_test_conn().await;

    let value: i32 = query_scalar("SELECT ?")
        .bind(42i32)
        .fetch_one(&mut conn)
        .await
        .expect("query failed");
    assert_eq!(value, 42);

    conn.close().await.unwrap();
}

/// `BString`/`BStr` support comes from generic impls in `sqlx-core` that
/// delegate to the driver's `Vec<u8>`/`[u8]` impls, so this checks the wiring
/// actually reaches them.
#[cfg(feature = "bstr")]
#[tokio::test]
async fn binds_and_reads_bstr_values() {
    use sqlx_core::types::bstr::BString;

    let mut conn = get_test_conn().await;

    // Not valid UTF-8, so it cannot round-trip through a text path.
    let value = BString::from(&b"binary\xffdata"[..]);

    let read: BString = query_scalar("SELECT ?")
        .bind(value.clone())
        .fetch_one(&mut conn)
        .await
        .expect("bstr bind failed");

    assert_eq!(read, value);

    conn.close().await.unwrap();
}

#[tokio::test]
async fn binds_heterogeneous_and_null_parameters() {
    let mut conn = get_test_conn().await;

    let text: String = query_scalar("SELECT ?")
        .bind("hello".to_string())
        .fetch_one(&mut conn)
        .await
        .expect("text bind failed");
    assert_eq!(text, "hello");

    let flag: bool = query_scalar("SELECT ?")
        .bind(true)
        .fetch_one(&mut conn)
        .await
        .expect("bool bind failed");
    assert!(flag);

    let number: f64 = query_scalar("SELECT ?")
        .bind(1.5f64)
        .fetch_one(&mut conn)
        .await
        .expect("float bind failed");
    assert_eq!(number, 1.5);

    let nothing: Option<i32> = query_scalar("SELECT ?")
        .bind(Option::<i32>::None)
        .fetch_one(&mut conn)
        .await
        .expect("null bind failed");
    assert!(nothing.is_none());

    conn.close().await.unwrap();
}

#[tokio::test]
async fn execute_reports_rows_affected() {
    let mut conn = get_test_conn().await;
    let table = test_table_name("rows_affected");

    create_table(&mut conn, &table, "id INT").await;

    let result = conn
        .execute(dynamic(format!(
            "INSERT INTO [{table}] (id) VALUES (1), (2), (3)"
        )))
        .await
        .expect("insert failed");
    assert_eq!(result.rows_affected(), 3);

    drop_table(&mut conn, &table).await;
    conn.close().await.unwrap();
}

#[tokio::test]
async fn transaction_commit_persists_and_rollback_discards() {
    let mut conn = get_test_conn().await;
    let table = test_table_name("txn");

    create_table(&mut conn, &table, "id INT").await;

    // Commit.
    let mut tx = conn.begin().await.expect("begin failed");
    query(dynamic(format!("INSERT INTO [{table}] (id) VALUES (10)")))
        .execute(&mut *tx)
        .await
        .expect("insert failed");
    tx.commit().await.expect("commit failed");

    let count: i32 = query_scalar(dynamic(format!("SELECT COUNT(*) FROM [{table}]")))
        .fetch_one(&mut conn)
        .await
        .expect("count failed");
    assert_eq!(count, 1, "committed row should be visible");

    // Roll back.
    let mut tx = conn.begin().await.expect("begin failed");
    query(dynamic(format!("INSERT INTO [{table}] (id) VALUES (20)")))
        .execute(&mut *tx)
        .await
        .expect("insert failed");
    tx.rollback().await.expect("rollback failed");

    let count: i32 = query_scalar(dynamic(format!("SELECT COUNT(*) FROM [{table}]")))
        .fetch_one(&mut conn)
        .await
        .expect("count failed");
    assert_eq!(count, 1, "rolled back row should not be visible");

    drop_table(&mut conn, &table).await;
    conn.close().await.unwrap();
}

#[tokio::test]
async fn dropped_transaction_is_rolled_back() {
    let mut conn = get_test_conn().await;
    let table = test_table_name("dropped_txn");

    create_table(&mut conn, &table, "id INT").await;

    {
        let mut tx = conn.begin().await.expect("begin failed");
        query(dynamic(format!("INSERT INTO [{table}] (id) VALUES (30)")))
            .execute(&mut *tx)
            .await
            .expect("insert failed");
        // `tx` is dropped here without committing.
    }

    // The rollback is issued lazily, so this query also proves it happened.
    let count: i32 = query_scalar(dynamic(format!("SELECT COUNT(*) FROM [{table}]")))
        .fetch_one(&mut conn)
        .await
        .expect("count failed");
    assert_eq!(count, 0, "abandoned transaction should have rolled back");

    drop_table(&mut conn, &table).await;
    conn.close().await.unwrap();
}

#[tokio::test]
async fn prepare_reports_column_metadata() {
    let mut conn = get_test_conn().await;

    let statement = conn
        .prepare(AssertSqlSafe("SELECT 1 AS one, N'x' AS two").into_sql_str())
        .await
        .expect("prepare failed");

    let columns = statement.columns();
    assert_eq!(columns.len(), 2);
    assert_eq!(columns[0].name(), "one");
    assert_eq!(columns[1].name(), "two");

    conn.close().await.unwrap();
}

/// Parameter types reach sqlx when they resolve to exactly one Rust type, and
/// fall back to a count when they do not.
#[tokio::test]
async fn prepare_reports_parameter_types_when_they_are_unambiguous() {
    let mut conn = get_test_conn().await;
    let table = test_table_name("param_types");

    create_table(
        &mut conn,
        &table,
        "id INT, name NVARCHAR(50), amount DECIMAL(18,2)",
    )
    .await;

    // Integer and character parameters have a single possible Rust type.
    let statement = conn
        .prepare(
            dynamic(format!(
                "SELECT id FROM [{table}] WHERE name = ? AND id = ?"
            ))
            .into_sql_str(),
        )
        .await
        .expect("prepare failed");

    match statement.parameters() {
        Some(Either::Left(types)) => {
            assert_eq!(types.len(), 2, "one entry per parameter marker");
            assert_eq!(types[0].type_name(), "nvarchar");
            assert_eq!(types[1].type_name(), "int");
        }
        other => panic!("expected per-parameter types, got {other:?}"),
    }

    // `i8` accepts any numeric type, so a decimal would be reported as `i8` and
    // then reject a correctly bound argument. A count is the safe answer.
    let statement = conn
        .prepare(dynamic(format!("SELECT id FROM [{table}] WHERE amount > ?")).into_sql_str())
        .await
        .expect("prepare failed");

    match statement.parameters() {
        Some(Either::Right(count)) => assert_eq!(count, 1),
        other => panic!("expected a parameter count for decimal, got {other:?}"),
    }

    drop_table(&mut conn, &table).await;
    conn.close().await.unwrap();
}

/// Integer columns must resolve to the Rust type of matching width rather than
/// to the first `compatible` entry. `sp_describe_first_result_set` reports a
/// precision and scale for every numeric type, which used to break the exact
/// match and make `bigint` infer as `i8`.
#[tokio::test]
async fn integer_columns_infer_as_their_own_type() {
    use sqlx_core::config::macros::PreferredCrates;
    use sqlx_core::type_checking::TypeChecking;

    let mut conn = get_test_conn().await;

    let statement = conn
        .prepare(
            AssertSqlSafe(
                "SELECT CAST(1 AS tinyint) AS t, CAST(1 AS smallint) AS s, \
                 CAST(1 AS int) AS i, CAST(1 AS bigint) AS b",
            )
            .into_sql_str(),
        )
        .await
        .expect("prepare failed");

    let preferred = PreferredCrates::default();
    let inferred: Vec<&str> = statement
        .columns()
        .iter()
        .map(|column| {
            <sqlx_mssql_rs::Mssql as TypeChecking>::return_type_for_id(
                column.type_info(),
                &preferred,
            )
            .expect("every integer column should map to a Rust type")
        })
        .collect();

    assert_eq!(inferred, vec!["i8", "i16", "i32", "i64"]);

    conn.close().await.unwrap();
}

#[tokio::test]
async fn invalid_query_is_reported_as_a_database_error() {
    let mut conn = get_test_conn().await;

    let error = query("SELECT * FROM nope_this_table_does_not_exist")
        .fetch_all(&mut conn)
        .await
        .expect_err("query should have failed");

    assert!(
        error.as_database_error().is_some(),
        "expected a database error, got {error:?}"
    );
    let database_error = error
        .as_database_error()
        .expect("expected a database error");
    assert_eq!(
        database_error.code().as_deref(),
        Some("208"),
        "invalid object name should surface SQL Server error 208"
    );

    conn.close().await.unwrap();
}

#[tokio::test]
async fn wrong_parameter_count_errors() {
    let mut conn = get_test_conn().await;

    // The result type is annotated because the value itself is never used.
    let result: Result<i32, _> = query_scalar("SELECT ?, ?")
        .bind(1i32)
        .fetch_one(&mut conn)
        .await;
    let error = result.expect_err("mismatched parameter count should fail");

    assert!(
        error.to_string().contains("parameter marker"),
        "expected a parameter mismatch error, got {error:?}"
    );

    conn.close().await.unwrap();
}

/// Dropping a row stream early leaves the result set half-read; the connection
/// must recover rather than become unusable.
#[tokio::test]
async fn early_dropped_stream_leaves_connection_usable() {
    let mut conn = get_test_conn().await;
    let table = test_table_name("dropped_stream");

    create_table(&mut conn, &table, "id INT").await;
    let sql = format!("INSERT INTO [{table}] (id) VALUES (1), (2), (3)");
    conn.execute(dynamic(sql.clone()))
        .await
        .expect("insert failed");

    {
        let mut stream =
            (&mut conn).fetch_many(query(dynamic(format!("SELECT id FROM [{table}]"))));
        // Read exactly one row, then drop the stream.
        let _ = stream.next().await;
    }

    let value: i32 = query_scalar("SELECT 1")
        .fetch_one(&mut conn)
        .await
        .expect("connection should still be usable");
    assert_eq!(value, 1);

    drop_table(&mut conn, &table).await;
    conn.close().await.unwrap();
}

/// Temporal values need their type integration enabled.
#[cfg(feature = "chrono")]
#[tokio::test]
async fn binds_temporal_parameters() {
    use chrono::{NaiveDate, NaiveDateTime, TimeZone, Utc};

    let mut conn = get_test_conn().await;

    let naive: NaiveDateTime = NaiveDate::from_ymd_opt(2026, 9, 21)
        .unwrap()
        .and_hms_opt(12, 34, 56)
        .unwrap();

    let echoed: NaiveDateTime = query_scalar("SELECT ?")
        .bind(naive)
        .fetch_one(&mut conn)
        .await
        .expect("datetime2 bind failed");
    assert_eq!(echoed, naive);

    // A server-generated value guards against a symmetric encode/decode error.
    let literal: NaiveDateTime = query_scalar("SELECT CAST('2026-09-21T12:34:56' AS datetime2)")
        .fetch_one(&mut conn)
        .await
        .expect("literal decode failed");
    assert_eq!(literal, naive);

    let offset = Utc.with_ymd_and_hms(2026, 9, 21, 12, 34, 56).unwrap();

    let echoed: chrono::DateTime<Utc> = query_scalar("SELECT ?")
        .bind(offset)
        .fetch_one(&mut conn)
        .await
        .expect("datetimeoffset bind failed");
    assert_eq!(echoed, offset);

    // A non-zero offset is what actually exercises recovering the instant: the
    // stored time is already UTC, so the offset must not be applied a second
    // time.
    let offset_literal: chrono::DateTime<Utc> =
        query_scalar("SELECT CAST('2026-09-21T12:34:56+02:00' AS datetimeoffset)")
            .fetch_one(&mut conn)
            .await
            .expect("offset literal decode failed");
    assert_eq!(
        offset_literal,
        Utc.with_ymd_and_hms(2026, 9, 21, 10, 34, 56).unwrap()
    );

    conn.close().await.unwrap();
}

/// The `time` integration is entirely separate code from `chrono`, so it needs
/// its own round-trip rather than relying on the test above.
#[cfg(feature = "time")]
#[tokio::test]
async fn binds_time_crate_temporal_parameters() {
    use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time};

    let mut conn = get_test_conn().await;

    let date = Date::from_calendar_date(2026, Month::September, 21).unwrap();
    let time_of_day = Time::from_hms(12, 34, 56).unwrap();

    let echoed: Date = query_scalar("SELECT ?")
        .bind(date)
        .fetch_one(&mut conn)
        .await
        .expect("date bind failed");
    assert_eq!(echoed, date);

    let echoed: Time = query_scalar("SELECT ?")
        .bind(time_of_day)
        .fetch_one(&mut conn)
        .await
        .expect("time bind failed");
    assert_eq!(echoed, time_of_day);

    let primitive = PrimitiveDateTime::new(date, time_of_day);
    let echoed: PrimitiveDateTime = query_scalar("SELECT ?")
        .bind(primitive)
        .fetch_one(&mut conn)
        .await
        .expect("datetime2 bind failed");
    assert_eq!(echoed, primitive);

    // A server-generated value guards against a symmetric encode/decode error.
    let literal: PrimitiveDateTime =
        query_scalar("SELECT CAST('2026-09-21T12:34:56' AS datetime2)")
            .fetch_one(&mut conn)
            .await
            .expect("literal decode failed");
    assert_eq!(literal, primitive);

    let offset = OffsetDateTime::new_utc(date, time_of_day);
    let echoed: OffsetDateTime = query_scalar("SELECT ?")
        .bind(offset)
        .fetch_one(&mut conn)
        .await
        .expect("datetimeoffset bind failed");
    assert_eq!(echoed, offset);

    conn.close().await.unwrap();
}

/// The stricter encryption modes have to survive the trip into the client, so
/// connect with one rather than only inspecting the parsed options.
#[tokio::test]
async fn connects_with_required_encryption() {
    use sqlx_core::connection::ConnectOptions;

    let mut options: MssqlConnectOptions = database_url().parse().expect("URL should parse");
    options.encryption(MssqlEncryption::Required);

    let mut conn = options
        .connect()
        .await
        .expect("a server that supports encryption should accept `required`");

    let value: i32 = query_scalar("SELECT 1")
        .fetch_one(&mut conn)
        .await
        .expect("query failed");
    assert_eq!(value, 1);

    conn.close().await.unwrap();
}

/// The `jiff` integration is separate code again, and its day arithmetic is
/// derived from instants rather than a day-number API.
#[cfg(feature = "jiff")]
#[tokio::test]
async fn binds_jiff_temporal_parameters() {
    use jiff::Timestamp;
    use jiff::civil::{Date, DateTime, Time};
    use jiff::tz::TimeZone;

    let mut conn = get_test_conn().await;

    let date = Date::new(2026, 9, 21).unwrap();
    let time_of_day = Time::new(12, 34, 56, 0).unwrap();

    let echoed: Date = query_scalar("SELECT ?")
        .bind(date)
        .fetch_one(&mut conn)
        .await
        .expect("date bind failed");
    assert_eq!(echoed, date);

    let echoed: Time = query_scalar("SELECT ?")
        .bind(time_of_day)
        .fetch_one(&mut conn)
        .await
        .expect("time bind failed");
    assert_eq!(echoed, time_of_day);

    let datetime = DateTime::new(2026, 9, 21, 12, 34, 56, 0).unwrap();
    let echoed: DateTime = query_scalar("SELECT ?")
        .bind(datetime)
        .fetch_one(&mut conn)
        .await
        .expect("datetime2 bind failed");
    assert_eq!(echoed, datetime);

    // A server-generated value guards against a symmetric encode/decode error.
    let literal: DateTime = query_scalar("SELECT CAST('2026-09-21T12:34:56' AS datetime2)")
        .fetch_one(&mut conn)
        .await
        .expect("literal decode failed");
    assert_eq!(literal, datetime);

    let timestamp = datetime.to_zoned(TimeZone::UTC).unwrap().timestamp();
    let echoed: Timestamp = query_scalar("SELECT ?")
        .bind(timestamp)
        .fetch_one(&mut conn)
        .await
        .expect("datetimeoffset bind failed");
    assert_eq!(echoed, timestamp);

    // A non-zero offset is what actually exercises turning a wall clock plus an
    // offset back into an instant.
    let utc = DateTime::new(2026, 9, 21, 10, 34, 56, 0)
        .unwrap()
        .to_zoned(TimeZone::UTC)
        .unwrap()
        .timestamp();
    let offset_literal: Timestamp =
        query_scalar("SELECT CAST('2026-09-21T12:34:56+02:00' AS datetimeoffset)")
            .fetch_one(&mut conn)
            .await
            .expect("offset literal decode failed");
    assert_eq!(offset_literal, utc);

    conn.close().await.unwrap();
}

/// Captures `log` records so the statement-logging path can be asserted on.
struct CaptureLogger;

static CAPTURED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
static LOGGER: CaptureLogger = CaptureLogger;

impl log::Log for CaptureLogger {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        if let Ok(mut captured) = CAPTURED.lock() {
            captured.push(record.args().to_string());
        }
    }

    fn flush(&self) {}
}

/// The sqlx `ConnectOptions` logging methods must actually log; they are easy to
/// accept and silently ignore.
#[tokio::test]
async fn statement_logging_emits_the_query() {
    // One logger per process; a second call is harmless.
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Debug);

    let mut conn = get_test_conn().await;

    let marker = "logged_statement_marker";
    let _ = query(dynamic(format!("SELECT 1 AS {marker}")))
        .fetch_all(&mut conn)
        .await;

    let captured: Vec<String> = CAPTURED.lock().expect("log capture mutex poisoned").clone();
    assert!(
        captured.iter().any(|line| line.contains(marker)),
        "expected the statement to be logged, captured {captured:?}"
    );

    conn.close().await.unwrap();
}

#[tokio::test]
async fn migrations_apply_and_revert() {
    use sqlx_core::migrate::{Migrate, Migrator};

    let mut conn = get_test_conn().await;

    let _ = conn.execute("DROP TABLE IF EXISTS test_items").await;
    let _ = conn
        .execute("DROP TABLE IF EXISTS [test_sqlx_migrations]")
        .await;

    let mut migrator = Migrator::new(std::path::Path::new("./tests/migrations"))
        .await
        .expect("failed to load migrations");
    // A private table keeps this test from disturbing anything else that shares
    // the database, such as the example's own migration state.
    migrator.table_name = std::borrow::Cow::Borrowed("test_sqlx_migrations");

    migrator.run(&mut conn).await.expect("migration failed");

    // The bookkeeping row must be visible, otherwise a second run re-applies.
    let applied = conn
        .list_applied_migrations("test_sqlx_migrations")
        .await
        .expect("list_applied_migrations failed");
    assert_eq!(
        applied.len(),
        1,
        "list_applied_migrations should report the applied migration"
    );

    let count: i32 = query_scalar("SELECT COUNT(*) FROM test_items")
        .fetch_one(&mut conn)
        .await
        .expect("migrated table should be queryable");
    assert_eq!(count, 0);

    // Re-running is a no-op.
    migrator.run(&mut conn).await.expect("second run failed");

    // Leave no trace for a later run.
    let _ = conn.execute("DROP TABLE IF EXISTS test_items").await;
    let _ = conn
        .execute("DROP TABLE IF EXISTS [test_sqlx_migrations]")
        .await;

    conn.close().await.unwrap();
}

/// SQL Server's `geometry` and `geography` are UDTs whose payload is the
/// server's native serialization, not WKB. These tests run only with the
/// `spatial` feature, which supplies the `geo-types` based wrappers.
#[cfg(feature = "spatial")]
mod spatial {
    use super::*;
    use geo_types::{Geometry, Point};

    use sqlx_mssql_rs::{MssqlGeography, MssqlGeometry};

    #[tokio::test]
    async fn spatial_columns_round_trip_natively() {
        let mut conn = get_test_conn().await;
        let table = test_table_name("spatial");
        create_table(&mut conn, &table, "id INT, g GEOMETRY, p GEOGRAPHY").await;

        let geometry = Geometry::Point(Point::new(1.0, 2.0));

        let result = query(dynamic(format!(
            "INSERT INTO [{table}] (id, g, p) VALUES (?, ?, ?)"
        )))
        .bind(1i32)
        .bind(MssqlGeometry::new(geometry.clone(), 4326))
        .bind(MssqlGeography::new(geometry.clone(), 4326))
        .execute(&mut conn)
        .await;
        result.expect("inserting native spatial values should succeed");

        let row = query(dynamic(format!("SELECT g, p FROM [{table}] WHERE id = 1")))
            .fetch_one(&mut conn)
            .await
            .expect("select failed");

        let stored: MssqlGeometry = row.try_get(0).expect("decode geometry column");
        assert_eq!(stored.srid(), 4326, "the geometry SRID should survive");
        assert_eq!(stored.geometry(), &geometry);

        let stored: MssqlGeography = row.try_get(1).expect("decode geography column");
        assert_eq!(stored.srid(), 4326, "the geography SRID should survive");
        assert_eq!(stored.geometry(), &geometry);

        drop_table(&mut conn, &table).await;
        conn.close().await.unwrap();
    }

    #[tokio::test]
    async fn a_geometry_column_is_not_read_as_wkb() {
        let mut conn = get_test_conn().await;

        let row = query("SELECT geometry::Point(1, 2, 4326) AS g")
            .fetch_one(&mut conn)
            .await
            .expect("query failed");

        // Before the fix this silently produced a wrong point, because byte 0
        // of the SRID was accepted as a big-endian WKB marker. sqlx rejects it
        // at the type check, and `Decode` rejects it again independently.
        let error = row
            .try_get::<Geometry<f64>, _>(0)
            .expect_err("a UDT payload must not be decoded as WKB");
        assert!(
            error.to_string().contains("geometry"),
            "unexpected error: {error}"
        );

        // The WKB projection in a `varbinary` column still decodes.
        let row = query("SELECT geometry::Point(1, 2, 4326).STAsBinary() AS w")
            .fetch_one(&mut conn)
            .await
            .expect("query failed");
        let wkb: Geometry<f64> = row.try_get(0).expect("decode WKB");
        assert_eq!(wkb, Geometry::Point(Point::new(1.0, 2.0)));

        conn.close().await.unwrap();
    }

    #[tokio::test]
    async fn a_null_wkb_parameter_is_declared_varbinary() {
        let mut conn = get_test_conn().await;

        // A typed NULL for `Geometry<f64>` reports `varbinary(max)`, whose TDS
        // type is `BigVarBinary`. It used to fall through to the `nvarchar(max)`
        // default, which SQL Server refuses to convert to `varbinary` in
        // `STGeomFromWKB(@p, 0)`.
        let value: Option<Geometry<f64>> = query_scalar("SELECT geometry::STGeomFromWKB(?, 0)")
            .bind(None::<Geometry<f64>>)
            .fetch_one(&mut conn)
            .await
            .expect("a NULL geometry parameter should be accepted");
        assert!(value.is_none());

        // The spatial wrapper's own typed NULL is declared the same way, so it
        // can be inserted straight into a `geometry` column.
        let table = test_table_name("spatial_null");
        create_table(&mut conn, &table, "g GEOMETRY").await;
        query(dynamic(format!("INSERT INTO [{table}] (g) VALUES (?)")))
            .bind(None::<MssqlGeometry>)
            .execute(&mut conn)
            .await
            .expect("a NULL geometry should insert");
        let count: i32 = query_scalar(dynamic(format!("SELECT COUNT(*) FROM [{table}]")))
            .fetch_one(&mut conn)
            .await
            .expect("count failed");
        assert_eq!(count, 1);
        drop_table(&mut conn, &table).await;

        conn.close().await.unwrap();
    }

    #[tokio::test]
    async fn prepare_reports_spatial_types_by_name() {
        let mut conn = get_test_conn().await;
        let table = test_table_name("spatial_meta");
        create_table(&mut conn, &table, "id INT, g GEOMETRY").await;

        // A parameter bound to a spatial column resolves to the wrapper type,
        // which is what makes the query macros accept a bound geometry.
        let statement = conn
            .prepare(dynamic(format!("INSERT INTO [{table}] (id, g) VALUES (?, ?)")).into_sql_str())
            .await
            .expect("prepare failed");
        match statement.parameters() {
            Some(Either::Left(types)) => {
                assert_eq!(types.len(), 2, "one entry per parameter marker");
                assert!(
                    types[1].is_geometry(),
                    "expected a geometry parameter, got {}",
                    types[1]
                );
            }
            other => panic!("expected a geometry parameter type, got {other:?}"),
        }

        // The column reports `geometry` rather than the generic `udt` name.
        let statement = conn
            .prepare(dynamic(format!("SELECT g FROM [{table}]")).into_sql_str())
            .await
            .expect("prepare failed");
        assert_eq!(statement.columns()[0].type_info().type_name(), "geometry");

        drop_table(&mut conn, &table).await;
        conn.close().await.unwrap();
    }

    /// The query macros splice the Rust type path from the driver's
    /// type-checking table straight into the caller, so `MssqlGeometry` has to
    /// resolve here without the driver crate's own `crate::` prefix.
    #[tokio::test]
    async fn query_macro_resolves_spatial_types() {
        let mut conn = get_test_conn().await;

        let value: Option<MssqlGeometry> =
            sqlx_mssql_rs::query_scalar!("SELECT geometry::Point(1, 2, 4326) AS g")
                .fetch_one(&mut conn)
                .await
                .expect("macro query failed");
        let value = value.expect("the point is not NULL");

        assert_eq!(value.srid(), 4326);
        assert_eq!(value.geometry(), &Geometry::Point(Point::new(1.0, 2.0)));

        conn.close().await.unwrap();
    }
}
