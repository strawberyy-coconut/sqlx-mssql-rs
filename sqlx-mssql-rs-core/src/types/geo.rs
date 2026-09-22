//! `geo-types` geometry support.
//!
//! SQL Server's `geometry` and `geography` types are UDTs. Their bytes are moved
//! as opaque binary and interpreted as WKB, which is the interchange format the
//! server uses for these values.

use geo_traits::to_geo::ToGeoGeometry;
use geo_types::Geometry;
use sqlx_core::decode::Decode;
use sqlx_core::encode::{Encode, IsNull};
use sqlx_core::error::BoxDynError;
use sqlx_core::types::Type;

use crate::{Mssql, MssqlArgumentValue, MssqlTypeInfo, MssqlValueRef};

impl Type<Mssql> for Geometry<f64> {
    fn type_info() -> MssqlTypeInfo {
        MssqlTypeInfo::varbinary_max()
    }

    fn compatible(ty: &MssqlTypeInfo) -> bool {
        ty.accepts_binary_data()
    }
}

impl<'q> Encode<'q, Mssql> for Geometry<f64> {
    fn encode_by_ref(&self, buf: &mut Vec<MssqlArgumentValue>) -> Result<IsNull, BoxDynError> {
        let mut bytes = Vec::new();
        wkb::writer::write_geometry(&mut bytes, self, &wkb::writer::WriteOptions::default())?;

        buf.push(MssqlArgumentValue::bytes(bytes));
        Ok(IsNull::No)
    }
}

impl<'r> Decode<'r, Mssql> for Geometry<f64> {
    fn decode(value: MssqlValueRef<'r>) -> Result<Self, BoxDynError> {
        let Some(bytes) = value.as_bytes() else {
            return Err("cannot decode value into a geometry: expected binary".into());
        };

        Ok(wkb::reader::read_wkb(bytes)?.to_geometry())
    }
}
