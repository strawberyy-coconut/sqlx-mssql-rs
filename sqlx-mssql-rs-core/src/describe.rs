//! Compile-time query description for the MSSQL macros.
//!
//! Only available with the `offline` feature.
//!
//! `mssql-tds` has no blocking API, so unlike a blocking driver this has to
//! stand up a small single-threaded Tokio runtime to describe a query from
//! inside a proc macro.

use crate::statement::clone_param_info;
use crate::{Mssql, MssqlConnectOptions, MssqlConnection, MssqlStatement};
use sqlx_core::connection::Connection as _;
use sqlx_core::describe::Describe;
use sqlx_core::executor::Executor;
use sqlx_core::sql_str::{AssertSqlSafe, SqlSafeStr};
use sqlx_core::statement::Statement as _;

/// Compile-time query descriptor plugged into [`sqlx_macros_core`].
#[doc(hidden)]
pub const MSSQL_DRIVER: sqlx_macros_core::query::QueryDriver =
    sqlx_macros_core::query::QueryDriver::new::<Mssql>();

impl sqlx_macros_core::database::DatabaseExt for Mssql {
    const DATABASE_PATH: &'static str = "sqlx_mssql_rs::Mssql";
    const ROW_PATH: &'static str = "sqlx_mssql_rs::MssqlRow";

    fn describe_blocking(
        query: &str,
        database_url: &str,
        driver_config: &sqlx_core::config::drivers::Config,
    ) -> sqlx_core::Result<Describe<Self>> {
        describe_blocking(query, database_url, driver_config)
    }
}

/// Connects to an MSSQL database at compile time and describes a SQL query.
///
/// Returns column metadata, the parameter count, and nullability information.
/// This function is `#[doc(hidden)]` — it is only used by the proc macros.
#[doc(hidden)]
pub fn describe_blocking(
    query: &str,
    database_url: &str,
    _driver_config: &sqlx_core::config::drivers::Config,
) -> Result<Describe<Mssql>, sqlx_core::Error> {
    let options: MssqlConnectOptions = database_url.parse()?;

    let statement = describe_with_runtime(&options, query)?;

    Ok(Describe {
        columns: statement.columns().to_vec(),
        parameters: clone_param_info(statement.parameters()),
        nullable: statement
            .columns()
            .iter()
            .map(crate::MssqlColumn::nullable)
            .collect(),
    })
}

/// Runs the async describe on a temporary runtime.
///
/// Proc macros expand during compilation, where no async runtime exists, so one
/// has to be created for the duration of the call.
fn describe_with_runtime(
    options: &MssqlConnectOptions,
    query: &str,
) -> Result<MssqlStatement, sqlx_core::Error> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| {
            sqlx_core::Error::Configuration(
                format!("failed to start a runtime to describe the query: {error}").into(),
            )
        })?;

    runtime.block_on(async move {
        let mut connection = MssqlConnection::connect_with(options).await?;

        let sql = AssertSqlSafe(query.to_owned()).into_sql_str();
        let statement = (&mut connection).prepare_with(sql, &[]).await?;

        // Describe is best-effort: a failure to close cleanly should not mask
        // the metadata that was successfully read.
        let _ = connection.close().await;

        Ok(statement)
    })
}
