use crate::{Mssql, MssqlConnection};

/// Transaction manager for MSSQL.
pub struct MssqlTransactionManager;

impl sqlx_core::transaction::TransactionManager for MssqlTransactionManager {
    type Database = Mssql;

    async fn begin(
        conn: &mut MssqlConnection,
        _statement: Option<sqlx_core::sql_str::SqlStr>,
    ) -> Result<(), sqlx_core::Error> {
        // Boxed so the driver's internal future chain is erased from the
        // opaque future the pool stores for each transaction.
        Box::pin(conn.begin_transaction()).await
    }

    async fn commit(conn: &mut MssqlConnection) -> Result<(), sqlx_core::Error> {
        Box::pin(conn.commit_transaction()).await
    }

    async fn rollback(conn: &mut MssqlConnection) -> Result<(), sqlx_core::Error> {
        Box::pin(conn.rollback_transaction()).await
    }

    fn start_rollback(conn: &mut MssqlConnection) {
        conn.queue_rollback();
    }

    fn get_transaction_depth(conn: &MssqlConnection) -> usize {
        conn.transaction_depth()
    }
}
