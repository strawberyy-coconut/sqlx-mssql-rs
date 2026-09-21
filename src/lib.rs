//! MSSQL driver for SQLx built on Microsoft's [`mssql-tds`] TDS implementation.
//!
//! `sqlx-mssql-rs` connects SQLx to Microsoft SQL Server and Azure SQL over the
//! TDS protocol directly — no ODBC driver manager is involved. The underlying
//! client is fully asynchronous, so this driver needs no background thread or
//! blocking bridge.
//!
//! # Connection
//!
//! ```no_run
//! #![recursion_limit = "512"]
//! use sqlx_mssql_rs::MssqlPool;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
//! let pool = MssqlPool::connect("mssql://sa:Password1!@localhost:1433/master").await?;
//!
//! let value: i32 = sqlx_core::query_scalar::query_scalar("SELECT 1")
//!     .fetch_one(&pool)
//!     .await?;
//! assert_eq!(value, 1);
//! # Ok(())
//! # }
//! ```
//!
//! Connection URLs use the `mssql://` scheme:
//!
//! ```text
//! mssql://user:password@host:1433/database?trust_certificate=true
//! ```
//!
//! # Features
//!
//! | Feature | Default | Description |
//! |---------|---------|-------------|
//! | `macros` | yes | Compile-time checked query macros |
//! | `derive` | yes | `Encode`, `Decode`, `Type`, `FromRow` derives |
//! | `migrate` | yes | Migration support |
//! | `offline` | yes | Offline query metadata via `.sqlx` |
//! | `uuid`, `chrono`, `time`, `json` | no | Type integrations |
//! | `bigdecimal`, `rust_decimal`, `decimal` | no | Exact numeric integrations |
//! | `spatial` | no | `geo-types` geometry support |
//! | `integrated-auth` | no | Kerberos / NTLM authentication |
//!
//! # Authentication
//!
//! SQL logins work out of the box. Enable `integrated-auth` for Kerberos or
//! NTLM; on Unix the GSSAPI library is loaded at runtime, so no build-time
//! Kerberos dependency is required.
//!
//! # Recursion limit
//!
//! Query builder types and the generated type-checking tables are deeply
//! nested, so a crate that uses the query builders heavily may need to raise
//! the compiler's limit:
//!
//! ```ignore
//! #![recursion_limit = "512"]
//! ```
//!
//! If you see "queries overflow the depth limit", this is the fix.
//!
//! [`mssql-tds`]: https://github.com/microsoft/mssql-rs

#![warn(future_incompatible, rust_2018_idioms)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub use sqlx_mssql_rs_core::*;

pub use sqlx_mssql_rs_core as core;

#[cfg(feature = "derive")]
#[cfg_attr(docsrs, doc(cfg(feature = "derive")))]
pub use sqlx_mssql_rs_macros::{Decode, Encode, FromRow, Type};

#[cfg(feature = "macros")]
#[cfg_attr(docsrs, doc(cfg(feature = "macros")))]
pub use sqlx_mssql_rs_macros::expand_query;

/// Compile-time checked SQL query.
///
/// The query is verified against the database schema at compile time, either
/// against a live server (`DATABASE_URL`) or an offline cache produced by
/// `cargo sqlx prepare`.
///
/// Requires the `macros` feature.
#[cfg(feature = "macros")]
#[cfg_attr(docsrs, doc(cfg(feature = "macros")))]
#[macro_export]
macro_rules! query {
    ($query:expr) => ({
        $crate::expand_query!(source = $query)
    });
    ($query:expr, $($args:tt)*) => ({
        $crate::expand_query!(source = $query, args = [$($args)*])
    });
}

/// Compile-time checked SQL query (unchecked variant).
///
/// Like [`query!`](crate::query) but does not verify the query against a
/// database or offline cache.
///
/// Requires the `macros` feature.
#[cfg(feature = "macros")]
#[cfg_attr(docsrs, doc(cfg(feature = "macros")))]
#[macro_export]
macro_rules! query_unchecked {
    ($query:expr) => ({
        $crate::expand_query!(source = $query, checked = false)
    });
    ($query:expr, $($args:tt)*) => ({
        $crate::expand_query!(source = $query, args = [$($args)*], checked = false)
    });
}

/// Compile-time checked SQL query that maps rows into a named struct.
///
/// Requires the `macros` feature.
#[cfg(feature = "macros")]
#[cfg_attr(docsrs, doc(cfg(feature = "macros")))]
#[macro_export]
macro_rules! query_as {
    ($out_struct:path, $query:expr) => ({
        $crate::expand_query!(record = $out_struct, source = $query)
    });
    ($out_struct:path, $query:expr, $($args:tt)*) => ({
        $crate::expand_query!(record = $out_struct, source = $query, args = [$($args)*])
    });
}

