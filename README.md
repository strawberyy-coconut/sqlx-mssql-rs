# sqlx-mssql-rs

Microsoft SQL Server driver for SQLx, built on Microsoft's `mssql-tds` TDS
implementation.

This crate connects SQLx to Microsoft SQL Server and Azure SQL by speaking the
TDS protocol directly. There is no ODBC driver manager, no native client library
to install, and no background thread bridging to a blocking API: the underlying
client is fully asynchronous, so the driver is Tokio-only and adds nothing but
crates.io dependencies.

> **Status:** not published to crates.io yet. Depend on it via `path` or `git`.

## Minimal Query

```toml
[dependencies]
sqlx-mssql-rs = { path = "../sqlx-mssql-rs" }
sqlx-core = { version = "0.9.0", default-features = false }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

```rust
use sqlx_core::connection::Connection;
use sqlx_core::row::Row;
use sqlx_mssql_rs::MssqlConnection;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut conn = MssqlConnection::connect(
        "mssql://sa:MyPass@localhost:1433/testdb?trust_certificate=true",
    ).await?;

    let row = sqlx_core::query::query("SELECT 1")
        .fetch_one(&mut conn)
        .await?;

    let value: i32 = row.try_get(0)?;
    println!("{value}");

    conn.close().await?;
    Ok(())
}
```

`MssqlConnection::connect()` accepts a standard `mssql://` URL:

```text
mssql://user:password@host:port/database?params
```

Port 1433 is assumed when omitted.

### URL query parameters

| Parameter | Description |
|---|---|
| `encrypt=true` | Encrypt the connection (already the default) |
| `trust_certificate=true` | Skip server certificate validation |
| `host_name_in_cert=...` | CN or SAN expected in the server certificate |
| `application_name=...` | Application name reported to the server |
| `connect_timeout=15` | Connection timeout, in seconds |
| `database=...` | Database name, overriding the URL path |

### Builder methods

```rust
use std::str::FromStr;
use std::time::Duration;
use sqlx_core::connection::ConnectOptions;
use sqlx_mssql_rs::MssqlConnectOptions;

let mut options = MssqlConnectOptions::from_str("mssql://localhost/testdb")?;
options.encrypt(true)
       .trust_certificate(true)
       .application_name("my-app")
       .connect_timeout(Duration::from_secs(15));
let conn = options.connect().await?;
```

`with_database()` returns a copy of the options pointing at a different
database; migrations use it to reach `master` when creating or dropping a
database.

## Encryption and Authentication

TLS is negotiated inside the TDS handshake, so there is no system TLS
configuration to manage and no certificate files to install. Connections request
encryption by default, matching the Microsoft ODBC driver's `Encrypt=yes`.
Setting `encrypt=false` prefers an unencrypted connection instead.

SQL logins work out of the box. Enable the `integrated-auth` feature for
Kerberos or NTLM authentication; on Unix the GSSAPI library is loaded at
runtime, so there is no build-time Kerberos dependency.

## Connection Pooling

```rust
use sqlx_mssql_rs::MssqlPoolOptions;

let pool = MssqlPoolOptions::new()
    .max_connections(10)
    .connect("mssql://sa:MyPass@localhost:1433/testdb?trust_certificate=true")
    .await?;

let row = sqlx_core::query::query("SELECT 1")
    .fetch_one(&pool)
    .await?;
```

`MssqlPool` and `MssqlPoolOptions` are aliases for SQLx's generic `Pool` and
`PoolOptions` specialised to `Mssql`.

## Compile-time Checked Queries

The `query!()`, `query_as!()` and `query_scalar!()` macros verify your SQL
against a real database while compiling, using `DATABASE_URL` or an offline
cache in `.sqlx` (see `prepare` below).

### Parameter checking

The number of `?` markers is always checked against the number of arguments. The
driver also reports the SQL type of every parameter, so each bound value is
checked against the type the server inferred:

```rust
// `users.id` is `uniqueidentifier`, so this is a compile error:
//   expected `Uuid`, found `&str`
sqlx_mssql_rs::query!("SELECT description FROM users WHERE id = ?", "not-a-uuid");
```

Types that do not resolve to exactly one Rust type are reported as a count
instead, so only the argument count is checked for those. That covers the exact
numeric types (`decimal`, `money`) and the feature-gated types (`uuid`,
`chrono`, `time`) when their feature is not enabled.

