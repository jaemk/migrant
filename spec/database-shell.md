# Database Shell

shell subcommand opening an interactive REPL to the configured database.

## SHELL-1

`migrant shell` opens the backend's interactive client connected to the configured
database: `sqlite3`, `psql`, or for mysql the MySQL Shell (`mysqlsh`) when it is on
`$PATH` and the classic `mysql` client otherwise. Server-database passwords are passed
via environment (`PGPASSWORD`/`MYSQL_PWD`), never in argv.

Coverage: unit tests in `migrant_lib/src/ops.rs` (all backends, both mysql clients,
in-memory sqlite error).