/// Compile-time checked query mapping into a struct (unchecked variant).
///
/// Requires the `macros` feature.
#[cfg(feature = "macros")]
#[cfg_attr(docsrs, doc(cfg(feature = "macros")))]
#[macro_export]
macro_rules! query_as_unchecked {
    ($out_struct:path, $query:expr) => ({
        $crate::expand_query!(record = $out_struct, source = $query, checked = false)
    });
    ($out_struct:path, $query:expr, $($args:tt)*) => ({
        $crate::expand_query!(record = $out_struct, source = $query, args = [$($args)*], checked = false)
    });
}

/// Compile-time checked query returning a single column.
///
/// Requires the `macros` feature.
#[cfg(feature = "macros")]
#[cfg_attr(docsrs, doc(cfg(feature = "macros")))]
#[macro_export]
macro_rules! query_scalar {
    ($query:expr) => (
        $crate::expand_query!(scalar = _, source = $query)
    );
    ($query:expr, $($args:tt)*) => (
        $crate::expand_query!(scalar = _, source = $query, args = [$($args)*])
    );
}

/// Compile-time checked single-column query (unchecked variant).
///
/// Requires the `macros` feature.
#[cfg(feature = "macros")]
#[cfg_attr(docsrs, doc(cfg(feature = "macros")))]
#[macro_export]
macro_rules! query_scalar_unchecked {
    ($query:expr) => (
        $crate::expand_query!(scalar = _, source = $query, checked = false)
    );
    ($query:expr, $($args:tt)*) => (
        $crate::expand_query!(scalar = _, source = $query, args = [$($args)*], checked = false)
    );
}

/// Compile-time checked SQL query read from a file.
///
/// Requires the `macros` feature.
#[cfg(feature = "macros")]
#[cfg_attr(docsrs, doc(cfg(feature = "macros")))]
#[macro_export]
macro_rules! query_file {
    ($path:literal) => ({
        $crate::expand_query!(source_file = $path)
    });
    ($path:literal, $($args:tt)*) => ({
        $crate::expand_query!(source_file = $path, args = [$($args)*])
    });
}

/// Compile-time SQL query read from a file (unchecked variant).
///
/// Requires the `macros` feature.
#[cfg(feature = "macros")]
#[cfg_attr(docsrs, doc(cfg(feature = "macros")))]
#[macro_export]
macro_rules! query_file_unchecked {
    ($path:literal) => ({
        $crate::expand_query!(source_file = $path, checked = false)
    });
    ($path:literal, $($args:tt)*) => ({
        $crate::expand_query!(source_file = $path, args = [$($args)*], checked = false)
    });
}

/// Compile-time checked SQL query read from a file, mapping into a struct.
///
/// Requires the `macros` feature.
#[cfg(feature = "macros")]
#[cfg_attr(docsrs, doc(cfg(feature = "macros")))]
#[macro_export]
macro_rules! query_file_as {
    ($out_struct:path, $path:literal) => ({
        $crate::expand_query!(record = $out_struct, source_file = $path)
    });
    ($out_struct:path, $path:literal, $($args:tt)*) => ({
        $crate::expand_query!(record = $out_struct, source_file = $path, args = [$($args)*])
    });
}

/// File-based query mapping into a struct (unchecked variant).
///
/// Requires the `macros` feature.
#[cfg(feature = "macros")]
#[cfg_attr(docsrs, doc(cfg(feature = "macros")))]
#[macro_export]
macro_rules! query_file_as_unchecked {
    ($out_struct:path, $path:literal) => ({
        $crate::expand_query!(record = $out_struct, source_file = $path, checked = false)
    });
    ($out_struct:path, $path:literal, $($args:tt)*) => ({
        $crate::expand_query!(record = $out_struct, source_file = $path, args = [$($args)*], checked = false)
    });
}

/// File-based single-column query.
///
/// Requires the `macros` feature.
#[cfg(feature = "macros")]
#[cfg_attr(docsrs, doc(cfg(feature = "macros")))]
#[macro_export]
macro_rules! query_file_scalar {
    ($path:literal) => (
        $crate::expand_query!(scalar = _, source_file = $path)
    );
    ($path:literal, $($args:tt)*) => (
        $crate::expand_query!(scalar = _, source_file = $path, args = [$($args)*])
    );
}

/// File-based single-column query (unchecked variant).
///
/// Requires the `macros` feature.
#[cfg(feature = "macros")]
#[cfg_attr(docsrs, doc(cfg(feature = "macros")))]
#[macro_export]
macro_rules! query_file_scalar_unchecked {
    ($path:literal) => (
        $crate::expand_query!(scalar = _, source_file = $path, checked = false)
    );
    ($path:literal, $($args:tt)*) => (
        $crate::expand_query!(scalar = _, source_file = $path, args = [$($args)*], checked = false)
    );
}
