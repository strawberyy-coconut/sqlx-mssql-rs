//! `time` crate date and time support.

use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};

use mssql_tds::datatypes::column_values::{ColumnValues, SqlDate, SqlDateTime2, SqlDateTimeOffset, SqlTime};
use mssql_tds::datatypes::sqltypes::SqlType;

use sqlx_core::decode::Decode;
use sqlx_core::encode::{Encode, IsNull};
use sqlx_core::error::BoxDynError;
use sqlx_core::types::Type;

use crate::{Mssql, MssqlArgumentValue, MssqlTypeInfo, MssqlValueRef};

/// Days between 0001-01-01 (the TDS epoch for `date`) and 1970-01-01.
const DAYS_FROM_YEAR_ONE_TO_EPOCH: i64 = 719_162;

const NANOS_PER_SECOND: u64 = 1_000_000_000;

/// Used to split a time of day into its hour, minute and second parts.
const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = 60 * SECONDS_PER_MINUTE;

fn date_from_days(days: u32) -> Option<Date> {
    let unix_days = i64::from(days) - DAYS_FROM_YEAR_ONE_TO_EPOCH;
    let epoch = Date::from_calendar_date(1970, Month::January, 1).ok()?;
    epoch.checked_add(time::Duration::days(unix_days))
}

fn days_from_date(date: Date) -> Option<u32> {
    let epoch = Date::from_calendar_date(1970, Month::January, 1).ok()?;
    let unix_days = (date - epoch).whole_days();
    u32::try_from(unix_days + DAYS_FROM_YEAR_ONE_TO_EPOCH).ok()
}

fn time_from_nanos(nanoseconds: u64) -> Option<Time> {
    // `SqlTime` counts nanoseconds since midnight, so the hour, minute and
    // second all have to be recovered from it.
    let total_seconds = nanoseconds / NANOS_PER_SECOND;
    let subsecond = u32::try_from(nanoseconds % NANOS_PER_SECOND).ok()?;

    let hour = u8::try_from(total_seconds / SECONDS_PER_HOUR).ok()?;
    let minute = u8::try_from((total_seconds % SECONDS_PER_HOUR) / SECONDS_PER_MINUTE).ok()?;
    let second = u8::try_from(total_seconds % SECONDS_PER_MINUTE).ok()?;

    Time::from_hms_nano(hour, minute, second, subsecond).ok()
}

fn nanos_from_time(time: Time) -> u64 {
    let seconds = u64::from(time.hour()) * SECONDS_PER_HOUR
        + u64::from(time.minute()) * SECONDS_PER_MINUTE
        + u64::from(time.second());

    seconds * NANOS_PER_SECOND + u64::from(time.nanosecond())
}

fn datetime2_from_primitive(value: PrimitiveDateTime) -> Option<SqlDateTime2> {
    let (time_nanoseconds, scale) = super::nanos_to_ticks(nanos_from_time(value.time()));

    Some(SqlDateTime2 {
        days: days_from_date(value.date())?,
        time: SqlTime {
            time_nanoseconds,
            scale,
        },
    })
}

fn primitive_from_datetime2(value: &SqlDateTime2) -> Option<PrimitiveDateTime> {
    Some(PrimitiveDateTime::new(
        date_from_days(value.days)?,
        time_from_nanos(super::ticks_to_nanos(value.time.time_nanoseconds))?,
    ))
}

impl Type<Mssql> for Date {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::date()
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        ty.accepts_datetime_data()
    }
}

impl<'q> Encode<'q, Mssql> for Date {
    fn encode_by_ref(&self, buf: &mut Vec<MssqlArgumentValue>) -> Result<IsNull, BoxDynError> {
        let days = days_from_date(*self)
            .ok_or_else(|| format!("date {self} is outside the range SQL Server DATE accepts"))?;
        let value = SqlDate::create(days)?;
        buf.push(MssqlArgumentValue::new(SqlType::Date(Some(value))));
        Ok(IsNull::No)
    }
}

impl<'r> Decode<'r, Mssql> for Date {
    fn decode(value: MssqlValueRef<'r>) -> Result<Self, BoxDynError> {
        match value.raw() {
            ColumnValues::Date(date) => date_from_days(date.get_days())
                .ok_or_else(|| "DATE value is out of range for time::Date".into()),
            ColumnValues::DateTime2(datetime) => primitive_from_datetime2(datetime)
                .map(|value| value.date())
                .ok_or_else(|| "DATETIME2 value is out of range for time::Date".into()),
            ColumnValues::DateTimeOffset(datetime) => primitive_from_datetime2(&datetime.datetime2)
                .map(|value| value.date())
                .ok_or_else(|| "DATETIMEOFFSET value is out of range for time::Date".into()),
            other => Err(format!("cannot decode {other:?} into time::Date").into()),
        }
    }
}

