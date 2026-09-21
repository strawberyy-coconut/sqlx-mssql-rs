//! `Any` driver integration for MSSQL.
//!
//! Implements [`AnyConnectionBackend`] for [`MssqlConnection`] plus the type
//! conversions the `Any` driver needs, so that `sqlx-cli` and `AnyConnection`
//! can work with `mssql://` URLs.

use std::str::FromStr;
use std::sync::Arc;

use futures_core::future::BoxFuture;
use futures_core::stream::BoxStream;
use futures_util::{future, stream, FutureExt, StreamExt};

use mssql_tds::datatypes::sqldatatypes::TdsDataType;

use sqlx_core::any::{
    AnyArguments, AnyColumn, AnyConnectOptions, AnyConnectionBackend, AnyQueryResult, AnyRow,
    AnyStatement, AnyTypeInfo, AnyTypeInfoKind,
};
use sqlx_core::column::Column;
use sqlx_core::connection::Connection;
use sqlx_core::database::Database;
use sqlx_core::encode::{Encode, IsNull};
use sqlx_core::error::BoxDynError;
use sqlx_core::executor::{Execute, Executor};
use sqlx_core::ext::ustr::UStr;
use sqlx_core::row::Row;
use sqlx_core::sql_str::SqlStr;
use sqlx_core::statement::Statement as _;
use sqlx_core::transaction::TransactionManager;
use sqlx_core::Either;
use sqlx_core::HashMap;

use crate::{
    Mssql, MssqlArgumentValue, MssqlArguments, MssqlColumn, MssqlConnectOptions, MssqlConnection,
    MssqlQueryResult, MssqlRow, MssqlStatement, MssqlTransactionManager, MssqlTypeInfo,
};

sqlx_core::declare_driver_with_optional_migrate!(DRIVER = Mssql);

/// A statement plus already-encoded arguments.
///
/// The `Any` backend receives raw SQL and `AnyArguments`, which have to be
/// re-encoded into this driver's argument buffer before going through the
/// normal [`Executor`] path.
struct RawQuery {
    sql: SqlStr,
    arguments: Option<MssqlArguments>,
}

impl<'q> Execute<'q, Mssql> for RawQuery {
    fn sql(self) -> SqlStr {
        self.sql
    }

    fn statement(&self) -> Option<&MssqlStatement> {
        None
    }

    fn take_arguments(&mut self) -> Result<Option<MssqlArguments>, BoxDynError> {
        Ok(self.arguments.take())
    }

    fn persistent(&self) -> bool {
        false
    }
}

/// `AnyArguments` stores text as `Arc<str>`; `str` itself has no `Encode` impl
/// of its own, so the standard smart-pointer blanket does not cover it.
impl<'q> Encode<'q, Mssql> for Arc<str> {
    fn encode(self, buf: &mut Vec<MssqlArgumentValue>) -> Result<IsNull, BoxDynError> {
        buf.push(MssqlArgumentValue::text(self.to_string()));
        Ok(IsNull::No)
    }

    fn encode_by_ref(&self, buf: &mut Vec<MssqlArgumentValue>) -> Result<IsNull, BoxDynError> {
        buf.push(MssqlArgumentValue::text(self.to_string()));
        Ok(IsNull::No)
    }
}

impl AnyConnectionBackend for MssqlConnection {
    fn name(&self) -> &str {
        <Mssql as Database>::NAME
    }

