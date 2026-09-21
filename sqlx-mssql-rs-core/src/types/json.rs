//! JSON support, layered on the `json` SQL Server type or a character column.

use serde::de::DeserializeOwned;
use serde::Serialize;
use sqlx_core::decode::Decode;
use sqlx_core::encode::{Encode, IsNull};
use sqlx_core::error::BoxDynError;
use sqlx_core::types::Json;
use sqlx_core::types::Type;

use crate::{Mssql, MssqlArgumentValue, MssqlTypeInfo, MssqlValueRef};

impl<T> Type<Mssql> for Json<T> {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::json()
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        ty.accepts_character_data() || ty.accepts_binary_data()
    }
}

impl<'q, T> Encode<'q, Mssql> for Json<T>
where
    T: Serialize,
{
    fn encode_by_ref(&self, buf: &mut Vec<MssqlArgumentValue>) -> Result<IsNull, BoxDynError> {
        // The value is sent as JSON text; SQL Server converts it to `json` or
        // `nvarchar` depending on the target column.
        let json = serde_json::to_string(&self.0)?;
        buf.push(MssqlArgumentValue::text(json));
        Ok(IsNull::No)
    }
}

impl<'r, T> Decode<'r, Mssql> for Json<T>
where
    T: DeserializeOwned,
{
    fn decode(value: MssqlValueRef<'r>) -> Result<Self, BoxDynError> {
        // A `json` column arrives as text, but a `varbinary` column holding JSON
        // comes back as bytes.
        let text = match value.as_str() {
            Some(text) => text.to_owned(),
            None => match value.as_bytes() {
                Some(bytes) => String::from_utf8(bytes.to_vec())?,
                None => return Err("cannot decode value into JSON".into()),
            },
        };

        Ok(Json(serde_json::from_str(&text)?))
    }
}