impl Type<Mssql> for Time {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::time()
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        ty.accepts_datetime_data()
    }
}

impl<'q> Encode<'q, Mssql> for Time {
    fn encode_by_ref(&self, buf: &mut Vec<MssqlArgumentValue>) -> Result<IsNull, BoxDynError> {
        let (time_nanoseconds, scale) = super::nanos_to_ticks(nanos_from_time(*self));

        buf.push(MssqlArgumentValue::new(SqlType::Time(Some(SqlTime {
            time_nanoseconds,
            scale,
        }))));
        Ok(IsNull::No)
    }
}

impl<'r> Decode<'r, Mssql> for Time {
    fn decode(value: MssqlValueRef<'r>) -> Result<Self, BoxDynError> {
        match value.raw() {
            ColumnValues::Time(time) => {
                time_from_nanos(super::ticks_to_nanos(time.time_nanoseconds))
                    .ok_or_else(|| "TIME value is out of range for time::Time".into())
            }
            ColumnValues::DateTime2(datetime) => primitive_from_datetime2(datetime)
                .map(|value| value.time())
                .ok_or_else(|| "DATETIME2 value is out of range for time::Time".into()),
            ColumnValues::DateTimeOffset(datetime) => primitive_from_datetime2(&datetime.datetime2)
                .map(|value| value.time())
                .ok_or_else(|| "DATETIMEOFFSET value is out of range for time::Time".into()),
            other => Err(format!("cannot decode {other:?} into time::Time").into()),
        }
    }
}

impl Type<Mssql> for PrimitiveDateTime {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::datetime2()
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        ty.accepts_datetime_data()
    }
}

impl<'q> Encode<'q, Mssql> for PrimitiveDateTime {
    fn encode_by_ref(&self, buf: &mut Vec<MssqlArgumentValue>) -> Result<IsNull, BoxDynError> {
        let value = datetime2_from_primitive(*self)
            .ok_or_else(|| format!("{self} is outside the range SQL Server DATETIME2 accepts"))?;
        buf.push(MssqlArgumentValue::new(SqlType::DateTime2(Some(value))));
        Ok(IsNull::No)
    }
}

impl<'r> Decode<'r, Mssql> for PrimitiveDateTime {
    fn decode(value: MssqlValueRef<'r>) -> Result<Self, BoxDynError> {
        match value.raw() {
            ColumnValues::DateTime2(datetime) => primitive_from_datetime2(datetime)
                .ok_or_else(|| "DATETIME2 value is out of range for time::PrimitiveDateTime".into()),
            ColumnValues::DateTimeOffset(datetime) => primitive_from_datetime2(&datetime.datetime2)
                .ok_or_else(|| {
                    "DATETIMEOFFSET value is out of range for time::PrimitiveDateTime".into()
                }),
            other => {
                Err(format!("cannot decode {other:?} into time::PrimitiveDateTime").into())
            }
        }
    }
}

impl Type<Mssql> for OffsetDateTime {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::datetimeoffset()
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        ty.accepts_datetime_data()
    }
}

impl<'q> Encode<'q, Mssql> for OffsetDateTime {
    fn encode_by_ref(&self, buf: &mut Vec<MssqlArgumentValue>) -> Result<IsNull, BoxDynError> {
        // The offset is always written as UTC, so drop it explicitly: `time`
        // provides no `From<OffsetDateTime>` conversion to build on.
        let utc = self.to_offset(UtcOffset::UTC);
        let value = datetime2_from_primitive(PrimitiveDateTime::new(utc.date(), utc.time()))
            .ok_or_else(|| {
                format!("{self} is outside the range SQL Server DATETIMEOFFSET accepts")
            })?;

        buf.push(MssqlArgumentValue::new(SqlType::DateTimeOffset(Some(
            SqlDateTimeOffset {
                datetime2: value,
                offset: 0,
            },
        ))));
        Ok(IsNull::No)
    }
}

impl<'r> Decode<'r, Mssql> for OffsetDateTime {
    fn decode(value: MssqlValueRef<'r>) -> Result<Self, BoxDynError> {
        match value.raw() {
            ColumnValues::DateTimeOffset(datetime) => {
                let naive = primitive_from_datetime2(&datetime.datetime2).ok_or(
                    "DATETIMEOFFSET value is out of range for time::OffsetDateTime",
                )?;
                let offset = UtcOffset::from_whole_seconds(i32::from(datetime.offset) * 60)?;

                Ok(naive.assume_offset(offset))
            }
            ColumnValues::DateTime2(datetime) => primitive_from_datetime2(datetime)
                .map(|value| value.assume_utc())
                .ok_or_else(|| "DATETIME2 value is out of range for time::OffsetDateTime".into()),
            other => Err(format!("cannot decode {other:?} into time::OffsetDateTime").into()),
        }
    }
}
