# CLI Migration Management

new, edit, list, apply, and redo subcommands for creating and running migrations.

## CLIMIG-1

`migrant new <tag>` generates a timestamped up/down migration file pair under the configured
migration location. `--repeatable` instead generates only an `up.sql`, seeded with the
`-- migrant:repeatable` directive (see
[repeatable-migrations.md](repeatable-migrations.md) REPEAT-10).

## CLIMIG-2

`migrant edit <tag>` opens the migration's up file in `$EDITOR`; `--down` selects the down
file instead.

## CLIMIG-3

`migrant list` displays all managed migrations with their applied status.

## CLIMIG-4

`migrant apply` applies all pending migrations by default. Flags: `--step N` applies exactly
N migrations instead of all of them; `--down` reverses direction (unapplies) and defaults to
a single step unless `--step N` is also given; `--fake` marks migrations applied/unapplied
without executing their SQL. `--force[=<mode>]` continues past failed migrations: bare
`--force` (or `--force=accept-failures`) records a failed migration as applied so it is not
retried; `--force=skip-failures` leaves it unrecorded, skips it for the rest of the run, and
retries it on the next run. `apply` no longer has an `--all` flag: applying all pending
migrations is the default behavior.

## CLIMIG-5

`migrant redo` unapplies then reapplies the latest migration (`--down` then up); `--all`
redoes all applied migrations. Down-migrations run in reverse application order. `redo` warns
when it will not revert applied repeatable migrations (see
[repeatable-migrations.md](repeatable-migrations.md) REPEAT-13).

## CLIMIG-6

`migrant status` reports every managed migration with its applied/pending state plus summary
counts (total, applied, pending, stale). The summary `stale` count covers only applied
migrations that will re-run (repeatable ones whose SQL changed), so `applied + pending` still
equals `total`; the per-row `stale` field is broader, true for anything that would run next
including a versioned migration with no row yet. `--format text` (the default) prints a summary line
followed by a `[✓]`/`[ ]` row per migration; a repeatable migration is annotated
`(repeatable)` and, when stale, marked `[~]` with `(repeatable, will re-run)`. The summary
line reports the stale count only when it is non-zero. `--format json` prints the same data
as pretty-printed JSON
(`{ total, applied, pending, stale, migrations: [{ tag, applied, repeatable, stale }] }`)
for scripting.

## CLIMIG-7

`apply` and `redo` accept `--allow-unknown-tags`, `--allow-out-of-order`, and
`--allow-checksum-mismatch`, all off by default. `--allow-unknown-tags` permits a run when the
database has an applied tag not present in the defined migration set, instead of erroring.
`--allow-out-of-order` permits a run to apply migrations out of their defined order, instead of
erroring. `--allow-checksum-mismatch` permits a run when an already-applied migration's SQL has
changed since it was recorded (see [checksum-drift-detection.md](checksum-drift-detection.md)),
instead of erroring.

Coverage: `tests/migrant.rs` (kitchen_sink, new_rejects_invalid_tag,
apply_fake_records_without_running, force_modes_through_the_cli, status_reports_text_and_json),
backend integration tests, unit tests in `migrant_lib/src/ops.rs` and `src/status.rs`.
