# Repeatable Migrations

Data-migration scripts that re-run whenever their up-SQL checksum changes, as the
counterpart to versioned migrations. A versioned migration applies once and its recorded
`checksum` (see [migration-types.md](migration-types.md) MIGTYPE-6) is expected to stay
stable, so a later change is drift. A repeatable migration inverts this: a checksum change
is the signal to re-run. These suit idempotent data work (seeding, backfills, refreshing
views, re-granting privileges) rather than one-time schema versioning.

## REPEAT-1

A repeatable migration re-runs its up-direction whenever its current `checksum()` differs
from the value last recorded in `__migrant_migrations`, or when no row exists for its tag
yet. After a successful run the recorded checksum is updated to the new value. A repeatable
migration whose current checksum equals the recorded value is skipped. Within a single run a
repeatable migration runs at most once.

## REPEAT-2

Repeatable and versioned migrations share the `checksum` bookkeeping column but interpret a
mismatch oppositely. For a versioned migration a recorded-vs-current mismatch is drift and
aborts the run (see [checksum-drift-detection.md](checksum-drift-detection.md) DRIFT-1); for
a repeatable migration a mismatch is expected and drives the re-run in REPEAT-1. Repeatable
migrations are therefore excluded from the drift check entirely.

The available migration set is authoritative on kind for any tag it still defines: a migration
converted back to versioned is drift-checked again even though its recorded row still says
repeatable. The `is_repeatable` column decides only for tags that are no longer in the
available set, where nothing else records what they were (the unknown-tags check and `Down`
selection, REPEAT-5).

## REPEAT-3

A migration is declared repeatable either by the `-- migrant:repeatable` directive on a
comment line in its up-SQL, or by the `repeatable()` builder method on
[`FileMigration`](migration-types.md)/`EmbeddedMigration`. This mirrors the
`-- migrant:no-transaction` mechanism (MIGTYPE-5): the directive travels with the SQL, so it
is the only form available to migrations the `migrant` CLI discovers from disk. Either form
declares the migration repeatable; unlike `no-transaction` there is no precedence between
them, because neither can un-declare the other.

`Migratable::is_repeatable(&self) -> bool` (default `false`) reports the result, so the migrator
branches on kind through the trait and custom `Migratable` implementations can opt in. The trait
predicate is `is_repeatable` rather than `repeatable` because the builder method holds the plain
name on the concrete types, the same split as `no_transaction()` and `use_transaction()`.

## REPEAT-4

Repeatable migrations are forward-only: they have no meaningful `down`. A `Down` run never
selects them, so `redo` never reverts one. `redo`'s `Up` phase is an ordinary run, so it
re-runs a repeatable migration only under the REPEAT-1 rule (its checksum changed), the same
as any other `Up` run.

Defining a down direction on a repeatable migration is an error, not a silent no-op:
registration rejects it (REPEAT-7). Because of this the down direction of a file-discovered
migration is now optional -- `down.sql` may be absent, and a migration with no down file
reverts by removing its bookkeeping row without running SQL (see MIGTYPE-8).

## REPEAT-5

Repeatable migrations run after all pending versioned migrations in a run, and re-run in
definition order among themselves each time. They are excluded from the unknown-tags and
out-of-order checks (see [migrator-api.md](migrator-api.md) MIGRATOR-7): a repeatable tag is
not part of the versioned sequence.

Per REPEAT-2, kind comes from the available set where it defines the tag (the out-of-order
check, which walks that set) and from the recorded `is_repeatable` column where it does not
(the unknown-tags check, and `Down` selection, which both walk applied tags). A repeatable
migration removed from the codebase therefore leaves a row that is skipped rather than
reported as an unknown tag or treated as a missing `Down` target.

## REPEAT-6

A repeatable migration keeps a single row in `__migrant_migrations`, updated in place
(`checksum` and `applied_at`) on each re-run, rather than inserting a new row per run. The
bookkeeping table carries an `is_repeatable` column (see
[database-backends.md](database-backends.md) BACKEND-6) recording the kind of each row, so a
re-run updates rather than duplicates and so drift/unknown-tag logic treats each kind
correctly even for a tag that has since been removed from the available set. The row keeps
its original `id`, so a re-run does not change recorded application order.

## REPEAT-7

Registering a migration that is repeatable but whose `checksum()` is `None` is an error:
without a checksum there is no basis for "changed since last run", so the declaration is
rejected rather than silently always-running or never-running. Declaring both `repeatable()`
and a down direction is likewise an error (REPEAT-4). Both are reported as
`Error::Migration` from `Config::use_migrations` for explicitly defined migrations, and from
the migrator's own load of the available set for file-discovered ones. In practice this
means `FnMigration` (which has no SQL to hash) cannot be repeatable.

## REPEAT-8

`status` and `list` ([cli-migration-management.md](cli-migration-management.md) CLIMIG-3,
CLIMIG-6) surface repeatable migrations and whether each is up-to-date (recorded checksum
matches) or stale (will re-run). `MigrationStatus` exposes `repeatable()` and `stale()`
alongside `applied()`, and `pending_migrations` lists stale repeatable tags after the pending
versioned ones, in the order a run would apply them. A run's `Report`
([migrator-api.md](migrator-api.md) MIGRATOR-1) reports the repeatable tags it re-ran through
`Report::repeatable_tags()`, a subset of `tags()`.

## REPEAT-9

Repeatable migrations follow the same transactional wrapping as versioned ones (see
[transactional-migrations.md](transactional-migrations.md) and MIGTYPE-5): the re-run and
its in-place bookkeeping update commit or roll back together, subject to the usual
`no_transaction` opt-outs. `fake` and the `ForceMode` behaviors apply unchanged, with
"recorded" meaning the in-place update of REPEAT-6.

## REPEAT-10

`migrant new --repeatable <tag>` creates a migration directory containing only an `up.sql`
seeded with the `-- migrant:repeatable` directive, and no `down.sql` -- the shape REPEAT-4
and REPEAT-7 require.

## REPEAT-11

A `Down` run leaves repeatable rows in place (REPEAT-4), so after reverting and re-applying
every versioned migration a repeatable migration whose SQL has not changed stays skipped.
Re-running it is triggered by changing its SQL, which is the same signal as REPEAT-1.

## REPEAT-12

`Migrator::rerun_repeatable(bool)` (default `false`) re-runs every repeatable migration on an
`Up` run whether or not its checksum changed, so re-running an unedited one does not require
touching its SQL. Everything else is unchanged: they still run after the pending versioned
migrations, still at most once per run (so an `all` run still terminates), and each still
records its checksum afterwards. It never re-applies a versioned migration, and has no effect
on a `Down` run. The CLI exposes it as `--rerun-repeatable` on `apply` and `redo`.

## REPEAT-13

Because a `Down` run walks past repeatable migrations (REPEAT-4), `redo` reverts and re-applies
the most recent *versioned* migration, which may not be the migration the user was iterating
on. `migrant redo` therefore prints a note to stderr naming the applied repeatable migrations
it will not revert, and pointing at `--rerun-repeatable`. The note is suppressed when there are
no applied repeatable migrations, or when `--rerun-repeatable` was passed.

Coverage: `migrant_lib/tests/sqlite.rs`, `server_dbs.rs`, `tests/migrant.rs`; unit tests in
`migrant_lib/src/migration.rs`, `migratable.rs`, `migrator.rs`, `ops.rs`, `drivers/mod.rs`,
`drivers/sqlite.rs`, and `src/status.rs`.
