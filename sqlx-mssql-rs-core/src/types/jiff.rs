//! `jiff` date and time support.

use jiff::Timestamp;
use jiff::civil::{Date, DateTime, Time};
use jiff::tz::TimeZone;

use mssql_tds::datatypes::column_values::{
    ColumnValues, SqlDate, SqlDateTime2, SqlDateTimeOffset, SqlTime,
};
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

/// The number of days from 1970-01-01 for a civil date.
///
/// `jiff` has no public day-number API, and going through an instant would
/// inherit the narrower range of `Timestamp`, so this is the usual
/// days-from-civil calculation for the proleptic Gregorian calendar.
fn unix_days_from_date(date: Date) -> i64 {
    let year = i64::from(date.year());
    let month = i64::from(date.month());
    let day = i64::from(date.day());

    let adjusted_year = year - i64::from(month <= 2);
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year - era * 400;
    let month_shift = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_shift + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;

    era * 146_097 + day_of_era - 719_468
}

/// The inverse of [`unix_days_from_date`].
fn date_from_unix_days(unix_days: i64) -> Option<Date> {
    let shifted = unix_days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_shift = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_shift + 2) / 5 + 1;
    let month = if month_shift < 10 {
        month_shift + 3
    } else {
        month_shift - 9
    };
    let year = year + i64::from(month <= 2);

    Date::new(
        i16::try_from(year).ok()?,
        i8::try_from(month).ok()?,
        i8::try_from(day).ok()?,
    )
    .ok()
}

fn date_from_days(days: u32) -> Option<Date> {
    date_from_unix_days(i64::from(days) - DAYS_FROM_YEAR_ONE_TO_EPOCH)
}

fn days_from_date(date: Date) -> Option<u32> {
    u32::try_from(unix_days_from_date(date) + DAYS_FROM_YEAR_ONE_TO_EPOCH).ok()
}

/// Nanoseconds since midnight for a civil time.
fn nanos_from_time(time: Time) -> Option<u64> {
    let seconds = u64::try_from(time.hour()).ok()? * SECONDS_PER_HOUR
        + u64::try_from(time.minute()).ok()? * SECONDS_PER_MINUTE
        + u64::try_from(time.second()).ok()?;

    Some(seconds * NANOS_PER_SECOND + u64::try_from(time.subsec_nanosecond()).ok()?)
}

fn time_from_nanos(nanoseconds: u64) -> Option<Time> {
    // `SqlTime` counts nanoseconds since midnight, so the hour, minute and
    // second all have to be recovered from it.
    let total_seconds = nanoseconds / NANOS_PER_SECOND;
    let subsecond = i32::try_from(nanoseconds % NANOS_PER_SECOND).ok()?;

    Time::new(
        i8::try_from(total_seconds / SECONDS_PER_HOUR).ok()?,
        i8::try_from((total_seconds % SECONDS_PER_HOUR) / SECONDS_PER_MINUTE).ok()?,
        i8::try_from(total_seconds % SECONDS_PER_MINUTE).ok()?,
        subsecond,
    )
    .ok()
}

fn sql_time_from_time(time: Time) -> Option<SqlTime> {
    let (time_nanoseconds, scale) = super::nanos_to_ticks(nanos_from_time(time)?);

    Some(SqlTime {
        time_nanoseconds,
        scale,
    })
}

fn datetime2_from_civil(value: DateTime) -> Option<SqlDateTime2> {
    Some(SqlDateTime2 {
        days: days_from_date(value.date())?,
        time: sql_time_from_time(value.time())?,
    })
}

fn civil_from_datetime2(value: &SqlDateTime2) -> Option<DateTime> {
    let date = date_from_days(value.days)?;
    let time = time_from_nanos(super::ticks_to_nanos(value.time.time_nanoseconds))?;

    DateTime::new(
        date.year(),
        date.month(),
        date.day(),
        time.hour(),
        time.minute(),
        time.second(),
        time.subsec_nanosecond(),
    )
    .ok()
}

/// The instant a `DATETIME2` value represents, read as UTC.
fn timestamp_from_datetime2(value: &SqlDateTime2) -> Option<Timestamp> {
    Some(
        civil_from_datetime2(value)?
            .to_zoned(TimeZone::UTC)
            .ok()?
            .timestamp(),
    )
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
                .ok_or_else(|| "DATE value is out of range for jiff::civil::Date".into()),
            ColumnValues::DateTime2(datetime) => civil_from_datetime2(datetime)
                .map(|value| value.date())
                .ok_or_else(|| "DATETIME2 value is out of range for jiff::civil::Date".into()),
            ColumnValues::DateTimeOffset(datetime) => civil_from_datetime2(&datetime.datetime2)
                .map(|value| value.date())
                .ok_or_else(|| "DATETIMEOFFSET value is out of range for jiff::civil::Date".into()),
            other => Err(format!("cannot decode {other:?} into jiff::civil::Date").into()),
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
        let value = sql_time_from_time(*self)
            .ok_or_else(|| format!("{self} is outside the range SQL Server TIME accepts"))?;

        buf.push(MssqlArgumentValue::new(SqlType::Time(Some(value))));
        Ok(IsNull::No)
    }
}

