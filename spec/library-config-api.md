# Library Config API

migrant_lib Config: load from Migrant.toml or explicit Settings, setup, reload,
CLI-compatible tag mode.

## LIBRAR-1

`Config::from_settings_file(path)` loads configuration from a `Migrant.toml` file.
`search_for_settings_file(dir)` locates one by walking parent directories.

## LIBRAR-2

`Config::with_settings(settings)` builds a config from an explicit `Settings` value
(taken by value), with no config file on disk.

## LIBRAR-3

`Config::setup()` verifies the database connection and creates the migration tracking
table if missing.

## LIBRAR-4

`Config::reload()` re-reads the settings file (when one is used) and refreshes the applied
migration list. For in-memory SQLite the live connection is preserved across reloads.

## LIBRAR-5

`Config::use_cli_compatible_tags(bool)` toggles timestamp-prefixed tag validation so
library-managed migrations interoperate with CLI-created ones, and returns `&mut Self`
so it can be chained onto construction before `use_migrations`/`reload`;
`is_cli_compatible()` reports the current mode.

## LIBRAR-6

`Config::init_in(dir)` returns a `SettingsFileInitializer` that writes a new `Migrant.toml`
template. Its setters take and return an owned `Self` so calls chain by value: `interactive(bool)`
(default `true`; when on, `initialize()` opens the file in `$EDITOR` and runs `setup`),
`with_env_defaults(bool)` (seed every unset value as `env:VAR`), and `with_sqlite_options` /
`with_postgres_options` / `with_mysql_options`, each taking its settings builder by value.
`initialize()` renders and writes the template. Without a database type set it either prompts
(interactive) or errors (non-interactive).

Coverage: `tests/migrant.rs` (init_non_interactive_creates_config,
init_rejects_invalid_database_type, init --default-from-env); doc examples in
`migrant_lib/src/config/init.rs`.
