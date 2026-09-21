//! SQL Server type information.

use mssql_tds::datatypes::sqldatatypes::TdsDataType;
use mssql_tds::query::metadata::ColumnMetadata;

/// Type information for an MSSQL column.
///
/// This is a driver-owned description of a column's SQL Server type rather than
/// a borrowed TDS descriptor, so it can be cached, compared and serialized for
/// offline query metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "offline", derive(serde::Serialize, serde::Deserialize))]
pub struct MssqlTypeInfo {
    type_name: String,
    sql_data_type: u8,
    length: u32,
    precision: Option<u8>,
    scale: Option<u8>,
}

impl MssqlTypeInfo {
    /// Creates type information from its parts.
    pub fn new(
        type_name: impl Into<String>,
        sql_data_type: u8,
        length: u32,
        precision: Option<u8>,
        scale: Option<u8>,
    ) -> Self {
        Self {
            type_name: type_name.into(),
            sql_data_type,
            length,
            precision,
            scale,
        }
    }

    /// Builds type information from a TDS column metadata entry.
    pub(crate) fn from_column_metadata(column: &ColumnMetadata) -> Self {
        let data_type = column.data_type;

        Self {
            type_name: data_type_name(data_type),
            sql_data_type: data_type as u8,
            length: u32::try_from(column.type_info.length).unwrap_or(u32::MAX),
            precision: column.get_precision(),
            scale: column.get_scale(),
        }
    }

    /// Canonical SQL Server type name, for example `int` or `nvarchar`.
    pub fn type_name(&self) -> &str {
        &self.type_name
    }

    /// Byte length reported in the column metadata.
    pub const fn length(&self) -> u32 {
        self.length
    }

    /// Decimal precision, when the underlying type carries one.
    pub const fn precision(&self) -> Option<u8> {
        self.precision
    }

    /// Decimal scale, when the underlying type carries one.
    pub const fn scale(&self) -> Option<u8> {
        self.scale
    }

    /// The parsed TDS type, when the identifier is one this driver knows.
    pub fn data_type(&self) -> Option<TdsDataType> {
        TdsDataType::try_from(self.sql_data_type).ok()
    }

    /// Whether values of this type are character data.
    pub fn accepts_character_data(&self) -> bool {
        matches!(
            self.data_type(),
            Some(
                TdsDataType::BigChar
                    | TdsDataType::BigVarChar
                    | TdsDataType::Char
                    | TdsDataType::VarChar
                    | TdsDataType::Text
                    | TdsDataType::NChar
                    | TdsDataType::NVarChar
                    | TdsDataType::NText
                    | TdsDataType::Xml
                    | TdsDataType::Json
            )
        )
    }

    /// Whether values of this type are binary data.
    pub fn accepts_binary_data(&self) -> bool {
        matches!(
            self.data_type(),
            Some(
                TdsDataType::BigBinary
                    | TdsDataType::BigVarBinary
                    | TdsDataType::Binary
                    | TdsDataType::VarBinary
                    | TdsDataType::Image
                    | TdsDataType::Udt
                    | TdsDataType::Vector
            )
        )
    }

    /// Whether values of this type are numeric.
    pub fn accepts_numeric_data(&self) -> bool {
        matches!(
            self.data_type(),
            Some(
                TdsDataType::Int1
                    | TdsDataType::Int2
                    | TdsDataType::Int4
                    | TdsDataType::Int8
                    | TdsDataType::IntN
                    | TdsDataType::Decimal
                    | TdsDataType::DecimalN
                    | TdsDataType::Numeric
                    | TdsDataType::NumericN
                    | TdsDataType::Money
                    | TdsDataType::Money4
                    | TdsDataType::MoneyN
                    | TdsDataType::Flt4
                    | TdsDataType::Flt8
                    | TdsDataType::FltN
            )
        )
    }

    /// Whether values of this type are date/time values.
    pub fn accepts_datetime_data(&self) -> bool {
        matches!(
            self.data_type(),
            Some(
                TdsDataType::DateN
                    | TdsDataType::TimeN
                    | TdsDataType::DateTime2N
                    | TdsDataType::DateTimeOffsetN
                    | TdsDataType::DateTime
                    | TdsDataType::DateTim4
                    | TdsDataType::DateTimeN
            )
        )
    }

    /// Whether values of this type are boolean.
    pub fn accepts_boolean_data(&self) -> bool {
        matches!(
            self.data_type(),
            Some(TdsDataType::Bit | TdsDataType::BitN)
        )
    }

    /// Type information for a type this driver cannot name.
    pub fn unknown() -> Self {
        Self::new("unknown", TdsDataType::None as u8, 0, None, None)
    }

