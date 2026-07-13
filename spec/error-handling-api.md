# Error Handling API

Typed Error variants and helpers.

## ERRORH-1

`Error` variants cover the main failure modes: `MigrationComplete` (nothing left to
apply/unapply), `MigrationNotFound`, `TagError` (invalid tag format), `FeatureRequired`
(operation needs a disabled cargo feature), and `Config`.

## ERRORH-2

`Error::is_migration_complete()` identifies the completion case so callers can treat it
as success; `Migrator::swallow_completion` does this automatically.

Coverage: unit tests in `migrant_lib/src/errors.rs`, `tags.rs`; exercised throughout the
integration tests.
