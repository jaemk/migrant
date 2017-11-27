# Migrant
[![Build Status](https://travis-ci.org/jaemk/migrant.svg?branch=master)](https://travis-ci.org/jaemk/migrant)
[![crates.io:migrant](https://img.shields.io/crates/v/migrant.svg?label=migrant)](https://crates.io/crates/migrant)
[![crates.io:migrant_lib](https://img.shields.io/crates/v/migrant_lib.svg?label=migrant_lib)](https://crates.io/crates/migrant_lib)
[![docs](https://docs.rs/migrant_lib/badge.svg)](https://docs.rs/migrant_lib)

> Basic migration manager powered by [`migrant_lib`](https://github.com/jaemk/migrant/tree/master/migrant_lib)

Currently supports:
 * postgres
 * sqlite


`migrant` will manage all migrations that live under `<project-dir>/migrations/` where `project-dir` is the closest
parent path that contains a `.migrant.toml` configuration file (`..../<project-dir>/.migrant.toml`).
The default migration file location can be modified in your `.migrant.toml` file (`"migration_location"`).
If the `migration_location` directory doesn't exist, it will be created the first time you create a new migration.
`migrant` stores all applied migrations in a database table named `__migrant_migrations`


### Installation

By default `migrant` will build without any features, falling back to using each database's `cli` commands (`psql` & `sqlite3`).
The `postgres` and `rusqlite` database driver libraries can be activated with the `postgresql` and `sqlite` `features`.
Both of these drivers require their dev libraries (`postgresql`: `libpq-dev`, `sqlite`: `libsqlite3-dev`).
Self update functionality (updating to the latest GitHub release) is available behind the `update` feature.
The binary releases are built with all features.

See [releases](https://github.com/jaemk/migrant/releases) for binaries. If you've installed a binary release, you can update to the latest release via `migrant self update`.

Install from source/`crates.io`:

```shell
# install without features
# use cli commands for all db interaction
cargo install migrant

# install with `postgres`
cargo install migrant --features postgresql

# install with `rusqlite`
cargo install migrant --features sqlite

# all
cargo install migrant --features 'postgresql sqlite'
```

### Simple Usage

`migrant init [--type <database-type>, --location <project-dir>, --no-confirm]` - Initialize project by creating a `.migrant.toml` file with db info/credentials.
When run interactively (without `--no-confirm`), `setup` will be run automatically.

`migrant setup` - Verify database info/credentials and setup a `__migrant_migrations` table if missing.

`migrant new <tag>` - Generate new up & down files with the given `<tag>` under the specified `migration_location`.

`migrant edit <tag> [--down]` - Edit the `up` [or `down`] migration file with the given `<tag>`.

`migrant list` - Display all available .sql files and mark those applied.

`migrant apply [--down, --all, --force, --fake]` - Apply the next available migration[s].

`migrant shell` - Open a repl

`migrant which-config` - Display the full path of the `.migrant.toml` file being used

`migrant connect-string` - Display either the connection-string generated from config-params or the database-path for sqlite

`migrant self update` - Update to the latest version released on GitHub.

`migrant self bash-completions install [--path <path]` - Generate a bash completion script and save it to the default or specified path.


### Usage as a library

See [`migrant_lib`](https://github.com/jaemk/migrant/tree/master/migrant_lib) and
[examples](https://github.com/jaemk/migrant/tree/master/migrant_lib/examples).
`migrant` itself is just a thin wrapper around `migrant_lib`, so the full functionality of migration management
can be embedded in your actual project.


### Development

- Install dependencies: `sqlite3`, `libsqlite3-dev`, `postgresql`, `libpq-dev`
- `./test_all.sh`

