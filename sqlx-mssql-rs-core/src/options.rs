//! Connection options and URL parsing for the MSSQL driver.

use std::fmt::Debug;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use mssql_tds::connection::client_context::ClientContext;
use mssql_tds::core::{EncryptionOptions, EncryptionSetting};
use sqlx_core::connection::LogSettings;
use sqlx_core::error::Error;
use url::Url;

use crate::MssqlConnection;

/// The port SQL Server listens on by default.
const DEFAULT_PORT: u16 = 1433;

/// How a connection encrypts its traffic.
///
/// These correspond to what the client asks for during the TDS pre-login
/// handshake. This is a driver-owned type so that the `mssql-tds` client
/// settings stay out of the public API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MssqlEncryption {
    /// Do not encrypt, if the server allows an unencrypted connection.
    PreferOff,
    /// Encrypt everything after pre-login.
    #[default]
    On,
    /// Require encryption after pre-login, failing if the server refuses.
    Required,
    /// Encrypt from the very start, including pre-login (TDS 8.0).
    ///
    /// Stricter than most server configurations accept, so it is not the
    /// default.
    Strict,
}

impl MssqlEncryption {
    /// The equivalent `mssql-tds` setting.
    fn to_setting(self) -> EncryptionSetting {
        match self {
            Self::PreferOff => EncryptionSetting::PreferOff,
            Self::On => EncryptionSetting::On,
            Self::Required => EncryptionSetting::Required,
            Self::Strict => EncryptionSetting::Strict,
        }
    }
}

/// Connect options for an MSSQL database.
///
/// Accepts a `mssql://` URL, for example:
///
/// ```text
/// mssql://sa:Password1!@localhost:1433/master?trust_certificate=true
/// ```
///
/// Recognised query parameters are:
///
/// | Parameter | Meaning |
/// |-----------|---------|
/// | `database` | Database name, overriding the URL path |
/// | `encrypt` | `on`, `required`, `strict` or `off` (also `true`/`false`) |
/// | `trust_certificate` | `true` skips server certificate validation |
/// | `server_certificate` | Path to a DER or PEM certificate to pin |
/// | `host_name_in_cert` | CN or SAN expected in the server certificate |
/// | `application_name` | Application name reported to the server |
/// | `connect_timeout` | Connection timeout in seconds |
#[derive(Clone)]
pub struct MssqlConnectOptions {
    host: String,
    port: u16,
    username: String,
    password: String,
    database: String,
    encryption: MssqlEncryption,
    trust_server_certificate: bool,
    server_certificate: Option<PathBuf>,
    host_name_in_cert: Option<String>,
    application_name: Option<String>,
    connect_timeout: Option<Duration>,
    log_settings: LogSettings,
}

impl Debug for MssqlConnectOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MssqlConnectOptions")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            // Never render credentials, even accidentally.
            .field("password", &"<redacted>")
            .field("database", &self.database)
            .field("encryption", &self.encryption)
            .field("trust_server_certificate", &self.trust_server_certificate)
            .field("server_certificate", &self.server_certificate)
            .finish()
    }
}

impl Default for MssqlConnectOptions {
    fn default() -> Self {
        Self {
            host: "localhost".to_owned(),
            port: DEFAULT_PORT,
            username: String::new(),
            password: String::new(),
            database: String::new(),
            // Encrypt after pre-login, matching the behaviour of the Microsoft
            // ODBC driver's `Encrypt=yes`.
            encryption: MssqlEncryption::On,
            trust_server_certificate: false,
            server_certificate: None,
            host_name_in_cert: None,
            application_name: None,
            connect_timeout: None,
            log_settings: LogSettings::default(),
        }
    }
}

impl MssqlConnectOptions {
    /// The TDS datasource string for these options, for example `tcp:host,1433`.
    pub(crate) fn datasource(&self) -> String {
        format!("tcp:{},{}", self.host, self.port)
    }

