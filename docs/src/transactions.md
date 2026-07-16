# Transactions

By default, migrant applies each migration's SQL and its `__migrant_migrations`
bookkeeping row in a single transaction. The schema change and the record that it
was applied commit or roll back together, so a migration that fails partway
leaves neither partial schema nor a bookkeeping row. Do not add your own
`begin`/`commit` to migration SQL; migrant manages the transaction.

## What is wrapped

Wrapping is resolved per direction (`up` vs `down`). The default is on. It covers
the SQL migrant runs itself:

- CLI file migrations (`up.sql`/`down.sql`).
- Library `FileMigration` and `EmbeddedMigration`.

Function migrations (`FnMigration`) run arbitrary Rust and may open their own
connections, so migrant never wraps them.

## Backend behavior

The guarantee depends on how each backend treats DDL inside a transaction:

| Backend | DDL in a transaction | Result |
|---------|----------------------|--------|
| SQLite | transactional | schema and DML roll back together |
| PostgreSQL | transactional (with exceptions below) | schema and DML roll back together |
| MySQL / MariaDB | implicit commit per DDL statement | only pure-DML migrations are atomic; DDL cannot roll back |

On MySQL a `CREATE TABLE`/`ALTER TABLE` commits the moment it runs, so wrapping
there makes only DML-only migrations atomic. This is a property of the database,
not a setting.

## Opting out: statements that cannot run in a transaction

Some PostgreSQL statements refuse to run inside a transaction block, for example
`CREATE INDEX CONCURRENTLY` and `ALTER TYPE ... ADD VALUE`. Wrapping such a
migration would error. Opt the affected direction out.

### From the SQL file (CLI and library)

Put the directive on a comment line in that direction's SQL. This is the way to
opt out from a CLI file migration, with no Rust code:

```sql
-- migrant:no-transaction
alter type mood add value 'excited';
```

Rules:

- Resolved per direction and per source: an `up.sql`/`down.sql` for a file
  migration, the embedded string for an embedded migration. Put it only in the
  direction that needs it.
- Matched case-insensitively as the first token of a `--` comment, so a trailing
  note is allowed: `-- migrant:no-transaction (enum add)`.
- A directive in the SQL takes precedence over the builder flag below.

### From Rust (library)

`EmbeddedMigration` and `FileMigration` expose `no_transaction()`, which opts
*both* directions out:

```rust
EmbeddedMigration::with_tag("add-index")
    .up("create index concurrently idx_users_email on users (email);")
    .down("drop index idx_users_email;")
    .no_transaction()
    .boxed();
```

For per-direction control, prefer the SQL directive. When both are present, the
directive wins.

## What opting out changes

Without a transaction, a failed migration is not rolled back: earlier statements
in the file stay applied. migrant still does not record the migration as applied
when it fails, so a re-run will attempt it again. Write opted-out migrations so a
partial application is safe to retry.

Note that `--force` changes the recording rule: bare `--force`
(`accept-failures`) records a failed migration as applied anyway, while
`--force=skip-failures` keeps the not-recorded/retry behavior described above.
See [the apply flags](cli.md).

## Interaction with locking

Transaction wrapping is independent of the migration advisory lock. The lock
spans the whole run and each migration's transaction nests inside it. See
[Concurrency and locking](concurrency.md).
