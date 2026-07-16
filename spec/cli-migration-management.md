# CLI Migration Management

new, edit, list, apply, and redo subcommands for creating and running migrations.

## CLIMIG-1

`migrant new <tag>` generates a timestamped up/down migration file pair under the configured
migration location.

## CLIMIG-2

`migrant edit <tag>` opens the migration's up file in `$EDITOR`; `--down` selects the down
file instead.

## CLIMIG-3

`migrant list` displays all managed migrations with their applied status.

## CLIMIG-4

`migrant apply` applies the next unapplied migration. Flags: `--all` applies all remaining,
`--down` reverses direction (unapplies), `--fake` marks migrations applied/unapplied without
executing their SQL. `--force[=<mode>]` continues past failed migrations: bare `--force` (or
`--force=accept-failures`) records a failed migration as applied so it is not retried;
`--force=skip-failures` leaves it unrecorded, skips it for the rest of the run, and retries
it on the next run.

## CLIMIG-5

`migrant redo` unapplies then reapplies the latest migration (`--down` then up); `--all`
redoes all applied migrations. Down-migrations run in reverse application order.

Coverage: `tests/migrant.rs` (kitchen_sink, new_rejects_invalid_tag,
apply_fake_records_without_running, force_modes_through_the_cli), backend integration tests,
unit tests in `migrant_lib/src/ops.rs`.
