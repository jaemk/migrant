# Quickstart

A file-based SQLite project from scratch. Every command is run from inside the
project directory (the one holding `Migrant.toml`).

## 1. Initialize

```sh
migrant init --type sqlite
```

This writes a `Migrant.toml` and, when run interactively, runs `setup` to create
the `__migrant_migrations` tracking table. See [Configuration](configuration.md)
for the file format and server-database options.

## 2. Create a migration

```sh
migrant new create-users
```

This creates a timestamped directory under `migrations/` with empty `up.sql` and
`down.sql` files:

```
migrations/
  20260713094500_create-users/
    up.sql
    down.sql
```

Fill them in. Do not add your own `begin`/`commit`: migrant wraps each migration
in a transaction (see [Transactions](transactions.md)).

```sql
-- up.sql
create table users (id integer primary key, name text not null);
```

```sql
-- down.sql
drop table users;
```

## 3. Apply

```sh
migrant list          # show applied vs available
migrant apply --all   # apply every pending up migration
```

`apply` moves one migration at a time by default; `--all` runs every remaining
one. To revert, add `--down`:

```sh
migrant apply --down        # revert the most recent migration
migrant apply --down --all  # revert everything
```

## 4. Inspect

```sh
migrant list            # status of each migration
migrant which-config    # path of the active Migrant.toml
migrant shell           # open a database repl (sqlite3/psql/mysqlsh)
migrant tui             # interactive terminal UI
```

That is the whole loop: `new`, edit the SQL, `apply`. The full command reference
is in [The CLI](cli.md).
