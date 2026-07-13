# Troubleshooting

## `__migrant_migrations` table is missing

Run `migrant setup` (or `Config::setup()` in the library) before applying
migrations. It verifies credentials and creates the tracking table.

## A PostgreSQL migration errors with "cannot run inside a transaction block"

migrant wraps each migration in a transaction by default. Statements like
`CREATE INDEX CONCURRENTLY` and `ALTER TYPE ... ADD VALUE` cannot run there. Put
the directive at the top of that direction's SQL:

```sql
-- migrant:no-transaction
alter type mood add value 'excited';
```

See [Transactions](transactions.md).

## A MySQL migration was not rolled back on failure

MySQL commits DDL implicitly, so a failed DDL migration is not rolled back. Only
pure-DML migrations are atomic on MySQL. Write DDL migrations so a partial
application is safe to retry. See [Database backends](backends.md#mysql--mariadb).

## A second migrator seems to hang

Against PostgreSQL/MySQL, migrant takes an advisory lock for the run. A second
process blocks until the first releases it. This is expected: it prevents
concurrent runs from racing. See [Concurrency and locking](concurrency.md). If a
process truly died holding the lock, the database releases it when that session
ends.

## `migrant self update` says it is unavailable

Self-update only works when the binary was built with the `update` feature.
Release binaries include it; `cargo install migrant --features postgres` (for
example) does not. Reinstall from a [release](https://github.com/jaemk/migrant/releases)
or add the `update` feature.

## `migrant shell` cannot find the client

`shell` runs the database's own client: `sqlite3`, `psql`, or `mysqlsh`. Install
the matching client and make sure it is on your `PATH`.

## Wrong config is picked up

migrant searches upward from the current directory for `Migrant.toml`. Run
`migrant which-config` to see which file is active.
