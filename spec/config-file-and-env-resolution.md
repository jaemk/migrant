# Config File and Env Resolution

Migrant.toml format, env:VAR value resolution, and automatic .env loading.

## CONFIG-1

`Migrant.toml` keys: `database_type` (sqlite|postgres|mysql), `migration_location`,
`database_path` (SQLite), `database_name`/`database_user`/`database_password`/
`database_host`/`database_port` (server databases), `ssl_cert_file` (postgres), and
`database_params` (key-value connection parameters).

## CONFIG-2

Any config value written as `env:VAR_NAME` resolves from the environment when the config
is loaded.

## CONFIG-3

A `.env` file is loaded automatically (via dotenvy) before config values are resolved.

## CONFIG-4

A relative `database_path` resolves relative to the config file's directory.

Coverage: unit tests in `migrant_lib/src/config/settings.rs`; `tests/migrant.rs`
(init --default-from-env).
