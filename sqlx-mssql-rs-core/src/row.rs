use crate::{Mssql, MssqlColumn, MssqlValue};
use std::sync::Arc;

/// A row of an MSSQL result set.
///
/// Column metadata is shared between the rows of a result set so that wide
/// result sets do not repeat it per row.
#[derive(Debug, Clone, Default)]
pub struct MssqlRow {
    columns: Arc<[MssqlColumn]>,
    values: Vec<MssqlValue>,
}

impl MssqlRow {
    /// Creates a row from column metadata and values.
    pub fn new(columns: Vec<MssqlColumn>, values: Vec<MssqlValue>) -> Self {
        Self::new_shared(columns.into(), values)
    }

    pub(crate) fn new_shared(columns: Arc<[MssqlColumn]>, values: Vec<MssqlValue>) -> Self {
        Self { columns, values }
    }
}

impl sqlx_core::row::Row for MssqlRow {
    type Database = Mssql;

    fn columns(&self) -> &[MssqlColumn] {
        self.columns.as_ref()
    }

    fn try_get_raw<I>(
        &self,
        index: I,
    ) -> Result<<Self::Database as sqlx_core::database::Database>::ValueRef<'_>, sqlx_core::Error>
    where
        I: sqlx_core::column::ColumnIndex<Self>,
    {
        let index = index.index(self)?;
        let value = self
            .values
            .get(index)
            .ok_or(sqlx_core::Error::ColumnIndexOutOfBounds {
                index,
                len: self.values.len(),
            })?;

        Ok(sqlx_core::value::Value::as_ref(value))
    }
}

impl sqlx_core::column::ColumnIndex<MssqlRow> for usize {
    fn index(&self, row: &MssqlRow) -> Result<usize, sqlx_core::Error> {
        if *self >= row.columns.len() {
            return Err(sqlx_core::Error::ColumnIndexOutOfBounds {
                index: *self,
                len: row.columns.len(),
            });
        }

        Ok(*self)
    }
}

impl sqlx_core::column::ColumnIndex<MssqlRow> for &str {
    fn index(&self, row: &MssqlRow) -> Result<usize, sqlx_core::Error> {
        if let Some(index) = row
            .columns
            .iter()
            .position(|column| sqlx_core::column::Column::name(column) == *self)
        {
            return Ok(index);
        }

        row.columns
            .iter()
            .position(|column| sqlx_core::column::Column::name(column).eq_ignore_ascii_case(self))
            .ok_or_else(|| sqlx_core::Error::ColumnNotFound((*self).to_owned()))
    }
}
