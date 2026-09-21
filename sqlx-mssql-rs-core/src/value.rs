//! Owned values and decode implementations for the MSSQL driver.

use std::borrow::Cow;
use std::fmt::Display;

use mssql_tds::core::TdsResult;
use mssql_tds::datatypes::column_values::ColumnValues;
use mssql_tds::datatypes::sql_string::SqlString;
use mssql_tds::query::metadata::ColumnMetadata;

use crate::{Mssql, MssqlTypeInfo};

/// A single MSSQL value.
///
/// The decoded TDS value is kept as-is rather than being re-modelled into a
/// driver-private enum: `mssql-tds` already hands back an owned, comparable
/// representation. The one thing it cannot provide is a borrowed `&str` for a
/// character or exact-numeric column, because it stores raw bytes, so the
/// textual form of those columns is cached alongside.
#[derive(Debug, Clone, PartialEq)]
pub struct MssqlValue {
    raw: ColumnValues,
    /// Textual form of a character or exact-numeric value, when there is one.
    text: Option<String>,
    type_info: MssqlTypeInfo,
}

impl MssqlValue {
    /// Creates a value from its decoded TDS representation.
    pub fn new(raw: ColumnValues, type_info: MssqlTypeInfo) -> Self {
        let text = textual_form(&raw);

        Self {
            raw,
            text,
            type_info,
        }
    }

    /// Builds a value from a TDS column value and its column metadata.
    pub(crate) fn from_column_value(value: &ColumnValues, column: &ColumnMetadata) -> Self {
        Self::new(value.clone(), MssqlTypeInfo::from_column_metadata(column))
    }

    /// Builds a SQL `NULL` of the given type.
    pub(crate) fn null(type_info: MssqlTypeInfo) -> Self {
        Self::new(ColumnValues::Null, type_info)
    }

    /// Returns the decoded TDS representation.
    pub fn raw(&self) -> &ColumnValues {
        &self.raw
    }

    /// Returns the type this value was read as.
    pub fn type_info(&self) -> &MssqlTypeInfo {
        &self.type_info
    }

    /// Returns whether this value is SQL `NULL`.
    pub fn is_null(&self) -> bool {
        matches!(self.raw, ColumnValues::Null)
    }

    /// Returns this value as a signed integer where possible.
    pub fn as_i64(&self) -> Option<i64> {
        match &self.raw {
            ColumnValues::TinyInt(value) => Some(i64::from(*value)),
            ColumnValues::SmallInt(value) => Some(i64::from(*value)),
            ColumnValues::Int(value) => Some(i64::from(*value)),
            ColumnValues::BigInt(value) => Some(*value),
            ColumnValues::Bit(value) => Some(i64::from(*value)),
            _ => self.text.as_deref().and_then(parse_integer_text),
        }
    }

    /// Returns this value as `f64` where possible.
    pub fn as_f64(&self) -> Option<f64> {
        match &self.raw {
            ColumnValues::Real(value) => Some(f64::from(*value)),
            ColumnValues::Float(value) => Some(*value),
            ColumnValues::TinyInt(value) => Some(f64::from(*value)),
            ColumnValues::SmallInt(value) => Some(f64::from(*value)),
            ColumnValues::Int(value) => Some(f64::from(*value)),
            ColumnValues::BigInt(value) => Some(*value as f64),
            // `money` is scaled by 10^4 on the wire; the conversion upstream is
            // fallible only in theory, so a failure means "not numeric".
            ColumnValues::Money(value) => {
                let converted: TdsResult<f64> = value.into();
                converted.ok()
            }
            ColumnValues::SmallMoney(value) => Some(f64::from(value.int_val) / 10_000.0),
            _ => self
                .text
                .as_deref()
                .and_then(|text| text.trim().parse().ok()),
        }
    }

