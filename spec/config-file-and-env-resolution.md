# Config File and Env Resolution

Migrant.toml format, env:VAR value resolution, and automatic .env loading.

## CONFIG-1

`Migrant.toml` keys: `database_type` (sqlite|postgres|mysql), `migration_location`,
`database_path` (SQLite), `database_name`/`database_user`/`database_password`/
`database_host`/`database_port` (server databases), `ssl_cert_file` (postgres), and
`database_params` (key-value connection parameters). `database_port` accepts a TOML
integer or a string.

## CONFIG-2

Any config value written as `env:VAR_NAME` resolves from the process environment when
the config is loaded. This covers every settings value, including `ssl_cert_file` and
both keys and values of `database_params`. A referenced variable that is not set is a
hard error naming the variable.

## CONFIG-3

The `migrant` CLI loads a `.env` file automatically (via dotenvy) before config values
are resolved. The library does not: `Config::from_settings_file` resolves `env:` values
from the process environment as-is, and loading a `.env` file is the consumer's
responsibility.

## CONFIG-4

A relative `database_path` resolves relative to the config file's directory.

Coverage: unit tests in `migrant_lib/src/config/settings.rs`; `tests/migrant.rs`
(init --default-from-env).
