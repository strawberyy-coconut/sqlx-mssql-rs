use crate::Mssql;
#[allow(unused_imports)]
use sqlx_core as sqlx;
sqlx_core::impl_type_checking!(
    Mssql {
        bool,
        i8,
        i16,
        i32,
        i64,
        f32,
        f64,
        String | &str,
        Vec<u8> | &[u8],

        #[cfg(feature = "spatial")]
        sqlx_mssql_rs::MssqlGeometry,
        #[cfg(feature = "spatial")]
        sqlx_mssql_rs::MssqlGeography,

        #[cfg(feature = "spatial")]
        geo_types::Geometry<f64>,

        #[cfg(feature = "uuid")]
        sqlx::types::Uuid,

        // `sqlx-macros-core` has no `jiff` section, so these live in the main
        // list. They match date/time types exactly, which means an enabled
        // `jiff` takes precedence over `chrono` and `time` for inference.
        #[cfg(feature = "jiff")]
        jiff::civil::Date,
        #[cfg(feature = "jiff")]
        jiff::civil::Time,
        #[cfg(feature = "jiff")]
        jiff::civil::DateTime,
        #[cfg(feature = "jiff")]
        jiff::Timestamp,
    },
    ParamChecking::Weak,
    // The TDS layer decodes any column into a general representation that the
    // driver can then narrow, and parameters are declared from the encoded
    // value, so no type requires a feature gate.
    feature-types: _info => None,
    datetime-types: {
        chrono: {
            sqlx::types::chrono::NaiveDate,
            sqlx::types::chrono::NaiveTime,
            sqlx::types::chrono::NaiveDateTime,
            sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>
                | sqlx::types::chrono::DateTime<_>,
        },

        time: {
            sqlx::types::time::Date,
            sqlx::types::time::Time,
            sqlx::types::time::PrimitiveDateTime,
            sqlx::types::time::OffsetDateTime,
        },
    },
    numeric-types: {
        bigdecimal: {
            sqlx::types::BigDecimal,
        },
        rust_decimal: {
            sqlx::types::Decimal,
        },
    },
);