    /// Builds the `mssql-tds` client context for these options.
    pub(crate) fn client_context(&self) -> ClientContext {
        // `ClientContext` has private fields, so it must be built by assignment
        // rather than struct-update syntax.
        let mut context = ClientContext::default();

        context.user_name = self.username.clone();
        context.password = self.password.clone();
        context.database = self.database.clone();
        context.encryption_options = EncryptionOptions {
            mode: self.encryption.to_setting(),
            trust_server_certificate: self.trust_server_certificate,
            host_name_in_cert: self.host_name_in_cert.clone(),
            server_certificate: self.server_certificate.clone(),
        };

        if let Some(application_name) = &self.application_name {
            context.application_name = application_name.clone();
        }

        if let Some(timeout) = self.connect_timeout {
            context.connect_timeout = u32::try_from(timeout.as_secs()).unwrap_or(u32::MAX);
        }

        context
    }

    /// The database these options connect to.
    pub fn database(&self) -> &str {
        &self.database
    }

    /// The host these options connect to.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The port these options connect to.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Sets the application name reported to the server.
    pub fn application_name(&mut self, name: impl Into<String>) -> &mut Self {
        self.application_name = Some(name.into());
        self
    }

    /// Sets the connection timeout.
    pub fn connect_timeout(&mut self, timeout: Duration) -> &mut Self {
        self.connect_timeout = Some(timeout);
        self
    }

    /// Sets how the connection encrypts its traffic.
    pub fn encryption(&mut self, mode: MssqlEncryption) -> &mut Self {
        self.encryption = mode;
        self
    }

    /// Requests an encrypted connection.
    ///
    /// Shorthand for [`encryption`][Self::encryption], using
    /// [`MssqlEncryption::On`] when `enabled` and
    /// [`MssqlEncryption::PreferOff`] otherwise.
    pub fn encrypt(&mut self, enabled: bool) -> &mut Self {
        self.encryption = if enabled {
            MssqlEncryption::On
        } else {
            MssqlEncryption::PreferOff
        };
        self
    }

    /// Skips server certificate validation.
    ///
    /// This makes the connection vulnerable to man-in-the-middle attacks and
    /// should only be used against a server you trust on a private network.
    pub fn trust_certificate(&mut self, enabled: bool) -> &mut Self {
        self.trust_server_certificate = enabled;
        self
    }

    /// Pins the server's certificate instead of validating its chain.
    ///
    /// The file must hold a DER or PEM encoded X.509 certificate, which is
    /// matched byte for byte against the certificate the server presents. The
    /// normal CA chain check is bypassed, so this is how to keep verification
    /// without depending on a trusted root.
    pub fn server_certificate(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        self.server_certificate = Some(path.into());
        self
    }

    /// Returns the statement log settings.
    pub(crate) fn log_settings(&self) -> &LogSettings {
        &self.log_settings
    }

    /// Replaces the statement log settings, used when adopting options that were
    /// parsed by the `Any` driver.
    #[cfg(feature = "any")]
    pub(crate) fn set_log_settings(&mut self, settings: LogSettings) {
        self.log_settings = settings;
    }

    /// Returns a copy of these options pointing at a different database.
    ///
    /// Migrations use this to connect to `master` to create or drop databases.
    pub fn with_database(&self, database: &str) -> Self {
        let mut options = self.clone();
        options.database = database.to_owned();
        options
    }
}

impl FromStr for MssqlConnectOptions {
    type Err = Error;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let trimmed = input.trim();

        let url = Url::parse(trimmed).map_err(|error| {
            Error::Configuration(
                format!("invalid MSSQL connection URL `{trimmed}`: {error}").into(),
            )
        })?;

        if !url.scheme().eq_ignore_ascii_case("mssql") {
            return Err(Error::Configuration(
                format!(
                    "unsupported URL scheme `{}` for MSSQL; expected `mssql://`",
                    url.scheme()
                )
                .into(),
            ));
        }

        let host = url
            .host_str()
            .ok_or_else(|| {
                Error::Configuration("MSSQL connection URL is missing a host".into())
            })?
            .to_owned();

        let mut options = Self {
            host,
            port: url.port().unwrap_or(DEFAULT_PORT),
            username: percent_decode(url.username()),
            password: percent_decode(url.password().unwrap_or_default()),
            database: percent_decode(url.path().trim_start_matches('/')),
            ..Self::default()
        };

