//! Core MSSQL driver for SQLx built on Microsoft's [`mssql-tds`] TDS implementation.
//!
//! Unlike an ODBC-based driver, this one speaks TDS natively and is fully
//! asynchronous, so it needs no background thread or blocking bridge: the driver
//! is a thin mapping between SQLx's traits and `mssql-tds`'s pull-based
//! result-set cursor.
//!
//! # Connection
//!
//! ```no_run
//! use sqlx_core::connection::Connection;
//! use sqlx_core::row::Row;
//! use sqlx_mssql_rs_core::MssqlConnection;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
//! let mut conn = MssqlConnection::connect("mssql://sa:Password1!@localhost:1433/master").await?;
//!
//! let row = sqlx_core::query::query("SELECT 1").fetch_one(&mut conn).await?;
//! let value: i32 = row.try_get(0)?;
//! assert_eq!(value, 1);
//!
//! conn.close().await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Authentication
//!
//! Only SQL logins are enabled by default. Enable the `integrated-auth` feature
//! for Kerberos or NTLM authentication.
//!
//! [`mssql-tds`]: https://github.com/microsoft/mssql-rs

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![warn(future_incompatible, rust_2018_idioms)]
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "any")]
#[cfg_attr(docsrs, doc(cfg(feature = "any")))]
#[doc(hidden)]
pub mod any;

mod arguments;
mod column;
mod connection;
mod database;
#[cfg(feature = "offline")]
mod describe;
mod error;
#[cfg(feature = "migrate")]
mod migrate;
mod options;
mod param_markers;
mod query_result;
mod row;
mod statement;
mod transaction;
/// Type-checking support for compile-time query macros.
pub mod type_checking;
mod type_info;
mod types;
mod value;

pub use arguments::{MssqlArgumentValue, MssqlArguments};
pub use column::MssqlColumn;
pub use connection::MssqlConnection;
pub use database::Mssql;
#[cfg(feature = "offline")]
#[cfg_attr(docsrs, doc(cfg(feature = "offline")))]
pub use describe::{describe_blocking, MSSQL_DRIVER};
pub use error::{MssqlDatabaseError, MssqlError, Result};
pub use options::{MssqlConnectOptions, MssqlEncryption};
pub use query_result::MssqlQueryResult;
pub use row::MssqlRow;
pub use statement::MssqlStatement;
pub use transaction::MssqlTransactionManager;
pub use type_info::MssqlTypeInfo;
pub use value::{MssqlValue, MssqlValueRef};

/// An alias for [`Pool`][sqlx_core::pool::Pool], specialized for MSSQL.
pub type MssqlPool = sqlx_core::pool::Pool<Mssql>;

/// An alias for [`PoolOptions`][sqlx_core::pool::PoolOptions], specialized for MSSQL.
pub type MssqlPoolOptions = sqlx_core::pool::PoolOptions<Mssql>;

/// An alias for [`Transaction`][sqlx_core::transaction::Transaction], specialized for MSSQL.
pub type MssqlTransaction<'c> = sqlx_core::transaction::Transaction<'c, Mssql>;

/// An alias for [`Executor<'_, Database = Mssql>`][sqlx_core::executor::Executor].
pub trait MssqlExecutor<'c>: sqlx_core::executor::Executor<'c, Database = Mssql> {}
impl<'c, T> MssqlExecutor<'c> for T where T: sqlx_core::executor::Executor<'c, Database = Mssql> {}

sqlx_core::impl_acquire!(Mssql, MssqlConnection);
