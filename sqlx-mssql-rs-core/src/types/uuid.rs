//! `uuid::Uuid` support.

use mssql_tds::datatypes::column_values::ColumnValues;
use mssql_tds::datatypes::sqldatatypes::TdsDataType;
use mssql_tds::datatypes::sqltypes::SqlType;

use sqlx_core::decode::Decode;
use sqlx_core::encode::{Encode, IsNull};
use sqlx_core::error::BoxDynError;
use sqlx_core::types::{Type, Uuid};

use crate::{Mssql, MssqlArgumentValue, MssqlTypeInfo, MssqlValueRef};

impl Type<Mssql> for Uuid {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::uuid()
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        matches!(ty.data_type(), Some(TdsDataType::Guid))
    }
}

impl<'q> Encode<'q, Mssql> for Uuid {
    fn encode_by_ref(&self, buf: &mut Vec<MssqlArgumentValue>) -> Result<IsNull, BoxDynError> {
        // Send the value as `uniqueidentifier` so the server does not have to
        // convert it from a character expression.
        buf.push(MssqlArgumentValue::new(SqlType::Uuid(Some(*self))));
        Ok(IsNull::No)
    }
}

impl<'r> Decode<'r, Mssql> for Uuid {
    fn decode(value: MssqlValueRef<'r>) -> Result<Self, BoxDynError> {
        match value.raw() {
            ColumnValues::Uuid(uuid) => Ok(*uuid),
            other => {
                Err(format!("cannot decode MSSQL {other:?} value into uuid::Uuid").into())
            }
        }
    }
}
