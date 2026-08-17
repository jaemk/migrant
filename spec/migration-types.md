# Migration Types

FileMigration, EmbeddedMigration, and FnMigration registered via Config::use_migrations.

## MIGTYPE-1

`FileMigration` runs up/down SQL loaded from files at runtime.

## MIGTYPE-2

`EmbeddedMigration` runs up/down SQL from embedded strings (typically `include_str!`),
so binaries need no migration files on disk.

## MIGTYPE-3

`FnMigration` runs arbitrary Rust functions with signature
`fn(ConnConfig) -> Result<(), Box<dyn std::error::Error>>` for up and down.

## MIGTYPE-4

`Config::use_migrations(&[...])` registers an explicit, ordered migration list;
`is_explicit()` reports whether explicit migrations are in use (vs file discovery).

## MIGTYPE-5

`Migratable::use_transaction(direction)` reports whether a migration is applied
inside a transaction for that direction (default `true`). `EmbeddedMigration` and
`FileMigration` expose `no_transaction()` to opt out via the builder, or a
`-- migrant:no-transaction` directive in a direction's SQL to opt that direction
out (the directive takes precedence); `FnMigration` never runs in a
migrator-managed transaction. See
[transactional-migrations.md](transactional-migrations.md).

## MIGTYPE-6

`Migratable::checksum()` returns a sha256 checksum of a migration's up-SQL, recorded in the
bookkeeping table when the migration is applied. `FileMigration` and `EmbeddedMigration`
compute it from their up-SQL; `FnMigration` (a programmatic migration with no SQL) returns
`None`, recorded as a null checksum.

## MIGTYPE-7

`Migratable::description(Direction)` takes `Direction` by value instead of by reference.

## MIGTYPE-8

`Migratable::is_repeatable()` (default `false`) reports whether a migration re-runs on every
checksum change instead of applying once. `FileMigration` and `EmbeddedMigration` declare it
with a `repeatable()` builder method or a `-- migrant:repeatable` directive in their up-SQL
(either declares it; neither can un-declare the other), mirroring MIGTYPE-5's two forms. The
trait predicate carries the `is_`
prefix because the builder method occupies the plain name on the concrete types, the same
split as `no_transaction()` and `use_transaction()`. See
[repeatable-migrations.md](repeatable-migrations.md).

`Migratable::defines_down()` (default `false`) reports whether a migration has a down
direction to run, and exists so a repeatable migration that also defines a down can be
rejected (REPEAT-7).

## MIGTYPE-9

The down direction is optional for file-discovered migrations: a migration directory needs
only `up.sql`, and `down.sql` may be absent. Reverting a migration with no down direction
removes its bookkeeping row without running SQL, the same silent no-op `FnMigration` already
has for a missing function.

Coverage: `migrant_lib/tests/sqlite.rs`, `server_dbs.rs`, `reload_memory.rs`.
