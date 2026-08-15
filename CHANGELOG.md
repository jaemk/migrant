# Changelog

## [Unreleased]
### Added
- `apply` accepts `--step N` to apply exactly N migrations, in either direction
- `apply` and `redo` accept `--allow-unknown-tags` to permit a run when the database has an
  applied tag not present in the defined migration set, and `--allow-out-of-order` to permit
  applying migrations out of their defined order. Both are off by default: an unknown applied
  tag or an out-of-order pending migration now aborts the run with an error

### Changed
- `apply` applies all pending migrations by default, instead of just the next one. Use
  `--step 1`, or `--down` (which remains single-step by default), to move one migration at a
  time
- The `__migrant_migrations` bookkeeping table is now multi-column (`id`, `tag`, `checksum`,
  `applied_at`) instead of a single `tag` column. Each applied migration now records a sha256
  checksum of its up-SQL (null for programmatic migrations) and an applied-at timestamp, and
  applied order is tracked by `id` rather than inferred from file/tag order. `redo` and
  `apply --down` now target the most-recently-applied migration by this recorded order.

  This is a one-time breaking change to the on-disk schema. If you don't need to preserve
  applied history, the simplest upgrade is to drop the existing `__migrant_migrations` table
  and let `migrant setup` recreate it. To upgrade an existing table in place instead, add the
  missing columns and backfill them (existing rows can be left with `checksum` null):

  ```sql
  -- postgres
  alter table __migrant_migrations add column id bigserial;
  alter table __migrant_migrations add column checksum text;
  alter table __migrant_migrations add column applied_at timestamptz not null default now();

  -- sqlite
  alter table __migrant_migrations add column id integer;
  alter table __migrant_migrations add column checksum text;
  alter table __migrant_migrations add column applied_at text not null default (datetime('now'));

  -- mysql
  alter table __migrant_migrations add column id bigint unsigned auto_increment unique;
  alter table __migrant_migrations add column checksum text;
  alter table __migrant_migrations add column applied_at timestamp not null default current_timestamp;
  ```

  Since applied order was not previously tracked, `id` will not necessarily reflect the
  original apply order after an in-place upgrade; reconstruct it manually (e.g. from your own
  deployment history) if order matters, or prefer the fresh-table path above.

### Removed
- `apply --all`. Applying all pending migrations is now the default; use `--step N` if you
  need to control how many migrations move

## [1.0.0-rc.2]
### Added
- `migrant status` reports every managed migration's applied/pending state with summary counts,
  as pretty text (`--format text`, the default) or JSON (`--format json`) for scripting
- `apply` and `redo` accept `--no-sync` to disable the cross-process advisory lock, for when
  migration runs are serialized by an external mechanism. On `redo` it applies to both the down
  and up runs

### Changed
- Update `migrant_lib` to 1.0.0-rc.2: owned-`self` builder chains for the settings builders and
  `Migrator`, `#[non_exhaustive]` `DbKind`/`ForceMode`, and `MigrationStatus` field accessors.
  See the migrant_lib changelog. These are source-compatible for existing chained call sites

### Removed
- `migrant_lib`'s deprecated `d-sqlite`/`d-postgres`/`d-mysql`/`d-all` feature aliases. Use
  `sqlite`/`postgres`/`mysql`/`all`

## [1.0.0-rc.1]
### Added
- `--force` takes an optional mode: bare `--force` (or `--force=accept-failures`) records a
  failed migration as applied and continues; `--force=skip-failures` continues without
  recording it, so the next run retries it
- `migrant shell` falls back to the classic `mysql` client when `mysqlsh` is not on `PATH`
- `database_port` in `Migrant.toml` accepts a TOML integer or a string

### Changed
- Update `migrant_lib` to 1.0.0-rc.1
- `apply` and `redo` no longer treat an already-up-to-date database as an error; a run with
  nothing to apply now succeeds
- Running any subcommand other than `init` without a `Migrant.toml` now errors with a
  pointer to `migrant init`, instead of starting the interactive config-creation flow
- `apply`/`redo` no longer need a manual reload before running: the migrator re-reads
  applied state itself (see migrant_lib changelog)

## [0.15.0]
### Added
- `migrant tui` subcommand: interactive terminal UI for viewing and applying migrations
- GitHub Actions CI and release workflows. Releases are triggered by tags:
  `lib-v*` publishes `migrant_lib`, `cli-v*` publishes `migrant` and builds release binaries
- `vendored-openssl` feature for static (musl) builds
- `Migrator::synchronized` (default on): serialize migration runs across processes with a
  database advisory lock (postgres `pg_advisory_lock`, mysql `GET_LOCK`); no-op on sqlite.
  A server connection is recovered in place on error rather than dropped, so the lock is held
  for the whole run, including a `force`d run continuing past a failed migration
- `EmbeddedMigration::no_transaction` / `FileMigration::no_transaction` and
  `Migratable::use_transaction(direction)` to opt a migration out of transaction
  wrapping, resolved per direction
- `-- migrant:no-transaction` SQL directive to opt a single direction out from
  the migration file itself (works for `migrant` CLI file migrations); a
  directive in the SQL takes precedence over the builder flag

### Changed
- Update `migrant_lib` to 0.35
- Each migration's SQL and its `__migrant_migrations` bookkeeping row are now applied in a
  single transaction by default, so a failed migration leaves no partial state or record.
  Migration SQL should no longer include its own `begin`/`commit`
- Port CLI from clap 2 to clap 4, preserving the existing interface
- `self update` now resolves `cli-v*` release tags
- Replace `dotenv` with `dotenvy`, `error-chain` with plain error types
- Update to edition 2021

### Removed
- Travis CI

----

## [0.12.0]
### Added

### Changed
- Update `migrant_lib`

### Removed

----

## [0.11.4]
### Added
- Integration tests

### Changed
- Crate / program description

### Removed

----

## [0.11.3]
### Added

### Changed
- Update migrant_lib - improves invalid tag error messages

### Removed

----

## [0.11.2]
### Added

### Changed
- Update `migrant_lib`

### Removed

----

## [0.11.1]
### Added

### Changed
- Update `migrant_lib`

### Removed

----

## [0.11.0]
### Added
- MySQL support!

### Changed
- Update database feature flags to be more consistent
    - `postgres, sqlite, mysql`
- Update crate keywords

### Removed

----

## [0.10.4]
### Added

### Changed
- Update deps (migrant_lib postgres default port fix)

### Removed

----

## [0.10.3]
### Added

### Changed
- For the `bash-completions` subcommand, write the success message to stderr so the
  stdout of the command can be redirected to a file
- Update deps
- Cleanup
- Update cargo excluded items

### Removed

----

## [0.10.2]
### Added

### Changed
- Update deps
- Update readme

### Removed

----

## [0.10.1]
### Added

### Changed
- Fix 0.10.0 changelog entry
- Update config file name in README and `help`

### Removed

----

## [0.10.0]
### Added

### Changed
- Config file renamed from `.migrant.toml` to `Migrant.toml`
    - In sqlite configs, `database_name` parameter is now `database_path` and can be either an absolute
      or relative (to the config file dir) path.
    - Config file must be renamed (and `database_name` changed to `database_path`) or re-initialized.

### Removed

----

## [0.9.11]
### Added

### Changed
- Update dependencies

### Removed

