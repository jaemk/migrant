# Writing migrations

A CLI migration is a directory holding an `up.sql` and, optionally, a
`down.sql`, named with a timestamp and a tag:

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

`down.sql` is optional. A migration with no down file is a no-op in the down
direction: reverting it removes its tracking-table row without running any SQL.

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

`apply` runs every pending migration in timestamp order; `apply --step N` limits
a run to N. `apply --down` reverts the most recently applied one.

## Editing and iterating

- `migrant edit <tag>` opens `up.sql` in `$EDITOR`; add `--down` for `down.sql`.
- `migrant redo` re-runs the latest migration (down then up) so you can iterate
  on SQL you are still writing.

Editing a migration that has already been applied is drift: the next run aborts
with a checksum mismatch rather than silently building on changed SQL. Either
revert the edit and write a new migration, or pass `--allow-checksum-mismatch`.
Repeatable migrations invert this, see below.

## Repeatable migrations

A repeatable migration re-runs whenever its `up.sql` changes, instead of applying
exactly once. Use them for idempotent data work (seeding, backfills, refreshing
views) rather than schema versioning.

`migrant new --repeatable <tag>` creates one: an `up.sql` carrying the directive,
and no `down.sql`.

```sql
-- migrant:repeatable
insert into roles (name) values ('admin') on conflict do nothing;
```

The rules:

- They run after every pending versioned migration in a run, in timestamp order
  among themselves, and at most once per run.
- A checksum change is the signal to re-run, not drift, so editing the file is
  how you make it run again. An unchanged file is skipped.
- They keep one row in the tracking table, updated in place.
- They are forward-only: they must not have a `down.sql`, and `apply --down`
  never reverts them. `redo` reverts and re-applies the most recent *versioned*
  migration, which may not be the one you just edited; its up phase then re-runs
  a repeatable migration only if its SQL changed, like any other run. `redo`
  prints a note when it skips one.
- To run one whose SQL has not changed, pass `--rerun-repeatable` to `apply` or
  `redo`. It re-runs every repeatable migration, still after the versioned ones
  and still at most once per run.

`migrant status` marks one that is due to re-run:

```
Migration status: 2 applied, 0 pending, 1 stale (2 total)
  [✓] 20260713094500_create-roles
  [~] 20260714101500_seed-roles  (repeatable, will re-run)
```

## Non-transactional DDL

Some statements cannot run inside a transaction (for example PostgreSQL
`CREATE INDEX CONCURRENTLY` or `ALTER TYPE ... ADD VALUE`). Put a directive at
the top of that direction's file to opt it out:

```sql
-- migrant:no-transaction
alter type mood add value 'excited';
```

See [Transactions](transactions.md) for the full rules.
