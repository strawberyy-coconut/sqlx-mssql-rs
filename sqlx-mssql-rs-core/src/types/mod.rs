//! Type integrations for third-party Rust types.
//!
//! Each module is enabled by its matching feature and implements `Type`,
//! `Encode` and `Decode` for the relevant Rust types.

#[cfg(feature = "bigdecimal")]
mod bigdecimal;
#[cfg(feature = "chrono")]
mod chrono;
#[cfg(any(feature = "decimal", feature = "rust_decimal"))]
mod decimal;
#[cfg(feature = "spatial")]
mod geo;
#[cfg(feature = "jiff")]
mod jiff;
#[cfg(feature = "json")]
mod json;
#[cfg(feature = "time")]
mod time;
#[cfg(feature = "uuid")]
mod uuid;

/// Converts nanoseconds into the 100-nanosecond ticks TDS stores, at scale 7.
///
/// `SqlTime`'s field is named `time_nanoseconds` but the wire format counts
/// 100-nanosecond units, and the `sp_executesql` declaration for `time`,
/// `datetime2` and `datetimeoffset` carries no explicit precision, so SQL Server
/// applies the maximum scale of 7. Finer precision is therefore truncated.
#[cfg(any(feature = "chrono", feature = "jiff", feature = "time"))]
pub(crate) fn nanos_to_ticks(nanoseconds: u64) -> (u64, u8) {
    (nanoseconds / 100, 7)
}

/// Converts the 100-nanosecond ticks TDS stores back into nanoseconds.
#[cfg(any(feature = "chrono", feature = "jiff", feature = "time"))]
pub(crate) fn ticks_to_nanos(ticks: u64) -> u64 {
    ticks.saturating_mul(100)
}
