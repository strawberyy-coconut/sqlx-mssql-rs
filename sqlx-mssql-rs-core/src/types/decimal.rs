//! `rust_decimal::Decimal` support.
//!
//! As with `BigDecimal`, values are exchanged as text so the exact scale of the
//! server-side value is preserved.

use rust_decimal::Decimal;
use sqlx_core::decode::Decode;
use sqlx_core::encode::{Encode, IsNull};
use sqlx_core::error::BoxDynError;
use sqlx_core::types::Type;
use std::str::FromStr;

use crate::{Mssql, MssqlArgumentValue, MssqlTypeInfo, MssqlValueRef};

impl Type<Mssql> for Decimal {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::decimal()
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        ty.accepts_numeric_data() || ty.accepts_character_data()
    }
}

impl<'q> Encode<'q, Mssql> for Decimal {
    fn encode_by_ref(&self, buf: &mut Vec<MssqlArgumentValue>) -> Result<IsNull, BoxDynError> {
        buf.push(MssqlArgumentValue::text(self.to_string()));
        Ok(IsNull::No)
    }
}

impl<'r> Decode<'r, Mssql> for Decimal {
    fn decode(value: MssqlValueRef<'r>) -> Result<Self, BoxDynError> {
        // `decimal`/`numeric` arrive as text with their exact scale; integers and
        // floats are rendered first so they decode too.
        let text = match value.as_str() {
            Some(text) => text.to_owned(),
            None => match (value.as_i64(), value.as_f64()) {
                (Some(integer), _) => integer.to_string(),
                (None, Some(float)) => float.to_string(),
                (None, None) => return Err("cannot decode value into Decimal".into()),
            },
        };

        Decimal::from_str(text.trim()).map_err(Into::into)
    }
}
