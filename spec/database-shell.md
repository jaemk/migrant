# Database Shell

shell subcommand opening an interactive REPL to the configured database.

## SHELL-1

`migrant shell` opens the backend's interactive client (`sqlite3`, `psql`, or `mysql`)
connected to the configured database.

Coverage: unit tests in `migrant_lib/src/ops.rs`.
