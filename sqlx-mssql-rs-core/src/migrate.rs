//! Migration support for MSSQL.
//!
//! Implements [`MigrateDatabase`] for [`Mssql`] (database lifecycle) and
//! [`Migrate`] for [`MssqlConnection`] (execution and tracking) so that
//! [`Migrator`](sqlx_core::migrate::Migrator) works with this driver.

use std::str::FromStr;
use std::time::Duration;

use futures_core::future::BoxFuture;
use sqlx_core::connection::Connection as _;
use sqlx_core::error::Error;
use sqlx_core::migrate::{AppliedMigration, Migrate, MigrateDatabase, MigrateError, Migration};
use url::Url;

use crate::{Mssql, MssqlConnectOptions, MssqlConnection};

/// Extracts the database name from a `mssql://` URL.
fn extract_database_name(url: &str) -> Result<String, Error> {
    let parsed = Url::parse(url)
        .map_err(|error| Error::Protocol(format!("failed to parse migration URL: {error}")))?;

    let database = parsed.path().trim_start_matches('/').to_owned();
    if database.is_empty() {
        return Err(Error::Configuration(
            "migration URL does not contain a database name".into(),
        ));
    }

    Ok(database)
}

/// Escapes a value for use inside square brackets in T-SQL.
fn escape_sql_bracket(value: &str) -> String {
    value.replace(']', "]]")
}

/// Escapes a string for use inside a `N'...'` T-SQL string literal.
fn escape_sql_string(value: &str) -> String {
    value.replace('\'', "''")
}

/// Formats a byte slice as a T-SQL hex literal, for example `0xDEADBEEF`.
fn format_hex(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(2 + bytes.len() * 2);
    hex.push_str("0x");
    for byte in bytes {
        hex.push_str(&format!("{byte:02X}"));
    }
    hex
}

/// Splits a potentially schema-qualified table name into `(schema, table)`.
fn split_table_name(table_name: &str) -> (&str, &str) {
    match table_name.find('.') {
        Some(dot) => (&table_name[..dot], &table_name[dot + 1..]),
        None => ("", table_name),
    }
}

/// Builds a safely quoted `[schema].[table]` reference.
fn quoted_table_name(table_name: &str) -> String {
    let (schema, table) = split_table_name(table_name);

    if schema.is_empty() {
        format!("[{}]", escape_sql_bracket(table))
    } else {
        format!(
            "[{}].[{}]",
            escape_sql_bracket(schema),
            escape_sql_bracket(table)
        )
    }
}

/// Builds the `INSERT` that records an applied migration.
fn insert_migration_sql(
    table_name: &str,
    migration: &Migration,
    record_sql: bool,
) -> String {
    let sql_text = if record_sql {
        format!("N'{}'", escape_sql_string(migration.sql.as_str()))
    } else {
        "N''".to_owned()
    };

    format!(
        "INSERT INTO {quoted} \
         (version, description, migration_type, sql, checksum, no_tx) \
         VALUES ({version}, N'{description}', N'{migration_type}', {sql_text}, {checksum}, {no_tx})",
        quoted = quoted_table_name(table_name),
        version = migration.version,
        description = escape_sql_string(&migration.description),
        migration_type = escape_sql_string(&format!("{:?}", migration.migration_type)),
        sql_text = sql_text,
        checksum = format_hex(&migration.checksum),
        no_tx = if migration.no_tx { 1 } else { 0 },
    )
}

impl MigrateDatabase for Mssql {
    async fn create_database(url: &str) -> Result<(), Error> {
        let options = MssqlConnectOptions::from_str(url)?;
        let database = extract_database_name(url)?;

        // `CREATE DATABASE` cannot run from inside the database being created,
        // so this connects to `master`.
        let mut connection =
            MssqlConnection::connect_with(&options.with_database("master")).await?;

        connection
            .exec_sql(&format!(
                "CREATE DATABASE [{}]",
                escape_sql_bracket(&database)
            ))
            .await?;

        connection.close().await
    }

    async fn database_exists(url: &str) -> Result<bool, Error> {
        let options = MssqlConnectOptions::from_str(url)?;

        // Fast path: connecting successfully proves the database exists.
        if MssqlConnection::connect_with(&options).await.is_ok() {
            return Ok(true);
        }

        let database = extract_database_name(url)?;
        let mut connection =
            match MssqlConnection::connect_with(&options.with_database("master")).await {
                Ok(connection) => connection,
                Err(_) => return Ok(false),
            };

        let count = connection
            .scalar_i64(&format!(
                "SELECT COUNT(*) FROM sys.databases WHERE name = N'{}'",
                escape_sql_string(&database)
            ))
            .await?
            .unwrap_or(0);

        connection.close().await?;
        Ok(count > 0)
    }

    async fn drop_database(url: &str) -> Result<(), Error> {
        let options = MssqlConnectOptions::from_str(url)?;
        let database = extract_database_name(url)?;

        let mut connection =
            MssqlConnection::connect_with(&options.with_database("master")).await?;

        connection
            .exec_sql(&format!(
                "DROP DATABASE IF EXISTS [{}]",
                escape_sql_bracket(&database)
            ))
            .await?;

        connection.close().await
    }
}