    fn close(self: Box<Self>) -> BoxFuture<'static, sqlx_core::Result<()>> {
        Connection::close(*self).boxed()
    }

    fn close_hard(self: Box<Self>) -> BoxFuture<'static, sqlx_core::Result<()>> {
        Connection::close_hard(*self).boxed()
    }

    fn ping(&mut self) -> BoxFuture<'_, sqlx_core::Result<()>> {
        Connection::ping(self).boxed()
    }

    fn begin(&mut self, statement: Option<SqlStr>) -> BoxFuture<'_, sqlx_core::Result<()>> {
        MssqlTransactionManager::begin(self, statement).boxed()
    }

    fn commit(&mut self) -> BoxFuture<'_, sqlx_core::Result<()>> {
        MssqlTransactionManager::commit(self).boxed()
    }

    fn rollback(&mut self) -> BoxFuture<'_, sqlx_core::Result<()>> {
        MssqlTransactionManager::rollback(self).boxed()
    }

    fn start_rollback(&mut self) {
        MssqlTransactionManager::start_rollback(self)
    }

    fn get_transaction_depth(&self) -> usize {
        MssqlTransactionManager::get_transaction_depth(self)
    }

    fn shrink_buffers(&mut self) {
        Connection::shrink_buffers(self);
    }

    fn flush(&mut self) -> BoxFuture<'_, sqlx_core::Result<()>> {
        Connection::flush(self).boxed()
    }

    fn should_flush(&self) -> bool {
        Connection::should_flush(self)
    }

    fn cached_statements_size(&self) -> usize {
        Connection::cached_statements_size(self)
    }

    fn clear_cached_statements(&mut self) -> BoxFuture<'_, sqlx_core::Result<()>> {
        Connection::clear_cached_statements(self).boxed()
    }

    #[cfg(feature = "migrate")]
    fn as_migrate(
        &mut self,
    ) -> sqlx_core::Result<&mut (dyn sqlx_core::migrate::Migrate + Send + 'static)> {
        Ok(self)
    }

    fn fetch_many(
        &mut self,
        query: SqlStr,
        _persistent: bool,
        arguments: Option<AnyArguments>,
    ) -> BoxStream<'_, sqlx_core::Result<Either<AnyQueryResult, AnyRow>>> {
        let arguments = match arguments.map(|a| a.convert_into::<MssqlArguments>()).transpose() {
            Ok(arguments) => arguments,
            Err(error) => {
                return stream::once(future::ready(Err(sqlx_core::Error::Encode(error)))).boxed();
            }
        };

        Executor::fetch_many(&mut *self, RawQuery { sql: query, arguments })
            .map(|item| {
                item.and_then(|either| match either {
                    Either::Left(result) => Ok(Either::Left(AnyQueryResult {
                        rows_affected: result.rows_affected(),
                        last_insert_id: None,
                    })),
                    Either::Right(row) => AnyRow::try_from(&row).map(Either::Right),
                })
            })
            .boxed()
    }

    fn fetch_optional(
        &mut self,
        query: SqlStr,
        _persistent: bool,
        arguments: Option<AnyArguments>,
    ) -> BoxFuture<'_, sqlx_core::Result<Option<AnyRow>>> {
        let arguments = match arguments.map(|a| a.convert_into::<MssqlArguments>()).transpose() {
            Ok(arguments) => arguments,
            Err(error) => {
                return Box::pin(future::ready(Err(sqlx_core::Error::Encode(error))));
            }
        };

        let raw = RawQuery { sql: query, arguments };

        Box::pin(async move {
            match Executor::fetch_optional(&mut *self, raw).await? {
                Some(row) => Ok(Some(AnyRow::try_from(&row)?)),
                None => Ok(None),
            }
        })
    }

    fn prepare_with<'c, 'q: 'c>(
        &'c mut self,
        sql: SqlStr,
        _parameters: &[AnyTypeInfo],
    ) -> BoxFuture<'c, sqlx_core::Result<AnyStatement>> {
        Box::pin(async move {
            let statement = Executor::prepare_with(&mut *self, sql, &[]).await?;

            let columns: Vec<MssqlColumn> = statement.columns().to_vec();
            let mut names = HashMap::<UStr, usize>::new();
            for (index, column) in columns.iter().enumerate() {
                names.insert(UStr::from(column.name().to_owned()), index);
            }

            AnyStatement::try_from_statement(statement, Arc::new(names))
        })
    }

    #[cfg(feature = "offline")]
    fn describe(
        &mut self,
        sql: SqlStr,
    ) -> BoxFuture<'_, sqlx_core::Result<sqlx_core::describe::Describe<sqlx_core::any::Any>>> {
        Box::pin(async move {
            let describe = Executor::describe(&mut *self, sql).await?;
            describe.try_into_any()
        })
    }
}