    /// Returns this value as text where possible.
    pub fn as_str(&self) -> Option<Cow<'_, str>> {
        match &self.raw {
            // A GUID has no stored text form, so it is rendered on demand.
            ColumnValues::Uuid(value) => Some(Cow::Owned(guid_to_string(value.as_bytes()))),
            _ => self.text.as_deref().map(Cow::Borrowed),
        }
    }

    /// Returns this value as bytes where possible.
    pub fn as_bytes(&self) -> Option<Cow<'_, [u8]>> {
        match &self.raw {
            ColumnValues::Bytes(value) => Some(Cow::Borrowed(value)),
            ColumnValues::Uuid(value) => Some(Cow::Borrowed(value.as_bytes())),
            _ => self
                .text
                .as_deref()
                .map(|text| Cow::Borrowed(text.as_bytes())),
        }
    }

    /// Returns this value as a boolean where possible.
    pub fn as_bool(&self) -> Option<bool> {
        match &self.raw {
            ColumnValues::Bit(value) => Some(*value),
            ColumnValues::TinyInt(value) => Some(*value != 0),
            ColumnValues::SmallInt(value) => Some(*value != 0),
            ColumnValues::Int(value) => Some(*value != 0),
            ColumnValues::BigInt(value) => Some(*value != 0),
            ColumnValues::Real(value) => Some(*value != 0.0),
            ColumnValues::Float(value) => Some(*value != 0.0),
            _ => self.text.as_deref().and_then(parse_bool_text),
        }
    }
}

/// Borrowed MSSQL value reference.
#[derive(Debug, Clone, Copy)]
pub struct MssqlValueRef<'r> {
    value: &'r MssqlValue,
}

impl<'r> MssqlValueRef<'r> {
    /// Returns this value as a signed integer where possible.
    pub fn as_i64(&self) -> Option<i64> {
        self.value.as_i64()
    }

    /// Returns this value as `f64` where possible.
    pub fn as_f64(&self) -> Option<f64> {
        self.value.as_f64()
    }

    /// Returns this value as borrowed text where possible.
    ///
    /// Values whose textual form is computed on demand — `uniqueidentifier` is
    /// the notable one — return `None`; use [`MssqlValue::as_str`] for those.
    pub fn as_str(&self) -> Option<&'r str> {
        self.value.text.as_deref()
    }

    /// Returns this value as borrowed bytes where possible.
    pub fn as_bytes(&self) -> Option<&'r [u8]> {
        match &self.value.raw {
            ColumnValues::Bytes(value) => Some(value),
            ColumnValues::Uuid(value) => Some(value.as_bytes()),
            _ => self.value.text.as_deref().map(str::as_bytes),
        }
    }

    /// Returns this value as a boolean where possible.
    pub fn as_bool(&self) -> Option<bool> {
        self.value.as_bool()
    }

    /// Returns the decoded TDS representation.
    pub fn raw(&self) -> &'r ColumnValues {
        &self.value.raw
    }

    /// Returns this value's type information.
    pub fn type_info(&self) -> &'r MssqlTypeInfo {
        &self.value.type_info
    }
}

impl sqlx_core::value::Value for MssqlValue {
    type Database = Mssql;

    fn as_ref(&self) -> <Self::Database as sqlx_core::database::Database>::ValueRef<'_> {
        MssqlValueRef { value: self }
    }

    fn type_info(&self) -> Cow<'_, MssqlTypeInfo> {
        Cow::Borrowed(&self.type_info)
    }

    fn is_null(&self) -> bool {
        self.is_null()
    }
}

impl<'r> sqlx_core::value::ValueRef<'r> for MssqlValueRef<'r> {
    type Database = Mssql;

    fn to_owned(&self) -> MssqlValue {
        self.value.clone()
    }

    fn type_info(&self) -> Cow<'_, MssqlTypeInfo> {
        Cow::Borrowed(&self.value.type_info)
    }

    fn is_null(&self) -> bool {
        self.value.is_null()
    }
}

