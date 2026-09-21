//! Bound parameter values and encode implementations for the MSSQL driver.

use mssql_tds::datatypes::sql_string::SqlString;
use mssql_tds::datatypes::sqldatatypes::TdsDataType;
use mssql_tds::datatypes::sqltypes::SqlType;

use crate::{Mssql, MssqlTypeInfo};

/// A bound parameter value.
///
/// `mssql-tds` models RPC parameters as a [`SqlType`], which carries both the
/// type to declare in the `sp_executesql` parameter list and the optional
/// value. This newtype keeps that as the driver's argument buffer so encoders
/// can express typed NULLs exactly.
#[derive(Debug, Clone, PartialEq)]
pub struct MssqlArgumentValue {
    value: SqlType,
}

impl MssqlArgumentValue {
    /// Wraps a raw TDS parameter value.
    pub fn new(value: SqlType) -> Self {
        Self { value }
    }

    /// Returns the underlying TDS parameter value.
    pub fn value(&self) -> &SqlType {
        &self.value
    }

    pub(crate) fn text(value: String) -> Self {
        Self::new(SqlType::NVarcharMax(Some(SqlString::from_utf8_string(value))))
    }

    pub(crate) fn bytes(value: Vec<u8>) -> Self {
        Self::new(SqlType::VarBinaryMax(Some(value)))
    }

    pub(crate) fn integer(value: i64) -> Self {
        Self::new(SqlType::BigInt(Some(value)))
    }

    pub(crate) fn bit(value: bool) -> Self {
        Self::new(SqlType::Bit(Some(value)))
    }

    pub(crate) fn real(value: f32) -> Self {
        Self::new(SqlType::Real(Some(value)))
    }

    pub(crate) fn double(value: f64) -> Self {
        Self::new(SqlType::Float(Some(value)))
    }

    /// Builds a typed SQL `NULL` matching the given type information.
    pub(crate) fn null_like(type_info: &MssqlTypeInfo) -> Self {
        Self::new(null_sql_type(type_info))
    }
}

/// Picks the `SqlType` used to declare a `NULL` parameter of the given type.
///
/// The declared type matters: `sp_executesql` emits an explicit parameter list,
/// and SQL Server needs a type it can convert to the target column.
fn null_sql_type(type_info: &MssqlTypeInfo) -> SqlType {
    match type_info.data_type() {
        Some(TdsDataType::Int1) => SqlType::TinyInt(None),
        Some(TdsDataType::Int2) => SqlType::SmallInt(None),
        Some(TdsDataType::Int4) => SqlType::Int(None),
        Some(TdsDataType::Int8 | TdsDataType::IntN) => SqlType::BigInt(None),
        Some(TdsDataType::Flt4) => SqlType::Real(None),
        Some(TdsDataType::Flt8 | TdsDataType::FltN) => SqlType::Float(None),
        Some(TdsDataType::Bit | TdsDataType::BitN) => SqlType::Bit(None),
        Some(TdsDataType::Decimal | TdsDataType::DecimalN) => SqlType::Decimal(None),
        Some(TdsDataType::Numeric | TdsDataType::NumericN) => SqlType::Numeric(None),
        Some(TdsDataType::Money) => SqlType::Money(None),
        Some(TdsDataType::Money4) => SqlType::SmallMoney(None),
        Some(TdsDataType::Char) => SqlType::Char(None, 0),
        Some(TdsDataType::NChar) => SqlType::NChar(None, 0),
        Some(TdsDataType::Text) => SqlType::Text(None),
        Some(TdsDataType::NText) => SqlType::NText(None),
        Some(TdsDataType::BigChar | TdsDataType::BigVarChar | TdsDataType::VarChar) => {
            SqlType::VarcharMax(None)
        }
        Some(TdsDataType::BigBinary | TdsDataType::Binary | TdsDataType::VarBinary) => {
            SqlType::VarBinaryMax(None)
        }
        Some(TdsDataType::Guid) => SqlType::Uuid(None),
        Some(TdsDataType::DateN) => SqlType::Date(None),
        Some(TdsDataType::TimeN) => SqlType::Time(None),
        Some(TdsDataType::DateTime2N) => SqlType::DateTime2(None),
        Some(TdsDataType::DateTimeOffsetN) => SqlType::DateTimeOffset(None),
        Some(TdsDataType::DateTime | TdsDataType::DateTimeN) => SqlType::DateTime(None),
        Some(TdsDataType::DateTim4) => SqlType::SmallDateTime(None),
        Some(TdsDataType::Xml) => SqlType::Xml(None),
        Some(TdsDataType::Json) => SqlType::Json(None),
        // `nvarchar(max)` is the safest fallback: SQL Server converts to most
        // target types from a character expression.
        _ => SqlType::NVarcharMax(None),
    }
}

