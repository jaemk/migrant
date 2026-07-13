# Transactional Migrations

Each migration's SQL and its bookkeeping row are applied atomically, with a
per-migration opt-out.

## TXN-1

By default the migrator wraps a migration's application and its bookkeeping row
(the insert/delete in `__migrant_migrations`) in a single database transaction,
so the schema change and the record that it was applied commit or roll back
together. A migration that fails partway leaves neither partial state nor a
bookkeeping row.

## TXN-2

`Migratable::use_transaction()` controls wrapping (default `true`).
`EmbeddedMigration` and `FileMigration` expose `no_transaction()` to opt a
single migration out -- required for statements a backend refuses to run inside
a transaction block, such as Postgres `CREATE INDEX CONCURRENTLY`. `FnMigration`
is never wrapped: it runs arbitrary code that may use its own connection, so it
returns `false`.

## TXN-3

Migration SQL must not contain its own `begin`/`commit`; the migrator manages
the transaction. Migrations needing to control transactions themselves opt out
with `no_transaction()`.

## TXN-4

Backend behavior differs. Sqlite and Postgres roll DDL back, so wrapping makes
both DDL and DML migrations atomic. MySQL/MariaDB commit DDL implicitly, so
wrapping only makes pure-DML migrations atomic there; DDL cannot be rolled back
regardless of this setting.

Coverage: `migrant_lib/tests/sqlite.rs`
(`failed_migration_rolls_back_atomically`, `no_transaction_migration_leaves_partial_state`);
`server_dbs.rs` (`postgres_end_to_end` atomic-rollback phase, run via `test.sh`).
