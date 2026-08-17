# Database Backends

sqlite, postgres, and mysql drivers behind cargo feature flags.

## BACKEND-1

SQLite support (feature `sqlite`, via rusqlite): file-backed and in-memory databases.

## BACKEND-2

PostgreSQL support (feature `postgres`, via the postgres crate), including SSL with an
optional custom certificate.

## BACKEND-3

MySQL support (feature `mysql`, via the mysql crate).

## BACKEND-4

Connections are established lazily on first use and kept alive per `Config`.

## BACKEND-5

Invoking an operation whose backend feature is disabled returns
`Error::FeatureRequired` rather than panicking.

## BACKEND-6

The `__migrant_migrations` bookkeeping table has five columns on all three backends: `id`
(an auto-incrementing key recording applied order), `tag` (the migration tag), `checksum`
(a sha256 checksum of the migration's up-SQL, null for programmatic migrations),
`applied_at` (a timestamp of when the migration was applied), and `is_repeatable` (whether
the row records a repeatable migration, default false). The column is named `is_repeatable`
rather than `repeatable` because the latter is a keyword on both postgres and mysql and
would need per-backend quoting.

A repeatable migration's row is updated in place on each re-run rather than re-inserted, so
`checksum` and `applied_at` change while `id` (and therefore recorded application order)
does not. See [repeatable-migrations.md](repeatable-migrations.md) REPEAT-6.

Coverage: `migrant_lib/tests/sqlite.rs`; `server_dbs.rs` (postgres/mysql end-to-end,
gated on POSTGRES_TEST_CONN_STR/MYSQL_TEST_CONN_STR, run via `test.sh` against docker
databases).
