# Checksum Drift Detection

An `Up` run verifies that every already-applied migration still matches the `checksum`
recorded in `__migrant_migrations` (see [migration-types.md](migration-types.md) MIGTYPE-6)
before applying anything, so an edit to already-applied SQL is caught instead of silently
building on a changed migration. This is the enforcement counterpart to
[repeatable-migrations.md](repeatable-migrations.md), where a checksum change is instead the
signal to re-run.

## DRIFT-1

Before an `Up` run applies any migration, migrant compares each already-applied migration's
current `checksum()` against the checksum recorded for its tag. A difference is drift: the
run aborts with [`Error::ChecksumMismatch`](error-handling-api.md) before applying anything,
naming the drifted tag. The check runs alongside the unknown-tags and out-of-order checks
(see [migrator-api.md](migrator-api.md) MIGRATOR-7) and, like them, runs even when no
migrations are pending (so `apply` on an up-to-date database still validates checksums).

## DRIFT-2

The comparison only fires when both sides are present: the recorded checksum is non-null and
the migration's current `checksum()` is `Some`. A null on either side is skipped, not a
mismatch. This covers programmatic migrations (`FnMigration`, which records a null checksum)
and legacy rows recorded before checksums existed or backfilled null during an in-place
schema upgrade.

## DRIFT-3

The check covers only tags that are both applied and present in the available migration set.
An applied tag absent from the available set is handled by the unknown-tags check
(MIGRATOR-7), not here.

## DRIFT-4

`Migrator::allow_checksum_mismatch(bool)` (default `false`) opts out of the check, applying
despite drift. It is independent of `allow_unknown_tags` and `allow_out_of_order`: each
check is enabled or bypassed on its own.

## DRIFT-5

The check is part of the `Up`-run consistency checks. A pure `Down` run (`apply --down`)
does not trigger it; `redo` does, via its `Up` phase.

## DRIFT-6

The CLI exposes the opt-out as `--allow-checksum-mismatch` on `apply` and `redo`
(see [cli-migration-management.md](cli-migration-management.md)), off by default.

Coverage: `migrant_lib/tests/sqlite.rs`, `server_dbs.rs`, `tests/migrant.rs`; unit tests in
`migrant_lib/src/migrator.rs`.
