# Introduction

`migrant` is database migration management for SQLite, PostgreSQL, and MySQL. It
is two things that share one engine (`migrant_lib`):

- A **CLI** (`migrant`) that manages plain `up.sql`/`down.sql` files under a
  project directory, tracking applied migrations in a `__migrant_migrations`
  table.
- A **library** (`migrant_lib`) that embeds the same management in your own
  application. Migrations can be SQL files, SQL strings compiled into the binary
  (`include_str!`), or Rust functions.

Migrations are applied in definition order (file migrations order by a timestamp
in the tag). Each migration is applied together with its bookkeeping row in a
single transaction by default, and concurrent migration runs against a server
database serialize behind an advisory lock.

This site is the reference for installing, using, and embedding `migrant`. Start
with [Install](install.md) and the [Quickstart](quickstart.md). [The CLI](cli.md)
is the full command reference and [Writing migrations](migrations.md) covers the
file layout. For the behavior that matters most in production, see
[Transactions](transactions.md) and [Concurrency and locking](concurrency.md).
To embed migrant in your own program, see [Using migrant_lib](library.md).

The normative behavior is the
[spec](https://github.com/jaemk/migrant/tree/main/spec).
