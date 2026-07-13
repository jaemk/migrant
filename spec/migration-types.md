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

Coverage: `migrant_lib/tests/sqlite.rs`, `server_dbs.rs`, `reload_memory.rs`.