        for (key, value) in url.query_pairs() {
            match key.as_ref() {
                "database" => options.database = value.into_owned(),
                "encrypt" => {
                    options.encryption = parse_encryption(&key, &value)?;
                }
                "trust_certificate" => {
                    options.trust_certificate(parse_bool(&key, &value)?);
                }
                "server_certificate" => {
                    options.server_certificate = Some(PathBuf::from(value.into_owned()));
                }
                "host_name_in_cert" => options.host_name_in_cert = Some(value.into_owned()),
                "application_name" => options.application_name = Some(value.into_owned()),
                "connect_timeout" => {
                    let seconds: u64 = value.parse().map_err(|_| {
                        Error::Configuration(
                            format!("`connect_timeout` must be a whole number of seconds, got `{value}`")
                                .into(),
                        )
                    })?;
                    options.connect_timeout = Some(Duration::from_secs(seconds));
                }
                // Unknown parameters are ignored so that a URL can carry
                // settings meant for other drivers.
                _ => {}
            }
        }

        Ok(options)
    }
}

fn parse_bool(key: &str, value: &str) -> Result<bool, Error> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "yes" | "1" => Ok(true),
        "false" | "no" | "0" => Ok(false),
        other => Err(Error::Configuration(
            format!("`{key}` must be a boolean, got `{other}`").into(),
        )),
    }
}

/// Parses the `encrypt` query parameter.
///
/// The boolean spellings are still accepted, so URLs written before the
/// stricter modes existed keep working.
fn parse_encryption(key: &str, value: &str) -> Result<MssqlEncryption, Error> {
    match value.to_ascii_lowercase().as_str() {
        "on" => return Ok(MssqlEncryption::On),
        "required" => return Ok(MssqlEncryption::Required),
        "strict" => return Ok(MssqlEncryption::Strict),
        "off" | "prefer-off" | "prefer_off" => return Ok(MssqlEncryption::PreferOff),
        _ => {}
    }

    if let Ok(enabled) = parse_bool(key, value) {
        return Ok(if enabled {
            MssqlEncryption::On
        } else {
            MssqlEncryption::PreferOff
        });
    }

    Err(Error::Configuration(
        format!("`{key}` must be one of `on`, `required`, `strict` or `off`, got `{value}`")
            .into(),
    ))
}

/// Decodes `%XX` escapes in a URL component.
///
/// The `url` crate keeps credentials and paths percent-encoded, but the
/// credentials used for SQL Server logins are almost always literal.
fn percent_decode(input: &str) -> String {
    if !input.contains('%') {
        return input.to_owned();
    }

    let bytes = input.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok();
            if let Some(byte) = hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                output.push(byte);
                index += 3;
                continue;
            }
        }

        output.push(bytes[index]);
        index += 1;
    }

    String::from_utf8_lossy(&output).into_owned()
}

impl sqlx_core::connection::ConnectOptions for MssqlConnectOptions {
    type Connection = MssqlConnection;

    fn from_url(url: &Url) -> Result<Self, Error> {
        Self::from_str(url.as_str())
    }

    async fn connect(&self) -> Result<Self::Connection, Error> {
        MssqlConnection::connect_with(self).await
    }

    fn log_statements(mut self, level: log::LevelFilter) -> Self {
        self.log_settings.log_statements(level);
        self
    }

