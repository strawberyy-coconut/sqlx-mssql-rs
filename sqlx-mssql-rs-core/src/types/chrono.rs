//! `chrono` date and time support.

use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, NaiveTime, Timelike, Utc};

use mssql_tds::datatypes::column_values::{
    ColumnValues, SqlDate, SqlDateTime, SqlDateTime2, SqlDateTimeOffset, SqlSmallDateTime, SqlTime,
};
use mssql_tds::datatypes::sqltypes::SqlType;

use sqlx_core::decode::Decode;
use sqlx_core::encode::{Encode, IsNull};
use sqlx_core::error::BoxDynError;
use sqlx_core::types::Type;

use crate::{Mssql, MssqlArgumentValue, MssqlTypeInfo, MssqlValueRef};

/// Days between 0001-01-01 (the TDS epoch for `date`) and 1970-01-01.
const DAYS_FROM_YEAR_ONE_TO_EPOCH: i64 = 719_162;

/// Nanoseconds in one second.
const NANOS_PER_SECOND: u64 = 1_000_000_000;

fn date_from_days(days: u32) -> Option<NaiveDate> {
    let unix_days = i64::from(days) - DAYS_FROM_YEAR_ONE_TO_EPOCH;
    let epoch = NaiveDate::from_ymd_opt(1970, 1, 1)?;
    epoch.checked_add_days(chrono::Days::new(u64::try_from(unix_days).ok()?))
}

fn days_from_date(date: NaiveDate) -> Option<u32> {
    let epoch = NaiveDate::from_ymd_opt(1970, 1, 1)?;
    let unix_days = date.signed_duration_since(epoch).num_days();
    u32::try_from(unix_days + DAYS_FROM_YEAR_ONE_TO_EPOCH).ok()
}

fn time_from_nanos(nanoseconds: u64) -> Option<NaiveTime> {
    let seconds = u32::try_from(nanoseconds / NANOS_PER_SECOND).ok()?;
    let subsecond = u32::try_from(nanoseconds % NANOS_PER_SECOND).ok()?;
    NaiveTime::from_num_seconds_from_midnight_opt(seconds, subsecond)
}

fn nanos_from_time(time: NaiveTime) -> u64 {
    u64::from(time.num_seconds_from_midnight()) * NANOS_PER_SECOND + u64::from(time.nanosecond())
}

fn sql_time_from_time(time: NaiveTime) -> SqlTime {
    let (time_nanoseconds, scale) = super::nanos_to_ticks(nanos_from_time(time));

    SqlTime {
        time_nanoseconds,
        scale,
    }
}

fn datetime_from_sql_date_time(value: &SqlDateTime) -> Option<NaiveDateTime> {
    let epoch = NaiveDate::from_ymd_opt(1900, 1, 1)?;
    let date = if value.days >= 0 {
        epoch.checked_add_days(chrono::Days::new(u64::try_from(value.days).ok()?))?
    } else {
        epoch.checked_sub_days(chrono::Days::new(u64::try_from(-value.days).ok()?))?
    };

    // `datetime` counts 1/300ths of a second since midnight.
    let nanoseconds = u64::from(value.time) * NANOS_PER_SECOND / 300;
    Some(NaiveDateTime::new(date, time_from_nanos(nanoseconds)?))
}

fn datetime_from_small_date_time(value: &SqlSmallDateTime) -> Option<NaiveDateTime> {
    let epoch = NaiveDate::from_ymd_opt(1900, 1, 1)?;
    let date = epoch.checked_add_days(chrono::Days::new(u64::from(value.days)))?;
    let time = NaiveTime::from_num_seconds_from_midnight_opt(u32::from(value.time) * 60, 0)?;
    Some(NaiveDateTime::new(date, time))
}

fn naive_from_datetime2(value: &SqlDateTime2) -> Option<NaiveDateTime> {
    Some(NaiveDateTime::new(
        date_from_days(value.days)?,
        time_from_nanos(super::ticks_to_nanos(value.time.time_nanoseconds))?,
    ))
}

fn datetime2_from_naive(value: NaiveDateTime) -> Option<SqlDateTime2> {
    Some(SqlDateTime2 {
        days: days_from_date(value.date())?,
        time: sql_time_from_time(value.time()),
    })
}