impl Migrate for MssqlConnection {
    /// MSSQL has no `CREATE SCHEMA IF NOT EXISTS`, so a conditional block is
    /// used instead.
    fn create_schema_if_not_exists<'e>(
        &'e mut self,
        schema_name: &'e str,
    ) -> BoxFuture<'e, Result<(), MigrateError>> {
        let sql = format!(
            "IF NOT EXISTS (SELECT * FROM sys.schemas WHERE name = N'{}') \
             EXEC('CREATE SCHEMA [{}]')",
            escape_sql_string(schema_name),
            escape_sql_bracket(schema_name),
        );

        Box::pin(async move {
            self.exec_sql(&sql).await.map_err(MigrateError::Execute)?;
            Ok(())
        })
    }

    fn ensure_migrations_table<'e>(
        &'e mut self,
        table_name: &'e str,
    ) -> BoxFuture<'e, Result<(), MigrateError>> {
        let quoted = quoted_table_name(table_name);
        let (schema, table) = split_table_name(table_name);

        let schema_condition = if schema.is_empty() {
            "TABLE_SCHEMA = 'dbo'".to_owned()
        } else {
            format!("TABLE_SCHEMA = N'{}'", escape_sql_string(schema))
        };

        let sql = format!(
            "IF NOT EXISTS ( \
             SELECT * FROM INFORMATION_SCHEMA.TABLES \
             WHERE TABLE_NAME = N'{table}' AND {schema_condition} \
             ) \
             CREATE TABLE {quoted} ( \
             version        BIGINT         NOT NULL PRIMARY KEY, \
             description    NVARCHAR(MAX)  NOT NULL, \
             migration_type NVARCHAR(20)   NOT NULL, \
             sql            NVARCHAR(MAX)  NOT NULL, \
             checksum       VARBINARY(8000) NOT NULL, \
             executed_at    DATETIME2      NOT NULL DEFAULT GETUTCDATE(), \
             no_tx          BIT            NOT NULL DEFAULT 0 \
             )",
            table = escape_sql_string(table),
            schema_condition = schema_condition,
            quoted = quoted,
        );

        Box::pin(async move {
            self.exec_sql(&sql).await.map_err(MigrateError::Execute)?;
            Ok(())
        })
    }

    /// MSSQL supports transactional DDL, so a migration can never be left
    /// partially applied.
    fn dirty_version<'e>(
        &'e mut self,
        _table_name: &'e str,
    ) -> BoxFuture<'e, Result<Option<i64>, MigrateError>> {
        Box::pin(async move { Ok(None) })
    }

    fn list_applied_migrations<'e>(
        &'e mut self,
        table_name: &'e str,
    ) -> BoxFuture<'e, Result<Vec<AppliedMigration>, MigrateError>> {
        let sql = format!(
            "SELECT version, checksum FROM {} ORDER BY version",
            quoted_table_name(table_name)
        );

        Box::pin(async move {
            let rows = self.list_migrations(&sql).await.map_err(MigrateError::Execute)?;

            Ok(rows
                .into_iter()
                .map(|(version, checksum)| AppliedMigration {
                    version,
                    checksum: checksum.into(),
                })
                .collect())
        })
    }

    fn lock(&mut self) -> BoxFuture<'_, Result<(), MigrateError>> {
        Box::pin(async move {
            self.exec_sql(
                "EXEC sp_getapplock \
                 @Resource = N'sqlx_migration_lock', \
                 @LockMode = 'Exclusive', \
                 @LockOwner = 'Session'",
            )
            .await
            .map_err(MigrateError::Execute)
        })
    }

    fn unlock(&mut self) -> BoxFuture<'_, Result<(), MigrateError>> {
        Box::pin(async move {
            self.exec_sql(
                "EXEC sp_releaseapplock \
                 @Resource = N'sqlx_migration_lock', \
                 @LockOwner = 'Session'",
            )
            .await
            .map_err(MigrateError::Execute)
        })
    }

    fn apply<'e>(
        &'e mut self,
        table_name: &'e str,
        migration: &'e Migration,
    ) -> BoxFuture<'e, Result<Duration, MigrateError>> {
        let sql = migration.sql.as_str().to_owned();
        let insert_sql = insert_migration_sql(table_name, migration, true);
        let version = migration.version;
        let no_tx = migration.no_tx;

        Box::pin(async move {
            self.run_migration_pair(&sql, &insert_sql, no_tx)
                .await
                .map_err(|error| MigrateError::ExecuteMigration(error, version))
        })
    }

    fn revert<'e>(
        &'e mut self,
        table_name: &'e str,
        migration: &'e Migration,
    ) -> BoxFuture<'e, Result<Duration, MigrateError>> {
        let sql = migration.sql.as_str().to_owned();
        let delete_sql = format!(
            "DELETE FROM {} WHERE version = {}",
            quoted_table_name(table_name),
            migration.version
        );
        let version = migration.version;
        let no_tx = migration.no_tx;

        Box::pin(async move {
            self.run_migration_pair(&sql, &delete_sql, no_tx)
                .await
                .map_err(|error| MigrateError::ExecuteMigration(error, version))
        })
    }

    fn skip<'e>(
        &'e mut self,
        table_name: &'e str,
        migration: &'e Migration,
    ) -> BoxFuture<'e, Result<(), MigrateError>> {
        // The migration SQL is deliberately recorded as empty: it was not run.
        let insert_sql = insert_migration_sql(table_name, migration, false);
        let version = migration.version;

        Box::pin(async move {
            self.exec_sql(&insert_sql)
                .await
                .map_err(|error| MigrateError::ExecuteMigration(error, version))
        })
    }
}
