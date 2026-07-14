# Concurrency and locking

When several processes run migrations against the same server database at once
(a common case: multiple app instances applying migrations on boot), they must
not race. migrant serializes them with a database advisory lock.

## What happens

For a run against PostgreSQL or MySQL, migrant takes a session-level advisory
lock that it holds for the whole run:

- PostgreSQL: `pg_advisory_lock`.
- MySQL: `GET_LOCK` (blocks until the lock is available).

A second migrator blocks until the first finishes, then proceeds. The lock is
released when the run ends, and automatically by the database if the connection
(session) drops, so a crashed migrator cannot leave a stuck lock.

After taking the lock, migrant re-reads the applied migrations. A run that waited
for a peer therefore observes the migrations that peer committed and does not
re-apply them.

SQLite has no advisory lock and no cross-process migration concurrency (a single
connection already serializes writers), so locking is a no-op there.

## Interaction with `--force`

`migrant apply --force` continues past a failed migration. On a server database a
failed statement is recovered in place (a rollback) rather than by dropping the
connection, so the session, and the advisory lock it holds, survives the error. A
`--force` run keeps holding the lock as it continues, with no window for another
migrator to interleave.

The one case where the lock is unavoidably lost mid-run is a genuinely dead
connection. If the session ends, the database has already released the lock;
migrant reconnects and continues, but that new session does not hold it.

## Library control

Runs are synchronized by default. The `Migrator::synchronized(bool)` builder
toggles it, for example when an outer mechanism already serializes migrations:

```rust
Migrator::with_config(&config)
    .synchronized(false) // opt out of the advisory lock
    .all(true)
    .apply()?;
```

See [Using migrant_lib](library.md) for the rest of the `Migrator` API.
