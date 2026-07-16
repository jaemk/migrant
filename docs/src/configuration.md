# Configuration

A project is configured by a `Migrant.toml` file. The active config is found by
searching upward from the current directory, so commands work from any
subdirectory. `migrant which-config` prints the path in use.

## Migrant.toml

Keys:

- `database_type` (required): `sqlite`, `postgres`, or `mysql`.
- `migration_location`: directory holding migration folders. Default
  `migrations`. A relative path resolves against the config file's directory.
- SQLite: `database_path`. A relative path resolves against the config file's
  directory.
- Server databases: `database_name`, `database_user`, `database_password`,
  `database_host`, `database_port`.
- `database_params`: a table of extra connection parameters.
- `ssl_cert_file` (PostgreSQL): path to a custom SSL certificate.

### SQLite

```toml
database_type = "sqlite"
database_path = "db/migrant.db"
migration_location = "migrations"
```

### PostgreSQL

```toml
database_type = "postgres"
database_name = "myapp"
database_user = "myapp"
database_password = "secret"
database_host = "localhost"
database_port = 5432
migration_location = "migrations"

[database_params]
sslmode = "require"
```

MySQL uses the same server keys with `database_type = "mysql"`. `database_port`
accepts a TOML integer or a string.

## Environment variables

Any value written as `env:VAR_NAME` is resolved from the environment when the
config loads. Keep secrets out of the file:

```toml
database_password = "env:DB_PASSWORD"
```

`migrant init --default-from-env` seeds every value in this form.

The `migrant` CLI loads a `.env` file automatically (via dotenvy) before values
are resolved, so `env:` references can come from `.env` during local
development. The library does not load `.env`: `Config::from_settings_file`
resolves `env:` references from the process environment as-is, so load any
`.env` file yourself before creating the config.

## Connection strings and SSL

`migrant connect-string` prints the resolved connection string (or the file path
for SQLite). Credentials and parameters are percent-encoded, so special
characters in passwords and params are safe.

For PostgreSQL, TLS is selected from the connection: `sslmode=disable` (or an
absent `sslmode`) connects without TLS; any other `sslmode`
(`prefer`/`require`/`verify-ca`/`verify-full`) connects with TLS using the system
trust roots. `ssl_cert_file` verifies the server against a specific root
certificate instead. See [Database backends](backends.md).
