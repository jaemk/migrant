# Database backends

migrant supports SQLite, PostgreSQL, and MySQL. Each is behind a cargo feature so
you only build the drivers you need.

| Database | CLI feature | Library feature | Driver |
|----------|-------------|-----------------|--------|
| SQLite | `sqlite` | `d-sqlite` | rusqlite |
| PostgreSQL | `postgres` | `d-postgres` | postgres |
| MySQL | `mysql` | `d-mysql` | mysql |

The library also has `d-all` to enable all three. No backend is enabled by
default; invoking an operation whose feature is disabled returns
`Error::FeatureRequired` rather than panicking.

## SQLite

File-backed or in-memory. The `sqlite` CLI feature bundles SQLite; the library's
`d-sqlite` does not bundle it (enable rusqlite's `bundled` feature in your own
project if you want that). DDL is transactional, so migrations roll back cleanly
on failure.

An in-memory database (`:memory:`) lives entirely in one connection. migrant
keeps that connection alive for the life of the `Config` and its clones, so
migrations and later queries see the same database. See
[Using migrant_lib](library.md).

## PostgreSQL

DDL is transactional except for a handful of statements that cannot run in a
transaction block (`CREATE INDEX CONCURRENTLY`, `ALTER TYPE ... ADD VALUE`,
`VACUUM`, and similar). Opt those migrations out with the
`-- migrant:no-transaction` directive; see [Transactions](transactions.md).

TLS is chosen from the connection string's `sslmode`:

- absent or `sslmode=disable`: no TLS (the default).
- any other value (`prefer`/`require`/`verify-ca`/`verify-full`): TLS using the
  system trust roots.
- `ssl_cert_file` in the config: verify the server against that root certificate.

The `postgres` driver needs `libpq`'s dev package (`libpq-dev`) to build.

## MySQL / MariaDB

DDL commits implicitly, one statement at a time. A transaction cannot roll back
DDL, so transaction wrapping only makes pure-DML migrations atomic. This is a
property of the database; the `-- migrant:no-transaction` directive and
`no_transaction()` do not change it. Plan MySQL DDL migrations so a partial
application is safe to retry.

## Static builds

The CLI's `vendored-openssl` feature statically links OpenSSL for portable
(musl) builds. Release binaries are built with all features.
