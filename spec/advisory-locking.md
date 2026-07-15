# Advisory Locking

Migration runs serialize across processes using a database advisory lock.

## LOCK-1

`Migrator::synchronized(bool)` (default `true`) makes a run take a database
advisory lock for its whole duration, so concurrent migrators -- for example
several application instances booting at once -- apply migrations one at a time
instead of racing.

## LOCK-2

For server databases the lock is session-level and held on the run's live
connection: Postgres `pg_advisory_lock`, MySQL `GET_LOCK` (blocking until the
lock is available). It is released when the run finishes, and automatically by
the database if the connection (session) drops mid-run, so a crashed migrator
cannot leave a stuck lock.

## LOCK-3

The lock is acquired before applied migrations are re-read, so a run that waited
for a peer observes the peer's committed migrations under the lock and does not
re-apply them.

## LOCK-4

Sqlite has no advisory lock and no cross-process migration concurrency (a single
connection already serializes writers), so `synchronized` is a no-op there.

## LOCK-5

A mid-run error recovers the run's connection in place (a rollback) rather than
dropping it, so the session -- and the advisory lock it holds -- is not released
mid-run. A `force`d run therefore keeps holding the lock as it continues past a
failed migration, with no window for another migrator to interleave. Only a
genuinely dead connection (its rollback also fails) is dropped.

## LOCK-6

A synchronized run never continues after its locked session is gone. If the
connection had to be dropped and re-established mid-run (LOCK-5's dead-connection
case), the run aborts with an error before applying or recording anything on the
new, unlocked session. The applied-state re-read at the start of a run does not
re-read the settings file or swap the connection (see MIGRATOR-1), so the run
cannot silently migrate over a different connection than the one it locked.

Coverage: `migrant_lib/src/drivers/pg.rs`, `mysql.rs` (`advisory_lock`, gated on
the test connection strings and run via `test.sh`); both drivers' `advisory_lock`
tests cover exclusivity and lock survival across an in-transaction error;
end-to-end apply/unapply, the `force`-past-failure phase, and the
`synchronized(false)` skip phase in `migrant_lib/tests/server_dbs.rs`.