// ---------------------------------------------------------------------------
// Type conversions
// ---------------------------------------------------------------------------

impl TryFrom<&MssqlTypeInfo> for AnyTypeInfo {
    type Error = sqlx_core::Error;

    fn try_from(type_info: &MssqlTypeInfo) -> Result<Self, Self::Error> {
        let kind = match type_info.data_type() {
            Some(TdsDataType::Bit | TdsDataType::BitN) => AnyTypeInfoKind::Bool,
            Some(TdsDataType::Int1 | TdsDataType::Int2) => AnyTypeInfoKind::SmallInt,
            Some(TdsDataType::Int4) => AnyTypeInfoKind::Integer,
            Some(TdsDataType::Int8) => AnyTypeInfoKind::BigInt,
            Some(TdsDataType::Flt4) => AnyTypeInfoKind::Real,
            Some(TdsDataType::Flt8 | TdsDataType::FltN) => AnyTypeInfoKind::Double,
            Some(
                TdsDataType::BigChar
                | TdsDataType::BigVarChar
                | TdsDataType::Char
                | TdsDataType::VarChar
                | TdsDataType::Text
                | TdsDataType::NChar
                | TdsDataType::NVarChar
                | TdsDataType::NText
                | TdsDataType::Xml
                | TdsDataType::Json,
            ) => AnyTypeInfoKind::Text,
            Some(
                TdsDataType::BigBinary
                | TdsDataType::BigVarBinary
                | TdsDataType::Binary
                | TdsDataType::VarBinary
                | TdsDataType::Image
                | TdsDataType::Udt
                | TdsDataType::Vector,
            ) => AnyTypeInfoKind::Blob,
            // The `Any` driver has no dedicated kind for these, and text is a
            // lossless carrier for all of them.
            _ => AnyTypeInfoKind::Text,
        };

        Ok(AnyTypeInfo { kind })
    }
}

impl TryFrom<&MssqlColumn> for AnyColumn {
    type Error = sqlx_core::Error;

    fn try_from(column: &MssqlColumn) -> Result<Self, Self::Error> {
        Ok(AnyColumn {
            ordinal: column.ordinal(),
            name: UStr::from(column.name().to_owned()),
            type_info: AnyTypeInfo::try_from(column.type_info())?,
        })
    }
}

impl TryFrom<&MssqlRow> for AnyRow {
    type Error = sqlx_core::Error;

    fn try_from(row: &MssqlRow) -> Result<Self, Self::Error> {
        let columns: Vec<MssqlColumn> = row.columns().to_vec();
        let mut names = HashMap::<UStr, usize>::new();
        for (index, column) in columns.iter().enumerate() {
            names.insert(UStr::from(column.name().to_owned()), index);
        }

        AnyRow::map_from(row, Arc::new(names))
    }
}

impl TryFrom<&AnyConnectOptions> for MssqlConnectOptions {
    type Error = sqlx_core::Error;

    fn try_from(any_options: &AnyConnectOptions) -> Result<Self, Self::Error> {
        let mut options = Self::from_str(any_options.database_url.as_str())?;
        options.set_log_settings(any_options.log_settings.clone());
        Ok(options)
    }
}

/// Maps an `AnyQueryResult` back into a driver query result.
impl From<MssqlQueryResult> for AnyQueryResult {
    fn from(result: MssqlQueryResult) -> Self {
        Self {
            rows_affected: result.rows_affected(),
            last_insert_id: None,
        }
    }
}