    /// Type information for `tinyint`.
    pub fn tinyint() -> Self {
        Self::new("tinyint", TdsDataType::Int1 as u8, 1, None, None)
    }

    /// Type information for `smallint`.
    pub fn smallint() -> Self {
        Self::new("smallint", TdsDataType::Int2 as u8, 2, None, None)
    }

    /// Type information for `int`.
    pub fn integer() -> Self {
        Self::new("int", TdsDataType::Int4 as u8, 4, None, None)
    }

    /// Type information for `bigint`.
    pub fn bigint() -> Self {
        Self::new("bigint", TdsDataType::Int8 as u8, 8, None, None)
    }

    /// Type information for `real`.
    pub fn real() -> Self {
        Self::new("real", TdsDataType::Flt4 as u8, 4, None, None)
    }

    /// Type information for `float`.
    pub fn double() -> Self {
        Self::new("float", TdsDataType::Flt8 as u8, 8, None, None)
    }

    /// Type information for `bit`.
    pub fn bit() -> Self {
        Self::new("bit", TdsDataType::Bit as u8, 1, None, None)
    }

    /// Type information for `nvarchar(max)`.
    pub fn nvarchar_max() -> Self {
        Self::new("nvarchar", TdsDataType::NVarChar as u8, 0, None, None)
    }

    /// Type information for `varbinary(max)`.
    pub fn varbinary_max() -> Self {
        Self::new("varbinary", TdsDataType::BigVarBinary as u8, 0, None, None)
    }

    /// Type information for `uniqueidentifier`.
    pub fn uuid() -> Self {
        Self::new("uniqueidentifier", TdsDataType::Guid as u8, 16, None, None)
    }

    /// Type information for `decimal`.
    pub fn decimal() -> Self {
        Self::new("decimal", TdsDataType::DecimalN as u8, 17, None, None)
    }

    /// Type information for `date`.
    pub fn date() -> Self {
        Self::new("date", TdsDataType::DateN as u8, 3, None, None)
    }

    /// Type information for `time`.
    pub fn time() -> Self {
        Self::new("time", TdsDataType::TimeN as u8, 5, None, None)
    }

    /// Type information for `datetime2`.
    pub fn datetime2() -> Self {
        Self::new("datetime2", TdsDataType::DateTime2N as u8, 8, None, None)
    }

    /// Type information for `datetimeoffset`.
    pub fn datetimeoffset() -> Self {
        Self::new("datetimeoffset", TdsDataType::DateTimeOffsetN as u8, 10, None, None)
    }

    /// Type information for `json`.
    pub fn json() -> Self {
        Self::new("json", TdsDataType::Json as u8, 0, None, None)
    }

    /// Whether sqlx resolves this type to exactly one Rust type.
    ///
    /// The query macros turn each described parameter into a compile-time check
    /// that the bound argument is the expected Rust type. `param_type_for_id`
    /// finds that type by looking for an exact `Type::type_info()` match first
    /// and then for the first `Type::compatible()` match among the types listed
    /// in `impl_type_checking!`.
    ///
    /// Several of this driver's `compatible` predicates are deliberately
    /// permissive so a column can be decoded into whichever Rust type fits, and
    /// a *first* match is a poor choice for a parameter: `i8` accepts any
    /// numeric type, so it would claim `decimal(18,2)` and demand that a bound
    /// argument be an `i8`. Reporting a parameter type therefore has to be
    /// limited to the ones that resolve predictably; the caller falls back to
    /// reporting a parameter count for the rest.
    pub(crate) fn has_exact_rust_mapping(&self) -> bool {
        // Character data is claimed by `String`/`&str` and binary data by
        // `Vec<u8>`/`&[u8]`; nothing earlier in the list accepts either kind, so
        // the first match is the intended one whatever the length is.
        if self.accepts_character_data() || self.accepts_binary_data() {
            return true;
        }

        // Date/time types have no exact match and are resolved by the
        // feature-gated `datetime-types` section, so they can only be reported
        // when that section was compiled in.
        if self.accepts_datetime_data() {
            return cfg!(any(feature = "chrono", feature = "time"));
        }

        match self.data_type() {
            // A GUID needs the `uuid` feature for a Rust type to exist at all.
            Some(TdsDataType::Guid) => cfg!(feature = "uuid"),

            // The rest have to match a registered type's `type_info()` exactly,
            // which means both the TDS identifier and the reported length have
            // to agree with the constructor of the Rust type that owns them.
            Some(
                TdsDataType::Int1
                    | TdsDataType::Int2
                    | TdsDataType::Int4
                    | TdsDataType::Int8
                    | TdsDataType::Flt4
                    | TdsDataType::Flt8
                    | TdsDataType::Bit,
            ) => true,

            _ => false,
        }
    }
}