impl<'r> Decode<'r, Mssql> for Time {
    fn decode(value: MssqlValueRef<'r>) -> Result<Self, BoxDynError> {
        match value.raw() {
            ColumnValues::Time(time) => {
                time_from_nanos(super::ticks_to_nanos(time.time_nanoseconds))
                    .ok_or_else(|| "TIME value is out of range for jiff::civil::Time".into())
            }
            ColumnValues::DateTime2(datetime) => civil_from_datetime2(datetime)
                .map(|value| value.time())
                .ok_or_else(|| "DATETIME2 value is out of range for jiff::civil::Time".into()),
            ColumnValues::DateTimeOffset(datetime) => civil_from_datetime2(&datetime.datetime2)
                .map(|value| value.time())
                .ok_or_else(|| "DATETIMEOFFSET value is out of range for jiff::civil::Time".into()),
            other => Err(format!("cannot decode {other:?} into jiff::civil::Time").into()),
        }
    }
}

impl Type<Mssql> for DateTime {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::datetime2()
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        ty.accepts_datetime_data()
    }
}

impl<'q> Encode<'q, Mssql> for DateTime {
    fn encode_by_ref(&self, buf: &mut Vec<MssqlArgumentValue>) -> Result<IsNull, BoxDynError> {
        let value = datetime2_from_civil(*self)
            .ok_or_else(|| format!("{self} is outside the range SQL Server DATETIME2 accepts"))?;

        buf.push(MssqlArgumentValue::new(SqlType::DateTime2(Some(value))));
        Ok(IsNull::No)
    }
}

impl<'r> Decode<'r, Mssql> for DateTime {
    fn decode(value: MssqlValueRef<'r>) -> Result<Self, BoxDynError> {
        match value.raw() {
            ColumnValues::DateTime2(datetime) => civil_from_datetime2(datetime)
                .ok_or_else(|| "DATETIME2 value is out of range for jiff::civil::DateTime".into()),
            ColumnValues::DateTimeOffset(datetime) => civil_from_datetime2(&datetime.datetime2)
                .ok_or_else(|| {
                    "DATETIMEOFFSET value is out of range for jiff::civil::DateTime".into()
                }),
            other => Err(format!("cannot decode {other:?} into jiff::civil::DateTime").into()),
        }
    }
}

impl Type<Mssql> for Timestamp {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::datetimeoffset()
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        ty.accepts_datetime_data()
    }
}

impl<'q> Encode<'q, Mssql> for Timestamp {
    fn encode_by_ref(&self, buf: &mut Vec<MssqlArgumentValue>) -> Result<IsNull, BoxDynError> {
        // A timestamp is an instant, so it is stored as UTC.
        let value =
            datetime2_from_civil(self.to_zoned(TimeZone::UTC).datetime()).ok_or_else(|| {
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

impl<'r> Decode<'r, Mssql> for Timestamp {
    fn decode(value: MssqlValueRef<'r>) -> Result<Self, BoxDynError> {
        match value.raw() {
            // `mssql-tds` normalises the time part to UTC and keeps the
            // original offset beside it, so the offset must not be applied
            // again to recover the instant.
            ColumnValues::DateTimeOffset(datetime) => timestamp_from_datetime2(&datetime.datetime2)
                .ok_or_else(|| "DATETIMEOFFSET value is out of range for jiff::Timestamp".into()),
            ColumnValues::DateTime2(datetime) => timestamp_from_datetime2(datetime)
                .ok_or_else(|| "DATETIME2 value is out of range for jiff::Timestamp".into()),
            other => Err(format!("cannot decode {other:?} into jiff::Timestamp").into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_numbers_are_anchored_to_the_sql_server_epoch() {
        // `date` counts from 0001-01-01, which is 719_162 days before the unix
        // epoch. `jiff` has no day-number API, so this pins the conversion.
        assert_eq!(days_from_date(Date::new(1, 1, 1).unwrap()), Some(0));
        assert_eq!(
            days_from_date(Date::new(1970, 1, 1).unwrap()),
            Some(719_162)
        );
        assert_eq!(unix_days_from_date(Date::new(1970, 1, 1).unwrap()), 0);
        // SQL Server's own `DATEDIFF(day, '0001-01-01', '2026-09-21')`.
        assert_eq!(
            days_from_date(Date::new(2026, 9, 21).unwrap()),
            Some(739_879)
        );
    }

    #[test]
    fn day_conversions_round_trip() {
        // `3_652_058` is SQL Server's `date` maximum, 9999-12-31.
        for days in [0, 1, 719_162, 1_000_000, 2_932_896, 3_652_058] {
            let date = date_from_days(days)
                .unwrap_or_else(|| panic!("day number {days} was not representable"));

            assert_eq!(days_from_date(date), Some(days), "day number {days}");
        }
    }

    #[test]
    fn time_of_day_keeps_its_hour_and_minute() {
        let time = Time::new(12, 34, 56, 0).unwrap();

        assert_eq!(time_from_nanos(nanos_from_time(time).unwrap()), Some(time));
    }
}
