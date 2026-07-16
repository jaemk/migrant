# Error Handling API

Typed Error variants and helpers.

## ERRORH-1

`Error` variants cover the main failure modes: `Migration`, `MigrationNotFound`,
`TagError` (invalid tag format), `ShellCommand`, `PathError`, `InvalidDbKind`,
`FeatureRequired` (operation needs a disabled cargo feature), and `Config`. The
enum is `#[non_exhaustive]`. There is no "nothing to apply" error variant: a run
with nothing pending returns an empty `Report` (see [migrator-api.md](migrator-api.md)).

## ERRORH-2

`Error` exposes predicate methods for branching without matching the
`#[non_exhaustive]` enum: `is_config`, `is_migration`, `is_migration_not_found`,
`is_shell_command`, `is_tag_error`, `is_invalid_db_kind`, `is_feature_required`.

Coverage: unit tests in `migrant_lib/src/errors.rs`, `tags.rs`; exercised throughout the
integration tests.
