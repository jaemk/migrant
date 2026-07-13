# Settings Builders

Typed builders for sqlite, postgres, and mysql settings.

## SETTIN-1

`Settings::configure_sqlite()` returns a `SqliteSettingsBuilder` with `database_path`
(absolute, or relative to the config file), `memory()` for an in-memory database, and
`migration_location`.

## SETTIN-2

`Settings::configure_postgres()` returns a `PostgresSettingsBuilder` with
`database_name`/`database_user`/`database_password`/`database_host`/`database_port`,
`ssl_cert_file` for a custom SSL certificate, and `database_params` for extra connection
parameters.

## SETTIN-3

`Settings::configure_mysql()` returns a `MySqlSettingsBuilder` with the same
name/user/password/host/port and `database_params` options.

## SETTIN-4

Generated connection strings percent-encode credentials and parameters so special
characters in passwords and params are safe.

Coverage: `migrant_lib/tests/server_dbs.rs`, `sqlite.rs`; unit tests in
`migrant_lib/src/config/builders.rs`. `ssl_cert_file` has no dedicated test.