/// Values that can be bound to MSSQL parameters.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct MssqlArguments {
    values: Vec<MssqlArgumentValue>,
}

impl MssqlArguments {
    /// Returns the number of bound arguments.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Returns `true` when no arguments have been bound.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Returns the raw argument values.
    pub fn values(&self) -> &[MssqlArgumentValue] {
        &self.values
    }
}

impl sqlx_core::arguments::Arguments for MssqlArguments {
    type Database = Mssql;

    fn reserve(&mut self, additional: usize, _size: usize) {
        self.values.reserve(additional);
    }

    fn add<'t, T>(&mut self, value: T) -> Result<(), sqlx_core::error::BoxDynError>
    where
        T: sqlx_core::encode::Encode<'t, Self::Database> + sqlx_core::types::Type<Self::Database>,
    {
        let _ = value.encode(&mut self.values)?;
        Ok(())
    }

    fn len(&self) -> usize {
        self.values.len()
    }
}

sqlx_core::impl_into_arguments_for_arguments!(MssqlArguments);

impl<'q, T> sqlx_core::encode::Encode<'q, Mssql> for Option<T>
where
    T: sqlx_core::encode::Encode<'q, Mssql> + sqlx_core::types::Type<Mssql> + 'q,
{
    fn encode(
        self,
        buf: &mut Vec<MssqlArgumentValue>,
    ) -> Result<sqlx_core::encode::IsNull, sqlx_core::error::BoxDynError> {
        match self {
            Some(value) => value.encode(buf),
            None => {
                buf.push(MssqlArgumentValue::null_like(&T::type_info()));
                Ok(sqlx_core::encode::IsNull::Yes)
            }
        }
    }

    fn encode_by_ref(
        &self,
        buf: &mut Vec<MssqlArgumentValue>,
    ) -> Result<sqlx_core::encode::IsNull, sqlx_core::error::BoxDynError> {
        match self {
            Some(value) => value.encode_by_ref(buf),
            None => {
                buf.push(MssqlArgumentValue::null_like(&T::type_info()));
                Ok(sqlx_core::encode::IsNull::Yes)
            }
        }
    }

    fn produces(&self) -> Option<MssqlTypeInfo> {
        match self {
            Some(value) => value.produces(),
            None => Some(T::type_info()),
        }
    }
}

macro_rules! impl_encode_integer {
    ($ty:ty) => {
        impl<'q> sqlx_core::encode::Encode<'q, Mssql> for $ty {
            fn encode_by_ref(
                &self,
                buf: &mut Vec<MssqlArgumentValue>,
            ) -> Result<sqlx_core::encode::IsNull, sqlx_core::error::BoxDynError> {
                buf.push(MssqlArgumentValue::integer(i64::from(*self)));
                Ok(sqlx_core::encode::IsNull::No)
            }
        }
    };
}

impl_encode_integer!(i8);
impl_encode_integer!(i16);
impl_encode_integer!(i32);
impl_encode_integer!(i64);
impl_encode_integer!(u8);
impl_encode_integer!(u16);
impl_encode_integer!(u32);

