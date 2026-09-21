//! `bigdecimal::BigDecimal` support.
//!
//! Values are exchanged as text: the TDS `decimal` wire form carries a
//! precision/scale pair that both directions of this conversion would have to
//! reproduce exactly, and SQL Server converts to `decimal` from a character
//! expression without loss for every value this driver can produce.

use bigdecimal::BigDecimal;
use sqlx_core::decode::Decode;
use sqlx_core::encode::{Encode, IsNull};
use sqlx_core::error::BoxDynError;
use sqlx_core::types::Type;
use std::str::FromStr;

use crate::{Mssql, MssqlArgumentValue, MssqlTypeInfo, MssqlValueRef};

impl Type<Mssql> for BigDecimal {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::decimal()
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        ty.accepts_numeric_data() || ty.accepts_character_data()
    }
}

impl<'q> Encode<'q, Mssql> for BigDecimal {
    fn encode_by_ref(&self, buf: &mut Vec<MssqlArgumentValue>) -> Result<IsNull, BoxDynError> {
        buf.push(MssqlArgumentValue::text(self.to_string()));
        Ok(IsNull::No)
    }
}

impl<'r> Decode<'r, Mssql> for BigDecimal {
    fn decode(value: MssqlValueRef<'r>) -> Result<Self, BoxDynError> {
        // `decimal`/`numeric` arrive as text with their exact scale; integers and
        // floats are rendered first so they decode too.
        let text = match value.as_str() {
            Some(text) => text.to_owned(),
            None => match (value.as_i64(), value.as_f64()) {
                (Some(integer), _) => integer.to_string(),
                (None, Some(float)) => float.to_string(),
                (None, None) => return Err("cannot decode value into BigDecimal".into()),
            },
        };

        BigDecimal::from_str(text.trim()).map_err(Into::into)
    }
}