    fn log_slow_statements(mut self, level: log::LevelFilter, duration: Duration) -> Self {
        self.log_settings.log_slow_statements(level, duration);
        self
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{MssqlConnectOptions, MssqlEncryption};

    #[test]
    fn parses_a_full_url() {
        let options: MssqlConnectOptions = "mssql://sa:Password1!@db.example:1434/testdb"
            .parse()
            .expect("URL should parse");

        assert_eq!(options.host(), "db.example");
        assert_eq!(options.port(), 1434);
        assert_eq!(options.database(), "testdb");
    }

    #[test]
    fn defaults_to_the_standard_port_and_database_from_path() {
        let options: MssqlConnectOptions = "mssql://sa:secret@localhost/master"
            .parse()
            .expect("URL should parse");

        assert_eq!(options.port(), 1433);
        assert_eq!(options.database(), "master");
    }

    #[test]
    fn applies_trust_certificate_parameter() {
        let options: MssqlConnectOptions =
            "mssql://sa:secret@localhost/master?trust_certificate=true"
                .parse()
                .expect("URL should parse");

        assert!(options.trust_server_certificate);
    }

    #[test]
    fn rejects_non_mssql_scheme() {
        let error = "postgres://localhost/db"
            .parse::<MssqlConnectOptions>()
            .expect_err("wrong scheme should be rejected");

        assert!(error.to_string().contains("unsupported URL scheme"));
    }

    #[test]
    fn decodes_percent_escapes_in_credentials() {
        let options: MssqlConnectOptions = "mssql://sa:p%40ss%2Fword@localhost/master"
            .parse()
            .expect("URL should parse");

        assert_eq!(options.password, "p@ss/word");
    }

    #[test]
    fn debug_does_not_leak_the_password() {
        let options: MssqlConnectOptions = "mssql://sa:supersecret@localhost/master"
            .parse()
            .expect("URL should parse");

        let rendered = format!("{options:?}");
        assert!(!rendered.contains("supersecret"));
        assert!(rendered.contains("<redacted>"));
    }

    #[test]
    fn query_parameter_overrides_path_database() {
        let options: MssqlConnectOptions = "mssql://sa:secret@localhost/master?database=other"
            .parse()
            .expect("URL should parse");

        assert_eq!(options.database(), "other");
    }

    #[test]
    fn defaults_to_encrypting_after_pre_login() {
        let options: MssqlConnectOptions = "mssql://sa:secret@localhost/master"
            .parse()
            .expect("URL should parse");

        assert_eq!(options.encryption, MssqlEncryption::On);
    }

    #[test]
    fn parses_every_encryption_mode() {
        for (value, expected) in [
            ("on", MssqlEncryption::On),
            ("required", MssqlEncryption::Required),
            ("strict", MssqlEncryption::Strict),
            ("off", MssqlEncryption::PreferOff),
            // Boolean spellings predate the modes and must keep working.
            ("true", MssqlEncryption::On),
            ("false", MssqlEncryption::PreferOff),
        ] {
            let url = format!("mssql://sa:secret@localhost/master?encrypt={value}");
            let options: MssqlConnectOptions = url.parse().expect("URL should parse");

            assert_eq!(options.encryption, expected, "encrypt={value}");
        }
    }

    #[test]
    fn rejects_an_unknown_encryption_mode() {
        let error = "mssql://sa:secret@localhost/master?encrypt=sometimes"
            .parse::<MssqlConnectOptions>()
            .expect_err("an unknown mode should be rejected");

        assert!(error.to_string().contains("`encrypt` must be one of"));
    }

    #[test]
    fn parses_the_server_certificate_parameter() {
        let options: MssqlConnectOptions =
            "mssql://sa:secret@localhost/master?server_certificate=%2Fetc%2Fssl%2Fserver.pem"
                .parse()
                .expect("URL should parse");

        assert_eq!(
            options.server_certificate.as_deref(),
            Some(Path::new("/etc/ssl/server.pem"))
        );
    }

    #[test]
    fn forwards_the_tls_settings_to_the_client_context() {
        use mssql_tds::core::EncryptionSetting;

        let options: MssqlConnectOptions = "mssql://sa:secret@localhost/master\
             ?encrypt=strict&server_certificate=/etc/ssl/server.pem&host_name_in_cert=db.example"
            .parse()
            .expect("URL should parse");

        let encryption = options.client_context().encryption_options;

        assert!(matches!(encryption.mode, EncryptionSetting::Strict));
        assert_eq!(
            encryption.server_certificate.as_deref(),
            Some(Path::new("/etc/ssl/server.pem"))
        );
        assert_eq!(encryption.host_name_in_cert.as_deref(), Some("db.example"));
    }
}