impl<'q> sqlx_core::encode::Encode<'q, Mssql> for u64 {
    fn encode_by_ref(
        &self,
        buf: &mut Vec<MssqlArgumentValue>,
    ) -> Result<sqlx_core::encode::IsNull, sqlx_core::error::BoxDynError> {
        // MSSQL has no unsigned 64-bit integer; anything above BIGINT's range
        // has to go through NUMERIC/DECIMAL instead.
        let value = i64::try_from(*self).map_err(|_| {
            format!(
                "u64 value {self} exceeds BIGINT range (max {}) and cannot be encoded \
                 for MSSQL; use NUMERIC/DECIMAL via rust_decimal for larger values",
                i64::MAX
            )
        })?;

        buf.push(MssqlArgumentValue::integer(value));
        Ok(sqlx_core::encode::IsNull::No)
    }
}

impl<'q> sqlx_core::encode::Encode<'q, Mssql> for bool {
    fn encode_by_ref(
        &self,
        buf: &mut Vec<MssqlArgumentValue>,
    ) -> Result<sqlx_core::encode::IsNull, sqlx_core::error::BoxDynError> {
        buf.push(MssqlArgumentValue::bit(*self));
        Ok(sqlx_core::encode::IsNull::No)
    }
}

impl<'q> sqlx_core::encode::Encode<'q, Mssql> for f32 {
    fn encode_by_ref(
        &self,
        buf: &mut Vec<MssqlArgumentValue>,
    ) -> Result<sqlx_core::encode::IsNull, sqlx_core::error::BoxDynError> {
        buf.push(MssqlArgumentValue::real(*self));
        Ok(sqlx_core::encode::IsNull::No)
    }
}

impl<'q> sqlx_core::encode::Encode<'q, Mssql> for f64 {
    fn encode_by_ref(
        &self,
        buf: &mut Vec<MssqlArgumentValue>,
    ) -> Result<sqlx_core::encode::IsNull, sqlx_core::error::BoxDynError> {
        buf.push(MssqlArgumentValue::double(*self));
        Ok(sqlx_core::encode::IsNull::No)
    }
}

impl sqlx_core::types::Type<Mssql> for str {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::nvarchar_max()
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        ty.accepts_character_data()
    }
}

impl<'q> sqlx_core::encode::Encode<'q, Mssql> for String {
    fn encode_by_ref(
        &self,
        buf: &mut Vec<MssqlArgumentValue>,
    ) -> Result<sqlx_core::encode::IsNull, sqlx_core::error::BoxDynError> {
        buf.push(MssqlArgumentValue::text(self.clone()));
        Ok(sqlx_core::encode::IsNull::No)
    }
}

impl<'q> sqlx_core::encode::Encode<'q, Mssql> for &'q str {
    fn encode_by_ref(
        &self,
        buf: &mut Vec<MssqlArgumentValue>,
    ) -> Result<sqlx_core::encode::IsNull, sqlx_core::error::BoxDynError> {
        buf.push(MssqlArgumentValue::text((*self).to_owned()));
        Ok(sqlx_core::encode::IsNull::No)
    }
}

impl sqlx_core::types::Type<Mssql> for [u8] {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::varbinary_max()
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        ty.accepts_binary_data()
    }
}

impl<'q> sqlx_core::encode::Encode<'q, Mssql> for Vec<u8> {
    fn encode_by_ref(
        &self,
        buf: &mut Vec<MssqlArgumentValue>,
    ) -> Result<sqlx_core::encode::IsNull, sqlx_core::error::BoxDynError> {
        buf.push(MssqlArgumentValue::bytes(self.clone()));
        Ok(sqlx_core::encode::IsNull::No)
    }
}

impl<'q> sqlx_core::encode::Encode<'q, Mssql> for &'q [u8] {
    fn encode_by_ref(
        &self,
        buf: &mut Vec<MssqlArgumentValue>,
    ) -> Result<sqlx_core::encode::IsNull, sqlx_core::error::BoxDynError> {
        buf.push(MssqlArgumentValue::bytes((*self).to_vec()));
        Ok(sqlx_core::encode::IsNull::No)
    }
}