/// Parses the type name SQL Server reports for an undeclared parameter.
///
/// `sp_describe_undeclared_parameters` suggests names such as `int`,
/// `nvarchar(50)` or `decimal(18,2)`. Returns `None` for a type this driver does
/// not model, so the caller can fall back to reporting a parameter count.
///
/// The numeric types are normalized to the same values their constructors
/// produce, because [`MssqlTypeInfo`] compares every field and an exact match is
/// what makes the Rust mapping unambiguous.
pub(crate) fn from_system_type_name(system_type_name: &str) -> Option<MssqlTypeInfo> {
    let trimmed = system_type_name.trim();

    let (base, args) = match trimmed.find('(') {
        Some(open) => {
            let close = trimmed.rfind(')')?;
            (&trimmed[..open], Some(trimmed[open + 1..close].trim()))
        }
        None => (trimmed, None),
    };

    let base = base.trim().to_ascii_lowercase();

    let first_number = args.and_then(|args| parse_argument(args, 0));
    let second_number = args.and_then(|args| parse_argument(args, 1));

    // `max` is a length of its own rather than a number, and SQL Server reports
    // byte lengths, so the Unicode types are two bytes per character.
    let is_max = args.is_some_and(|args| args.eq_ignore_ascii_case("max"));
    let length_of = |bytes_per_char: u32| -> u32 {
        if is_max {
            u32::from(u16::MAX)
        } else {
            first_number
                .map(u32::from)
                .unwrap_or_default()
                .saturating_mul(bytes_per_char)
        }
    };

    let info = match base.as_str() {
        "tinyint" => MssqlTypeInfo::tinyint(),
        "smallint" => MssqlTypeInfo::smallint(),
        "int" => MssqlTypeInfo::integer(),
        "bigint" => MssqlTypeInfo::bigint(),
        "bit" => MssqlTypeInfo::bit(),
        "real" => MssqlTypeInfo::real(),
        "float" => MssqlTypeInfo::double(),
        "uniqueidentifier" => MssqlTypeInfo::uuid(),
        "date" => MssqlTypeInfo::date(),
        "time" => MssqlTypeInfo::time(),
        "datetime2" => MssqlTypeInfo::datetime2(),
        "datetimeoffset" => MssqlTypeInfo::datetimeoffset(),
        "datetime" => {
            MssqlTypeInfo::new("datetime", TdsDataType::DateTime as u8, 8, None, None)
        }
        "smalldatetime" => {
            MssqlTypeInfo::new("smalldatetime", TdsDataType::DateTim4 as u8, 4, None, None)
        }
        "money" => MssqlTypeInfo::new("money", TdsDataType::Money as u8, 8, None, None),
        "smallmoney" => {
            MssqlTypeInfo::new("smallmoney", TdsDataType::Money4 as u8, 4, None, None)
        }
        "decimal" | "numeric" => MssqlTypeInfo::new(
            base,
            TdsDataType::DecimalN as u8,
            17,
            first_number,
            second_number,
        ),
        "char" => MssqlTypeInfo::new(
            "char",
            TdsDataType::BigChar as u8,
            length_of(1),
            None,
            None,
        ),
        "varchar" => MssqlTypeInfo::new(
            "varchar",
            TdsDataType::BigVarChar as u8,
            length_of(1),
            None,
            None,
        ),
        "nchar" => MssqlTypeInfo::new(
            "nchar",
            TdsDataType::NChar as u8,
            length_of(2),
            None,
            None,
        ),
        "nvarchar" => MssqlTypeInfo::new(
            "nvarchar",
            TdsDataType::NVarChar as u8,
            length_of(2),
            None,
            None,
        ),
        "binary" => MssqlTypeInfo::new(
            "binary",
            TdsDataType::BigBinary as u8,
            length_of(1),
            None,
            None,
        ),
        "varbinary" => MssqlTypeInfo::new(
            "varbinary",
            TdsDataType::BigVarBinary as u8,
            length_of(1),
            None,
            None,
        ),
        "text" => MssqlTypeInfo::new("text", TdsDataType::Text as u8, 65535, None, None),
        "ntext" => MssqlTypeInfo::new("ntext", TdsDataType::NText as u8, 65535, None, None),
        "image" => MssqlTypeInfo::new("image", TdsDataType::Image as u8, 65535, None, None),
        "xml" => MssqlTypeInfo::new("xml", TdsDataType::Xml as u8, 65535, None, None),
        "json" => MssqlTypeInfo::new("json", TdsDataType::Json as u8, 65535, None, None),
        _ => return None,
    };

    Some(info)
}

