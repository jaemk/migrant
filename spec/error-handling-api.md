# Error Handling API

Typed Error variants and helpers.

## ERRORH-1

`Error` variants cover the main failure modes: `Migration`, `MigrationNotFound`,
`MigrationOrdering` (an applied migration is out of definition order),
`ChecksumMismatch` (an already-applied migration's SQL changed since it was
recorded; see [checksum-drift-detection.md](checksum-drift-detection.md)),
`TagError` (invalid tag format), `ShellCommand`, `PathError`, `InvalidDbKind`,
`FeatureRequired` (operation needs a disabled cargo feature), and `Config`. The
enum is `#[non_exhaustive]`. There is no "nothing to apply" error variant: a run
with nothing pending returns an empty `Report` (see [migrator-api.md](migrator-api.md)).

## ERRORH-2

`Error` exposes predicate methods for branching without matching the
`#[non_exhaustive]` enum: `is_config`, `is_migration`, `is_migration_not_found`,
`is_migration_ordering`, `is_checksum_mismatch`, `is_shell_command`, `is_tag_error`,
`is_invalid_db_kind`, `is_feature_required`.

## ERRORH-3

`DbKind` and `ForceMode` are also `#[non_exhaustive]`, so new database backends
or force modes can be added without a breaking change; downstream `match`
expressions on either enum must include a wildcard arm.

## ERRORH-4

`MigrationStatus`'s fields are private, accessed via `tag(&self) -> &str`,
`applied(&self) -> bool`, `repeatable(&self) -> bool`, and `stale(&self) -> bool`
(see [repeatable-migrations.md](repeatable-migrations.md) REPEAT-8).

## ERRORH-5

An unusable repeatable declaration (a repeatable migration with no checksum, or one that
defines a down direction) is reported as `Error::Migration`, from `Config::use_migrations` for
explicitly defined migrations and from the migrator's load of the available set for
file-discovered ones. See [repeatable-migrations.md](repeatable-migrations.md) REPEAT-7.

Coverage: unit tests in `migrant_lib/src/errors.rs`, `tags.rs`; exercised throughout the
integration tests.