An explicit cast skips the check for that argument, which is the escape hatch
when a valid binding is not the type the server inferred:

```rust
sqlx_mssql_rs::query!("SELECT id FROM users WHERE amount > ?", amount as f64);
```

The generated query-builder and type-checking types are deeply nested, so a
crate that uses the query builders or these macros may need to raise the
compiler's limit:

```rust
#![recursion_limit = "512"]
```

If the compiler reports "queries overflow the depth limit", this is the fix.
## Features

| Feature | Default | Description |
|---|---|---|
| `macros` | yes | `query!()`, `query_as!()`, `query_scalar!()` and friends |
| `derive` | yes | `Encode`, `Decode`, `Type`, `FromRow` derive macros |
| `migrate` | yes | Migration support |
| `offline` | yes | Compile-time checking against a `.sqlx` cache |
| `bigdecimal` | no | `BigDecimal` type support |
| `chrono` | no | `chrono` datetime types |
| `time` | no | `time` datetime types |
| `decimal` / `rust_decimal` | no | `Decimal` type support |
| `json` | no | `serde_json::Value` support |
| `uuid` | no | `uuid::Uuid` support |
| `spatial` | no | `geo-types` geometry support |
| `any` | no | `AnyConnection` support (used by the CLI) |
| `integrated-auth` | no | Kerberos / NTLM authentication |

There are no `runtime-*` or `tls-*` features: the driver is Tokio-only and TLS
lives inside the TDS handshake.

## CLI (`sqlx-mssql`)

A thin wrapper around `sqlx-cli` for managing MSSQL databases, running
migrations, and preparing offline query data.

### Install

```bash
cargo install --path sqlx-mssql-rs-cli --locked
```

This installs two binaries:

- `sqlx-mssql` — a standalone command
- `cargo-sqlx-mssql` — the same tool as a cargo subcommand

After installation, both are available on your `PATH`.

### Usage

All standard `sqlx-cli` subcommands are supported. Provide your database URL
via `--database-url` or the `DATABASE_URL` environment variable (or a `.env`
file; pass `--no-dotenv` to disable that).

```bash
# Create / drop the database
sqlx-mssql database create
sqlx-mssql database drop -y

# Create and run migrations
sqlx-mssql migrate add <name>
sqlx-mssql migrate run

# Revert the last migration
sqlx-mssql migrate revert

# List migration status
sqlx-mssql migrate info

# Prepare offline query data (for compile-time checked queries)
sqlx-mssql prepare
```

The same commands work through cargo:

```bash
cargo sqlx-mssql migrate run
```

`database drop` and `database reset` prompt for confirmation; pass `-y` to skip
the prompt in scripts.

**Environment variable** (add to `.env` in your project root):

```
DATABASE_URL=mssql://sa:Password1!@127.0.0.1:1433/my_database?trust_certificate=true
```

Or use the `--database-url` flag:

```bash
sqlx-mssql migrate run --database-url mssql://sa:Password1!@127.0.0.1:1433/my_database
```

### Run without installing

```bash
cargo run -p sqlx-mssql-rs-cli -- migrate run
```

## Example

A commented walk-through lives in `examples/full`:

```bash
cargo run -p full
```

It connects, runs migrations, binds parameters, reads rows back, commits,
rolls back, and exercises each of the query macros. It reads
`MSSQL_DATABASE_URL` first and falls back to `DATABASE_URL`.

## Running Tests

```bash
docker run -e "ACCEPT_EULA=Y" -e "MSSQL_SA_PASSWORD=MyPass" \
  -p 1433:1433 -d mcr.microsoft.com/mssql/server:2022-latest

DATABASE_URL="mssql://sa:MyPass@localhost:1433/master?trust_certificate=true" \
  cargo test --workspace
```

The integration tests create tables under unique names and drop them again, so
the suite is safe to run against a shared server. If `DATABASE_URL` is unset it
defaults to `mssql://sa:Password1!@localhost:1433/master?trust_certificate=true`.

Some tests are gated behind type features; run those with the feature enabled:

```bash
DATABASE_URL="mssql://sa:MyPass@localhost:1433/master?trust_certificate=true" \
  cargo test --workspace --features chrono
```