impl Type<Mssql> for NaiveDate {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::date()
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        matches!(
            ty.data_type(),
            Some(
                mssql_tds::datatypes::sqldatatypes::TdsDataType::DateN
                    | mssql_tds::datatypes::sqldatatypes::TdsDataType::DateTime2N
                    | mssql_tds::datatypes::sqldatatypes::TdsDataType::DateTime
                    | mssql_tds::datatypes::sqldatatypes::TdsDataType::DateTim4
            )
        ) || ty.accepts_datetime_data()
    }
}

impl<'q> Encode<'q, Mssql> for NaiveDate {
    fn encode_by_ref(&self, buf: &mut Vec<MssqlArgumentValue>) -> Result<IsNull, BoxDynError> {
        let days = days_from_date(*self)
            .ok_or_else(|| format!("date {self} is outside the range SQL Server DATE accepts"))?;
        let value = SqlDate::create(days)?;
        buf.push(MssqlArgumentValue::new(SqlType::Date(Some(value))));
        Ok(IsNull::No)
    }
}

impl<'r> Decode<'r, Mssql> for NaiveDate {
    fn decode(value: MssqlValueRef<'r>) -> Result<Self, BoxDynError> {
        match value.raw() {
            ColumnValues::Date(date) => date_from_days(date.get_days())
                .ok_or_else(|| "DATE value is out of range for chrono".into()),
            ColumnValues::DateTime(datetime) => datetime_from_sql_date_time(datetime)
                .map(|value| value.date())
                .ok_or_else(|| "DATETIME value is out of range for chrono".into()),
            ColumnValues::SmallDateTime(datetime) => datetime_from_small_date_time(datetime)
                .map(|value| value.date())
                .ok_or_else(|| "SMALLDATETIME value is out of range for chrono".into()),
            ColumnValues::DateTime2(datetime) => naive_from_datetime2(datetime)
                .map(|value| value.date())
                .ok_or_else(|| "DATETIME2 value is out of range for chrono".into()),
            ColumnValues::DateTimeOffset(datetime) => naive_from_datetime2(&datetime.datetime2)
                .map(|value| value.date())
                .ok_or_else(|| "DATETIMEOFFSET value is out of range for chrono".into()),
            other => Err(format!("cannot decode {other:?} into NaiveDate").into()),
        }
    }
}

impl Type<Mssql> for NaiveTime {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::time()
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        ty.accepts_datetime_data()
    }
}

impl<'q> Encode<'q, Mssql> for NaiveTime {
    fn encode_by_ref(&self, buf: &mut Vec<MssqlArgumentValue>) -> Result<IsNull, BoxDynError> {
        buf.push(MssqlArgumentValue::new(SqlType::Time(Some(
            sql_time_from_time(*self),
        ))));
        Ok(IsNull::No)
    }
}

impl<'r> Decode<'r, Mssql> for NaiveTime {
    fn decode(value: MssqlValueRef<'r>) -> Result<Self, BoxDynError> {
        match value.raw() {
            ColumnValues::Time(time) => {
                time_from_nanos(super::ticks_to_nanos(time.time_nanoseconds))
                    .ok_or_else(|| "TIME value is out of range for chrono".into())
            }
            ColumnValues::DateTime(datetime) => datetime_from_sql_date_time(datetime)
                .map(|value| value.time())
                .ok_or_else(|| "DATETIME value is out of range for chrono".into()),
            ColumnValues::SmallDateTime(datetime) => datetime_from_small_date_time(datetime)
                .map(|value| value.time())
                .ok_or_else(|| "SMALLDATETIME value is out of range for chrono".into()),
            ColumnValues::DateTime2(datetime) => naive_from_datetime2(datetime)
                .map(|value| value.time())
                .ok_or_else(|| "DATETIME2 value is out of range for chrono".into()),
            ColumnValues::DateTimeOffset(datetime) => naive_from_datetime2(&datetime.datetime2)
                .map(|value| value.time())
                .ok_or_else(|| "DATETIMEOFFSET value is out of range for chrono".into()),
            other => Err(format!("cannot decode {other:?} into NaiveTime").into()),
        }
    }
}

impl Type<Mssql> for NaiveDateTime {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::datetime2()
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        ty.accepts_datetime_data()
    }
}

