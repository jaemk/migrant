# Migrator API

Migrator builder: direction, all, force, fake, show_output, swallow_completion, apply.

## MIGRATOR-1

`Migrator::with_config(&config)` creates a migrator; `apply()` executes pending
migrations against the live connection.

## MIGRATOR-2

`direction(Direction::Up|Down)` sets the migration direction; `all(bool)` applies every
remaining migration in that direction instead of just the next one.

## MIGRATOR-3

`force(bool)` continues applying past SQL errors; `fake(bool)` updates the tracking table
without executing migration SQL.

## MIGRATOR-4

`show_output(bool)` toggles progress output; `swallow_completion(bool)` converts the
`MigrationComplete` error into `Ok` so "nothing to apply" is not an error.

## MIGRATOR-5

`synchronized(bool)` (default `true`) serializes migration runs across processes with a
database advisory lock, and each migration is applied in a transaction with its bookkeeping
row. See [advisory-locking.md](advisory-locking.md) and
[transactional-migrations.md](transactional-migrations.md).

Coverage: `migrant_lib/tests/sqlite.rs`, `server_dbs.rs`, `reload_memory.rs`,
`tests/migrant.rs`; unit tests in `migrant_lib/src/migrator.rs`.