/// Reads the `index`th comma-separated argument of a type name's parentheses.
fn parse_argument(args: &str, index: usize) -> Option<u8> {
    args.split(',')
        .nth(index)?
        .trim()
        .parse()
        .ok()
}

/// Best-effort SQL Server name for a TDS type.
///
/// `get_meta_type_name` deliberately rejects the wire-only variants that never
/// appear in column metadata, so those fall back to a lowercased debug name
/// rather than panicking.
fn data_type_name(data_type: TdsDataType) -> String {
    data_type
        .get_meta_type_name()
        .map(str::to_owned)
        .unwrap_or_else(|_| format!("{data_type:?}").to_lowercase())
}

impl sqlx_core::type_info::TypeInfo for MssqlTypeInfo {
    fn is_null(&self) -> bool {
        // No type decoded from SQL Server is itself the NULL type: a NULL is
        // carried by the value, not by its type.
        false
    }

    fn name(&self) -> &str {
        &self.type_name
    }
}

impl std::fmt::Display for MssqlTypeInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.type_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_bare_numeric_names_to_their_constructors() {
        // Exact equality with the constructor is what `param_type_for_id`
        // relies on to pick a Rust type without guessing.
        assert_eq!(from_system_type_name("int"), Some(MssqlTypeInfo::integer()));
        assert_eq!(from_system_type_name("bigint"), Some(MssqlTypeInfo::bigint()));
        assert_eq!(
            from_system_type_name("smallint"),
            Some(MssqlTypeInfo::smallint())
        );
        assert_eq!(
            from_system_type_name("tinyint"),
            Some(MssqlTypeInfo::tinyint())
        );
        assert_eq!(from_system_type_name("bit"), Some(MssqlTypeInfo::bit()));
        assert_eq!(from_system_type_name("real"), Some(MssqlTypeInfo::real()));
        assert_eq!(from_system_type_name("float"), Some(MssqlTypeInfo::double()));
        assert_eq!(
            from_system_type_name("uniqueidentifier"),
            Some(MssqlTypeInfo::uuid())
        );
    }

    #[test]
    fn reports_declared_lengths_in_bytes() {
        // Unicode types are two bytes per character, and `max` is reported as
        // the maximum length rather than as zero.
        assert_eq!(from_system_type_name("varchar(10)").unwrap().length(), 10);
        assert_eq!(from_system_type_name("nvarchar(50)").unwrap().length(), 100);
        assert_eq!(from_system_type_name("varbinary(16)").unwrap().length(), 16);
        assert_eq!(
            from_system_type_name("nvarchar(max)").unwrap().length(),
            u32::from(u16::MAX)
        );
    }

    #[test]
    fn keeps_decimal_precision_and_scale() {
        let info = from_system_type_name("decimal(18,2)").unwrap();
        assert_eq!(info.type_name(), "decimal");
        assert_eq!(info.precision(), Some(18));
        assert_eq!(info.scale(), Some(2));
    }

    #[test]
    fn rejects_types_it_does_not_model() {
        assert_eq!(from_system_type_name("sql_variant"), None);
        assert_eq!(from_system_type_name("hierarchyid"), None);
    }

    #[test]
    fn only_types_with_a_predictable_rust_mapping_are_reportable() {
        // `String`/`Vec<u8>` are the only claimants of these kinds.
        assert!(from_system_type_name("nvarchar(50)").unwrap().has_exact_rust_mapping());
        assert!(from_system_type_name("varchar(max)").unwrap().has_exact_rust_mapping());
        assert!(from_system_type_name("varbinary(max)").unwrap().has_exact_rust_mapping());
        assert!(MssqlTypeInfo::integer().has_exact_rust_mapping());
        assert!(MssqlTypeInfo::bit().has_exact_rust_mapping());

        // `i8` accepts any numeric type, so decimal and money would otherwise be
        // reported as `i8` and reject correctly bound arguments.
        assert!(!from_system_type_name("decimal(18,2)").unwrap().has_exact_rust_mapping());
        assert!(!from_system_type_name("money").unwrap().has_exact_rust_mapping());
    }

    #[test]
    fn feature_gated_types_are_only_reportable_when_their_feature_is_on() {
        // Reporting a type sqlx cannot map would turn a previously working
        // query into a compile error, so these follow the enabled features.
        assert_eq!(
            MssqlTypeInfo::uuid().has_exact_rust_mapping(),
            cfg!(feature = "uuid")
        );
        assert_eq!(
            MssqlTypeInfo::datetime2().has_exact_rust_mapping(),
            cfg!(any(feature = "chrono", feature = "time"))
        );
    }
}
