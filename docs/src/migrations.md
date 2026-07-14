# Writing migrations

A CLI migration is a directory holding an `up.sql` and a `down.sql`, named with a
timestamp and a tag:

```
migrations/
  20260713094500_create-users/
    up.sql
    down.sql
  20260714101500_add-users-email/
    up.sql
    down.sql
```

`migrant new <tag>` generates the directory and the two empty files. The
timestamp prefix defines application order: migrations apply oldest-first on the
way up, newest-first on the way down. Tags may contain `[a-z0-9-]`.

## up and down

`up.sql` moves the schema forward; `down.sql` reverses it. Keep them inverses so
`apply --down` cleanly undoes `apply`.

```sql
-- 20260714101500_add-users-email/up.sql
alter table users add column email text;
```

```sql
-- 20260714101500_add-users-email/down.sql
alter table users drop column email;
```

Multiple statements per file are fine. Do not wrap them in your own
`begin`/`commit`: migrant applies each migration inside a transaction already
(see [Transactions](transactions.md)).

## Order and the tracking table

Applied migrations are recorded by tag in the `__migrant_migrations` table.
`migrant list` reads that table to mark which migrations are applied:

```
Current Migration Status:
 -> [✓] 20260713094500_create-users
 -> [ ] 20260714101500_add-users-email
```

`apply` runs the next unapplied migration in timestamp order; `apply --all` runs
the rest. `apply --down` reverts the most recently applied one.

## Editing and iterating

- `migrant edit <tag>` opens `up.sql` in `$EDITOR`; add `--down` for `down.sql`.
- `migrant redo` re-runs the latest migration (down then up) so you can iterate
  on SQL you are still writing.

## Non-transactional DDL

Some statements cannot run inside a transaction (for example PostgreSQL
`CREATE INDEX CONCURRENTLY` or `ALTER TYPE ... ADD VALUE`). Put a directive at
the top of that direction's file to opt it out:

```sql
-- migrant:no-transaction
alter type mood add value 'excited';
```

See [Transactions](transactions.md) for the full rules.
