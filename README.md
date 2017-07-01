# Migrant [![Build Status](https://travis-ci.org/jaemk/migrant.svg?branch=master)](https://travis-ci.org/jaemk/migrant) [![crates.io:migrant](https://img.shields.io/crates/v/migrant.svg?label=migrant)](https://crates.io/crates/migrant) [![crates.io:migrant_lib](https://img.shields.io/crates/v/migrant_lib.svg?label=migrant_lib)](https://crates.io/crates/migrant_lib) [![docs](https://docs.rs/migrant_lib/badge.svg)](https://docs.rs/migrant_lib)
> Basic migration manager powered by [`migrant_lib`](https://github.com/jaemk/migrant/tree/master/migrant_lib)

Currently supports:
 * postgres
 * sqlite


### Installation

By default `migrant` will build without any features, falling back to using each database's `cli` commands (`psql` & `sqlite3`).
The `postgres` and `rusqlite` database driver libraries can be activated with the `postgresql` and `sqlite` `features`.
Both of these drivers require their dev libraries (`postgres`: `libpq-dev`, `sqlite`: `libsqlite3-dev`).
The binary releases are built with these features.

See [releases](https://github.com/jaemk/migrant/releases) for binaries, or

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

`migrant init` - initialize project and create a `.migrant.toml` file (which should be `.gitignore'd`) with db info/credentials. The default migration location (relative to your `.migrant.toml`) is `migrations/`. This can be modified in your `.migrant.toml` file (`"migration_location"`). If the directory doesn't exist, it will be created the first time you create a new migration.

`migrant new initial` - generate new up & down files with the tag `initial` under the specified `migration_location`.

`migrant list` - display all available .sql files and mark those applied.

`migrant apply [--down, --all, --force, --fake]` - apply the next available migration[s].

`migrant shell` - open a repl

`migrant which-config` - display the full path of the `.migrant.toml` file being used


### Usage as a library

See [`migrant_lib`](https://github.com/jaemk/migrant/tree/master/migrant_lib) and [examples](https://github.com/jaemk/migrant/tree/master/migrant_lib/examples)
