//! Error types for the MSSQL driver.

use std::borrow::Cow;
use std::fmt::{Display, Formatter, Result as FmtResult};

use mssql_tds::error::Error as TdsError;

/// Result alias for this crate.
pub type Result<T, E = MssqlError> = std::result::Result<T, E>;

/// Error type returned by this crate.
#[derive(Debug, thiserror::Error)]
pub enum MssqlError {
    /// Database error reported by SQL Server or the TDS layer.
    #[error(transparent)]
    Database(#[from] MssqlDatabaseError),

    /// Invalid local configuration.
    #[error("MSSQL configuration error: {0}")]
    Configuration(String),
}

impl From<TdsError> for MssqlError {
    fn from(error: TdsError) -> Self {
        Self::Database(MssqlDatabaseError::from(error))
    }
}

impl From<MssqlError> for sqlx_core::Error {
    fn from(error: MssqlError) -> Self {
        match error {
            MssqlError::Database(error) => sqlx_core::Error::Database(Box::new(error)),
            MssqlError::Configuration(message) => sqlx_core::Error::Configuration(message.into()),
        }
    }
}

/// Wraps a TDS error as a driver error with additional human-readable context.
pub(crate) fn database_error_with_context(
    error: TdsError,
    context: impl Into<String>,
) -> MssqlError {
    MssqlError::Database(MssqlDatabaseError::with_context(error, context))
}

/// Database error details extracted from `mssql-tds`.
///
/// The primary server error message and number are lifted out so that
/// [`sqlx_core::Error::as_database_error`] consumers see the same shape they
/// would get from any other driver.
#[derive(Debug)]
pub struct MssqlDatabaseError {
    source: Box<TdsError>,
    message: String,
    code: Option<String>,
}

impl MssqlDatabaseError {
    fn with_context(error: TdsError, context: impl Into<String>) -> Self {
        let mut database_error = Self::from(error);
        database_error.message = format!("{}: {}", context.into(), database_error.message);
        database_error
    }

    /// Primary diagnostic message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// SQL Server error number, rendered as a string, when the server reported one.
    pub fn code(&self) -> Option<Cow<'_, str>> {
        self.code.as_deref().map(Cow::Borrowed)
    }
}

impl From<TdsError> for MssqlDatabaseError {
    fn from(error: TdsError) -> Self {
        // A batch or RPC can report several errors; the first is the one SQL
        // Server considers primary, so that is what gets surfaced.
        let (message, code) = match &error {
            TdsError::SqlServerError { diagnostics } => match diagnostics.errors.first() {
                Some(first) => (first.message.clone(), Some(first.number.to_string())),
                None => (error.to_string(), None),
            },
            _ => (error.to_string(), None),
        };

        Self {
            source: Box::new(error),
            message,
            code,
        }
    }
}

impl Display for MssqlDatabaseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.write_str(&self.message)
    }
}

impl std::error::Error for MssqlDatabaseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

impl sqlx_core::error::DatabaseError for MssqlDatabaseError {
    fn message(&self) -> &str {
        self.message()
    }

    fn code(&self) -> Option<Cow<'_, str>> {
        self.code()
    }

    fn as_error(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
        self
    }

    fn as_error_mut(&mut self) -> &mut (dyn std::error::Error + Send + Sync + 'static) {
        self
    }

    fn into_error(self: Box<Self>) -> Box<dyn std::error::Error + Send + Sync + 'static> {
        self
    }

    fn kind(&self) -> sqlx_core::error::ErrorKind {
        sqlx_core::error::ErrorKind::Other
    }
}
