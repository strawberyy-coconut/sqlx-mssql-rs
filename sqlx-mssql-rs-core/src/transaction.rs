use std::future::Future;
use std::pin::Pin;

use crate::{Mssql, MssqlConnection};

/// Transaction manager for MSSQL.
pub struct MssqlTransactionManager;

impl sqlx_core::transaction::TransactionManager for MssqlTransactionManager {
    type Database = Mssql;

    async fn begin(
        conn: &mut MssqlConnection,
        _statement: Option<sqlx_core::sql_str::SqlStr>,
    ) -> Result<(), sqlx_core::Error> {
        // Coerced to a trait object so the driver's internal future chain is
        // erased from the opaque future the pool stores for each transaction.
        let begin: Pin<Box<dyn Future<Output = Result<(), sqlx_core::Error>> + Send + '_>> =
            Box::pin(conn.begin_transaction());
        begin.await
    }

    async fn commit(conn: &mut MssqlConnection) -> Result<(), sqlx_core::Error> {
        let commit: Pin<Box<dyn Future<Output = Result<(), sqlx_core::Error>> + Send + '_>> =
            Box::pin(conn.commit_transaction());
        commit.await
    }

    async fn rollback(conn: &mut MssqlConnection) -> Result<(), sqlx_core::Error> {
        let rollback: Pin<Box<dyn Future<Output = Result<(), sqlx_core::Error>> + Send + '_>> =
            Box::pin(conn.rollback_transaction());
        rollback.await
    }

    fn start_rollback(conn: &mut MssqlConnection) {
        conn.queue_rollback();
    }

    fn get_transaction_depth(conn: &MssqlConnection) -> usize {
        conn.transaction_depth()
    }
}
