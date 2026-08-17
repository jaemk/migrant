# The CLI

`migrant` manages migrations that live under `<project-dir>/migrations/`, where
`project-dir` is the closest parent directory containing a `Migrant.toml`.
Applied migrations are tracked in a `__migrant_migrations` table. Commands are
run from anywhere inside the project; migrant searches upward for the config.

## Project setup

`migrant init [--type <sqlite|postgres|mysql>] [--location <dir>] [--default-from-env] [--no-confirm]`
: Create a `Migrant.toml`. Run interactively (without `--no-confirm`) it also
  runs `setup`. `--default-from-env` seeds every value as an `env:VAR` reference
  instead of a literal (see [Configuration](configuration.md)).

`migrant setup`
: Verify database credentials and create the `__migrant_migrations` table if it
  is missing.

`migrant which-config`
: Print the path of the active `Migrant.toml`.

`migrant connect-string`
: Print the connection string (server databases) or the database file path
  (SQLite).

## Migrations

`migrant new <tag> [--repeatable]`
: Generate a timestamped `<stamp>_<tag>/` directory with empty `up.sql` and
  `down.sql`. Tags may contain `[a-z0-9-]`. `--repeatable` instead writes only an
  `up.sql`, seeded with the `-- migrant:repeatable` directive, for a migration
  that re-runs whenever its SQL changes (see
  [Writing migrations](migrations.md)).

`migrant edit <tag> [--down]`
: Open the `up.sql` (or `down.sql` with `--down`) for a migration matching
  `<tag>` in `$EDITOR`.

`migrant list`
: List available migrations and mark those applied.

`migrant status [--format <text|json>]`
: Report every managed migration with its applied/pending state and summary
  counts. `--format text` (the default) prints a summary line plus a
  `[✓]`/`[ ]`/`[~]` row per migration; `--format json` prints the same data as
  JSON
  (`{ total, applied, pending, stale, migrations: [{ tag, applied, repeatable, stale }] }`)
  for scripting. A repeatable migration is annotated, and marked `[~]` when it is
  due to re-run. The summary `stale` count covers applied migrations that will
  re-run; migrations with no row yet are counted in `pending`.

`migrant apply [--down] [--step <N>] [--force[=<mode>]] [--fake] [--no-sync] [--rerun-repeatable] [--allow-unknown-tags] [--allow-out-of-order] [--allow-checksum-mismatch]`
: Apply every pending migration. `--down` reverts instead of applying, and
  defaults to a single migration. `--step N` limits either direction to N.
  `--force` continues past a failed migration: bare `--force` (or
  `--force=accept-failures`) records the failed migration as applied anyway, so
  it is not retried on later runs; `--force=skip-failures` leaves it unrecorded
  and retries it on the next run. `--fake` records the migration as (un)applied
  without running its SQL. `--no-sync` disables the cross-process advisory lock
  that is otherwise on by default for PostgreSQL/MySQL; use it when migrations
  are already serialized by an external mechanism. `--rerun-repeatable` re-runs
  every repeatable migration even if its SQL is unchanged. The three `--allow-*` flags
  each bypass one otherwise-fatal consistency check: an applied tag missing from
  the migration set, a migration applied out of order, and an already-applied
  migration whose SQL has changed since it was recorded.

`migrant redo [--all] [--force[=<mode>]] [--no-sync] [--rerun-repeatable] [--allow-unknown-tags] [--allow-out-of-order] [--allow-checksum-mismatch]`
: Shortcut for the latest `down` then `up`. Useful while iterating on a migration
  you are still writing. `--all` redoes every applied migration. `--no-sync`
  disables the advisory lock for both the down and up runs. Repeatable
  migrations are forward-only, so the down phase skips them and targets the most
  recent versioned migration instead; the up phase then re-runs a repeatable
  migration only if its SQL changed, or unconditionally with
  `--rerun-repeatable`. `redo` prints a note naming the repeatable migrations it
  will not revert.

## Inspect and connect

`migrant shell`
: Open a database repl. Requires the matching client on your `PATH`: `sqlite3`
  for SQLite, `psql` for PostgreSQL, and for MySQL `mysqlsh` when installed,
  falling back to the classic `mysql` client. The password is passed out of band
  (`PGPASSWORD`/`MYSQL_PWD`), never on the command line.

`migrant tui`
: Interactive terminal UI for viewing and applying migrations. Keys: `j`/`k`
  (or Down/Up) move the selection, `u` applies the next migration and `d` reverts
  the last, `a` applies all and `D` reverts all, `r` refreshes from the database,
  and `q` (or Esc / Ctrl-C) quits.

## Maintenance

`migrant self update [--no-confirm] [--quiet]`
: Replace the running binary with the latest GitHub release. Only works when the
  binary was built with the `update` feature (release binaries are; a plain
  `cargo install` is not).

`migrant self bash-completions [install [--path <path>]]`
: Generate a bash completion script. Without `install` it is written to stdout,
  so you can redirect it yourself. With `install` it is written to a file
  (default `/etc/bash_completion.d/migrant`) and the success message goes to
  stderr.

## Behavior worth knowing

- Each migration and its bookkeeping row are applied in one transaction by
  default. See [Transactions](transactions.md) and the
  `-- migrant:no-transaction` directive for DDL that cannot run in a transaction.
- Runs against PostgreSQL/MySQL take an advisory lock so concurrent `migrant`
  processes serialize. See [Concurrency and locking](concurrency.md).