impl<'q> Encode<'q, Mssql> for NaiveDateTime {
    fn encode_by_ref(&self, buf: &mut Vec<MssqlArgumentValue>) -> Result<IsNull, BoxDynError> {
        let value = datetime2_from_naive(*self)
            .ok_or_else(|| format!("{self} is outside the range SQL Server DATETIME2 accepts"))?;
        buf.push(MssqlArgumentValue::new(SqlType::DateTime2(Some(value))));
        Ok(IsNull::No)
    }
}

impl<'r> Decode<'r, Mssql> for NaiveDateTime {
    fn decode(value: MssqlValueRef<'r>) -> Result<Self, BoxDynError> {
        match value.raw() {
            ColumnValues::DateTime2(datetime) => naive_from_datetime2(datetime)
                .ok_or_else(|| "DATETIME2 value is out of range for chrono".into()),
            ColumnValues::DateTimeOffset(datetime) => naive_from_datetime2(&datetime.datetime2)
                .ok_or_else(|| "DATETIMEOFFSET value is out of range for chrono".into()),
            ColumnValues::DateTime(datetime) => datetime_from_sql_date_time(datetime)
                .ok_or_else(|| "DATETIME value is out of range for chrono".into()),
            ColumnValues::SmallDateTime(datetime) => datetime_from_small_date_time(datetime)
                .ok_or_else(|| "SMALLDATETIME value is out of range for chrono".into()),
            ColumnValues::Date(date) => date_from_days(date.get_days())
                .map(|date| NaiveDateTime::new(date, NaiveTime::MIN))
                .ok_or_else(|| "DATE value is out of range for chrono".into()),
            other => Err(format!("cannot decode {other:?} into NaiveDateTime").into()),
        }
    }
}

impl Type<Mssql> for DateTime<Utc> {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::datetimeoffset()
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        ty.accepts_datetime_data()
    }
}

impl<'q> Encode<'q, Mssql> for DateTime<Utc> {
    fn encode_by_ref(&self, buf: &mut Vec<MssqlArgumentValue>) -> Result<IsNull, BoxDynError> {
        let naive = self.naive_utc();
        let datetime2 = datetime2_from_naive(naive).ok_or_else(|| {
            format!("{self} is outside the range SQL Server DATETIMEOFFSET accepts")
        })?;

        buf.push(MssqlArgumentValue::new(SqlType::DateTimeOffset(Some(
            SqlDateTimeOffset {
                datetime2,
                offset: 0,
            },
        ))));
        Ok(IsNull::No)
    }
}

impl<'r> Decode<'r, Mssql> for DateTime<Utc> {
    fn decode(value: MssqlValueRef<'r>) -> Result<Self, BoxDynError> {
        match value.raw() {
            ColumnValues::DateTimeOffset(datetime) => {
                // `mssql-tds` normalises the time part to UTC and keeps the
                // original offset beside it, so the offset must not be applied
                // again to recover the instant.
                naive_from_datetime2(&datetime.datetime2)
                    .map(|naive| DateTime::from_naive_utc_and_offset(naive, Utc))
                    .ok_or_else(|| "DATETIMEOFFSET value is out of range for chrono".into())
            }
            ColumnValues::DateTime2(datetime) => naive_from_datetime2(datetime)
                .map(|naive| DateTime::from_naive_utc_and_offset(naive, Utc))
                .ok_or_else(|| "DATETIME2 value is out of range for chrono".into()),
            ColumnValues::DateTime(datetime) => datetime_from_sql_date_time(datetime)
                .map(|naive| DateTime::from_naive_utc_and_offset(naive, Utc))
                .ok_or_else(|| "DATETIME value is out of range for chrono".into()),
            ColumnValues::SmallDateTime(datetime) => datetime_from_small_date_time(datetime)
                .map(|naive| DateTime::from_naive_utc_and_offset(naive, Utc))
                .ok_or_else(|| "SMALLDATETIME value is out of range for chrono".into()),
            other => Err(format!("cannot decode {other:?} into DateTime<Utc>").into()),
        }
    }
}

/// Keeps the `Datelike`/`Timelike` imports live; they are required for the
/// accessor methods used above.
#[allow(dead_code)]
fn _assert_traits_used(date: NaiveDate, time: NaiveTime) -> (i32, u32, u32) {
    (date.year(), time.hour(), time.nanosecond())
}
