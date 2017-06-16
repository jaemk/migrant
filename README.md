# Migrant [![Build Status](https://travis-ci.org/jaemk/migrant.svg?branch=master)](https://travis-ci.org/jaemk/migrant) [![crates.io](https://img.shields.io/crates/v/migrant.svg)](https://crates.io/crates/migrant) [![docs](https://docs.rs/migrant/badge.svg)](https://docs.rs/migrant)
> Basic migration manager powered by [migrant_lib](https://github.com/jaemk/migrant/tree/master/migrant_lib)

Currently supports:
 * postgres
 * sqlite


### Installation

By default `migrant` will build with the `postgres` and `rusqlite` database driver libraries. Both of these require their dev libraries (`postgres`: `libpq-dev`, `sqlite`: `libsqlite3-dev`). The binary releases are built with these defaults. `migrant` can also function without these dependencies, falling back to utilizing each database's `cli` commands (`psql` & `sqlite3`)

See [releases](https://github.com/jaemk/migrant/releases) for binaries, or

```shell
# install with default features
cargo install migrant

# install with `postgres`
cargo install migrant --no-default-features --features postgresql

# install with `rusqlite`
cargo install migrant --no-default-features --features sqlite

# use cli commands for all db interaction
cargo install migrant --no-default-features 
```

### Simple Usage

`migrant init` - initialize project and create a `.migrant.toml` file (which should be `.gitignore'd`) with db info/credentials. The default migration location (relative to your `.migrant.toml`) is `migrations/`. This can be modified in your `.migrant.toml` file (`"migration_location"`). If the directory doesn't exist, it will be created the first time you create a new migration.

`migrant new initial` - generate new up & down files with the tag `initial` under the specified `migration_location`.

`migrant list` - display all available .sql files and mark those applied.

`migrant apply [--down, --all, --force, --fake]` - apply the next available migration[s].

`migrant shell` - open a repl

`migrant which-config` - display the full path of the `.migrant.toml` file being used


### Usage as a library

See [migrant_lib](https://github.com/jaemk/migrant/tree/master/migrant_lib) and [examples](https://github.com/jaemk/migrant/tree/master/migrant_lib/examples)
