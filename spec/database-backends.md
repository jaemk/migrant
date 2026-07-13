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

Coverage: `migrant_lib/tests/sqlite.rs`; `server_dbs.rs` (postgres/mysql end-to-end,
gated on POSTGRES_TEST_CONN_STR/MYSQL_TEST_CONN_STR, run via `test.sh` against docker
databases).
