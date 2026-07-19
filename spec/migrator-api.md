# Migrator API

Migrator builder: direction, all, force, fake, show_output, synchronized, apply.

## MIGRATOR-1

`Migrator::with_config(&config)` creates a migrator; `apply()` executes pending
migrations against the live connection and returns a `Report`. A run re-reads
applied state from the database itself (for every backend), so consumers do not
need to call `Config::reload` before applying. The re-read never re-reads the
settings file or swaps the live connection: the whole run stays on the
connection it started (and, when synchronized, took the advisory lock) on.

`apply()` returns `Result<Report>`. `Report::tags()` lists the migration tags
whose bookkeeping the run changed, in order (applied for `Up`, reverted for
`Down`); `Report::is_empty()`/`len()` report the count. A run with nothing to
apply returns an empty `Report`, not an error.

## MIGRATOR-2

`direction(Direction::Up|Down)` sets the migration direction; `all(bool)` applies every
remaining migration in that direction instead of just the next one.

## MIGRATOR-3

`force(ForceMode)` controls how a run handles a migration that fails to apply:

- `ForceMode::Off` (default): the failure aborts the run with an error and the
  migration is not recorded.
- `ForceMode::AcceptFailures`: the run continues and the failed migration is
  recorded as applied anyway, so it is not retried on later runs.
- `ForceMode::SkipFailures`: the run continues without recording the failed
  migration; it is skipped for the remainder of the run (so an `all` run
  terminates) and retried on the next run.

`ForceMode` parses from `off` / `accept-failures` / `skip-failures`
(`FromStr`). `fake(bool)` updates the tracking table without executing
migration SQL.

## MIGRATOR-4

`show_output(bool)` toggles progress output. There is no separate "nothing to
apply" error to suppress: a run with no pending migrations returns an empty
`Report` (see MIGRATOR-1).

## MIGRATOR-5

`synchronized(bool)` (default `true`) serializes migration runs across processes with a
database advisory lock, and each migration is applied in a transaction with its bookkeeping
row. See [advisory-locking.md](advisory-locking.md) and
[transactional-migrations.md](transactional-migrations.md).

## MIGRATOR-6

The `Migrator` setters `direction`, `force`, `fake`, `all`, `show_output`, and
`synchronized` take and return an owned `self`, not `&mut self`, so calls chain by
value:

```rust
Migrator::with_config(&config)
    .direction(Direction::Up)
    .all(true)
    .apply()?;
```

`with_config(&Config)` and `apply(&self)` are unchanged.

Coverage: `migrant_lib/tests/sqlite.rs`, `server_dbs.rs`, `reload_memory.rs`,
`tests/migrant.rs`; unit tests in `migrant_lib/src/migrator.rs`.