/// Builds the error returned when a value cannot be decoded into the target type.
fn decode_error(value: MssqlValueRef<'_>, target: &str, reason: impl Display) -> sqlx_core::Error {
    sqlx_core::Error::Decode(
        format!(
            "cannot decode MSSQL {} value into {target}: {reason}",
            value.value.type_info.type_name()
        )
        .into(),
    )
}

/// Returns the textual form of a value, when it has one worth caching.
fn textual_form(value: &ColumnValues) -> Option<String> {
    match value {
        ColumnValues::String(value) => Some(sql_string_to_utf8(value)),
        ColumnValues::Xml(value) => Some(utf16le_to_string(&value.bytes)),
        ColumnValues::Json(value) => Some(String::from_utf8_lossy(&value.bytes).into_owned()),
        ColumnValues::Decimal(parts) | ColumnValues::Numeric(parts) => Some(parts.to_string()),
        _ => None,
    }
}

/// Transcodes a TDS string using its own declared encoding.
pub(crate) fn sql_string_to_utf8(value: &SqlString) -> String {
    let (bytes, encoding) = value.clone().into_parts();
    SqlString::decode(&bytes, encoding)
}

/// Decodes UTF-16LE bytes without panicking on malformed input.
fn utf16le_to_string(bytes: &[u8]) -> String {
    let mut units = Vec::with_capacity(bytes.len() / 2);

    for index in (0..bytes.len().saturating_sub(1)).step_by(2) {
        units.push(u16::from_le_bytes([bytes[index], bytes[index + 1]]));
    }

    String::from_utf16_lossy(&units)
}

/// Renders a `uniqueidentifier` in its canonical textual form.
fn guid_to_string(bytes: &[u8; 16]) -> String {
    let hex = |range: std::ops::Range<usize>| -> String {
        bytes[range].iter().map(|byte| format!("{byte:02x}")).collect()
    };

    format!(
        "{}-{}-{}-{}-{}",
        hex(0..4),
        hex(4..6),
        hex(6..8),
        hex(8..10),
        hex(10..16)
    )
}

fn parse_integer_text(text: &str) -> Option<i64> {
    text.trim().parse().ok().or_else(|| {
        // A decimal read as an integer is only valid when it has no fraction.
        text.trim()
            .parse::<f64>()
            .ok()
            .and_then(|value| (value.fract() == 0.0).then_some(value as i64))
    })
}

fn parse_bool_text(text: &str) -> Option<bool> {
    match text.trim().to_ascii_lowercase().as_str() {
        "1" | "true" => Some(true),
        "0" | "false" => Some(false),
        _ => None,
    }
}

macro_rules! impl_decode_integer {
    ($ty:ty) => {
        impl sqlx_core::types::Type<Mssql> for $ty {
            fn type_info() -> MssqlTypeInfo {
                integer_type_info_for(stringify!($ty))
            }

            fn compatible(ty: &MssqlTypeInfo) -> bool {
                ty.accepts_numeric_data()
            }
        }

        impl<'r> sqlx_core::decode::Decode<'r, Mssql> for $ty {
            fn decode(value: MssqlValueRef<'r>) -> Result<Self, sqlx_core::error::BoxDynError> {
                let Some(integer) = value.as_i64() else {
                    return Err(
                        decode_error(value, stringify!($ty), "source value is not an integer")
                            .into(),
                    );
                };

                Self::try_from(integer).map_err(|_| {
                    decode_error(
                        value,
                        stringify!($ty),
                        format!("integer value {integer} is outside the target range"),
                    )
                    .into()
                })
            }
        }
    };
}

impl_decode_integer!(i8);
impl_decode_integer!(i16);
impl_decode_integer!(i32);
impl_decode_integer!(i64);
impl_decode_integer!(u8);
impl_decode_integer!(u16);
impl_decode_integer!(u32);
impl_decode_integer!(u64);

