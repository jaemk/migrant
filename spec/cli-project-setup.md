# CLI Project Setup

init, setup, which-config, and connect-string subcommands for bootstrapping a project.

## CLIPRO-1

`migrant init` creates a `Migrant.toml` config file. Flags: `--type <sqlite|postgres|mysql>`
selects the database type, `--location <dir>` initializes in a specific directory,
`--default-from-env` seeds values as `env:VAR_NAME` references, `--no-confirm` disables
interactive prompts.

## CLIPRO-2

`migrant setup` verifies database credentials and creates the `__migrant_migrations`
tracking table if it does not exist.

## CLIPRO-3

`migrant which-config` prints the path of the active `Migrant.toml`. The active config is
found by searching upward from the current directory.

## CLIPRO-4

`migrant connect-string` prints the database connection string, or the database file path
for SQLite.

Coverage: `tests/migrant.rs` (kitchen_sink) and backend integration tests.
