# In-Memory SQLite

Shared in-process :memory: database with live connection access.

## INMEMO-1

`SqliteSettingsBuilder::memory()` (or a `:memory:` database path) configures an in-process
SQLite database instead of a file.

## INMEMO-2

The in-memory connection is established once and kept alive for the lifetime of the
`Config`; clones of the `Config` share the same database.

## INMEMO-3

`Config::sqlite_connection()` returns the live `Arc<Mutex<rusqlite::Connection>>` so
application code can query the same in-memory database the migrations ran against.

## INMEMO-4

`ConnConfig::sqlite_connection()` exposes the same connection inside `FnMigration`
functions.

## INMEMO-5

`Config::reload()` does not drop the in-memory database.

Coverage: `migrant_lib/tests/sqlite.rs` (in_memory_database_end_to_end,
in_memory_database_shared_across_clones), `reload_memory.rs`.