/// Type information matching the narrowest SQL Server integer type for `ty`.
fn integer_type_info_for(ty: &str) -> MssqlTypeInfo {
    match ty {
        "i8" | "u8" => MssqlTypeInfo::tinyint(),
        "i16" | "u16" => MssqlTypeInfo::smallint(),
        "i32" | "u32" => MssqlTypeInfo::integer(),
        _ => MssqlTypeInfo::bigint(),
    }
}

impl sqlx_core::types::Type<Mssql> for bool {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::bit()
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        // Deliberately narrow: `compatible` drives compile-time type inference,
        // and a permissive `bool` would claim every numeric column.
        ty.accepts_boolean_data()
    }
}

impl<'r> sqlx_core::decode::Decode<'r, Mssql> for bool {
    fn decode(value: MssqlValueRef<'r>) -> Result<Self, sqlx_core::error::BoxDynError> {
        value.as_bool().ok_or_else(|| {
            decode_error(value, "bool", "source value is not boolean-compatible").into()
        })
    }
}

impl sqlx_core::types::Type<Mssql> for f32 {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::real()
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        ty.accepts_numeric_data()
    }
}

impl<'r> sqlx_core::decode::Decode<'r, Mssql> for f32 {
    fn decode(value: MssqlValueRef<'r>) -> Result<Self, sqlx_core::error::BoxDynError> {
        value
            .as_f64()
            .map(|value| value as f32)
            .ok_or_else(|| decode_error(value, "f32", "source value is not numeric").into())
    }
}

impl sqlx_core::types::Type<Mssql> for f64 {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::double()
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        ty.accepts_numeric_data()
    }
}

impl<'r> sqlx_core::decode::Decode<'r, Mssql> for f64 {
    fn decode(value: MssqlValueRef<'r>) -> Result<Self, sqlx_core::error::BoxDynError> {
        value
            .as_f64()
            .ok_or_else(|| decode_error(value, "f64", "source value is not numeric").into())
    }
}

impl sqlx_core::types::Type<Mssql> for String {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::nvarchar_max()
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        ty.accepts_character_data()
    }
}

impl<'r> sqlx_core::decode::Decode<'r, Mssql> for String {
    fn decode(value: MssqlValueRef<'r>) -> Result<Self, sqlx_core::error::BoxDynError> {
        if let Some(text) = value.as_str() {
            return Ok(text.to_owned());
        }

        if let Some(bytes) = value.as_bytes() {
            return Ok(String::from_utf8_lossy(bytes).into_owned());
        }

        Err(decode_error(value, "String", "source value is not text-compatible").into())
    }
}

impl<'r> sqlx_core::decode::Decode<'r, Mssql> for &'r str {
    fn decode(value: MssqlValueRef<'r>) -> Result<Self, sqlx_core::error::BoxDynError> {
        value
            .as_str()
            .ok_or_else(|| decode_error(value, "&str", "source value is not borrowed text").into())
    }
}

impl sqlx_core::types::Type<Mssql> for Vec<u8> {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::varbinary_max()
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        ty.accepts_binary_data()
    }
}

impl<'r> sqlx_core::decode::Decode<'r, Mssql> for Vec<u8> {
    fn decode(value: MssqlValueRef<'r>) -> Result<Self, sqlx_core::error::BoxDynError> {
        value
            .as_bytes()
            .map(|bytes| bytes.to_vec())
            .ok_or_else(|| decode_error(value, "Vec<u8>", "source value is not bytes").into())
    }
}

impl<'r> sqlx_core::decode::Decode<'r, Mssql> for &'r [u8] {
    fn decode(value: MssqlValueRef<'r>) -> Result<Self, sqlx_core::error::BoxDynError> {
        value.as_bytes().ok_or_else(|| {
            decode_error(value, "&[u8]", "source value is not borrowed bytes").into()
        })
    }
}
