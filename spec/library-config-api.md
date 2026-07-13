# Library Config API

migrant_lib Config: load from Migrant.toml or explicit Settings, setup, reload,
CLI-compatible tag mode.

## LIBRAR-1

`Config::from_settings_file(path)` loads configuration from a `Migrant.toml` file.
`search_for_settings_file(dir)` locates one by walking parent directories.

## LIBRAR-2

`Config::with_settings(&settings)` builds a config from an explicit `Settings` value,
with no config file on disk.

## LIBRAR-3

`Config::setup()` verifies the database connection and creates the migration tracking
table if missing.

## LIBRAR-4

`Config::reload()` re-reads the settings file (when one is used) and refreshes the applied
migration list. For in-memory SQLite the live connection is preserved across reloads.

## LIBRAR-5

`Config::use_cli_compatible_tags(bool)` toggles timestamp-prefixed tag validation so
library-managed migrations interoperate with CLI-created ones; `is_cli_compatible()`
reports the current mode.

Coverage: `migrant_lib/tests/sqlite.rs`, `server_dbs.rs`, `reload_memory.rs`; unit tests in
`migrant_lib/src/tags.rs`.
