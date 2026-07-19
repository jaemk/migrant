# Migrant
[![Build Status](https://github.com/jaemk/migrant/actions/workflows/ci.yml/badge.svg)](https://github.com/jaemk/migrant/actions)
[![crates.io:migrant](https://img.shields.io/crates/v/migrant.svg?label=migrant)](https://crates.io/crates/migrant)

> Basic migration manager powered by [`migrant_lib`](./migrant_lib/)


**Supported databases/features:**

| Feature            |    Backend                                          |
|--------------------|------------------------------------------------------|
| `postgres`         | Enable postgres connectivity                        |
| `sqlite`           | Enable sqlite connectivity                          |
| `mysql`            | Enable mysql connectivity                           |
| `update`           | Enable `self-update` functionality                  |
| `vendored-openssl` | Statically vendor OpenSSL (for static/musl builds)  |


`migrant` will manage all migrations that live under `<project-dir>/migrations/`, where `project-dir` is the closest
parent path that contains a `Migrant.toml` configuration file (`..../<project-dir>/Migrant.toml`).
The default migration file location can be modified in your `Migrant.toml` file (`"migration_location"`).
If the `migration_location` directory doesn't exist, it will be created the first time you create a new migration.
`migrant` stores all applied migrations in a database table named `__migrant_migrations`.

*Note:* Configuration values prefixed with `env:` in your `Migrant.toml` will be sourced from environment variables.
For example, `database_user = "env:DB_USER"` will use the value of the environment variable `DB_USER`.
If a `.env` file exists, it will be "sourced" automatically before your `Migrant.toml` is loaded.

*Note:* Each migration and its `__migrant_migrations` bookkeeping row are applied in a single transaction
by default, so do not wrap your SQL in your own `begin`/`commit`. For DDL a backend cannot run in a
transaction (e.g. postgres `create index concurrently` or `alter type ... add value`), opt a direction out
with a `-- migrant:no-transaction` comment line in that migration file. See the
[docs](https://jaemk.github.io/migrant/).


### Installation

**Binary releases:**

See [releases](https://github.com/jaemk/migrant/releases) for binaries. If you've already installed a binary release, you can update to the latest release via `migrant self update`.
Note: `migrant self update` only works when the binary was built with the `update` feature; binary releases include it, but `cargo install migrant --features postgres` (for example) does not.

**Building from source:**

By default `migrant` builds without any database features, so at least one of `postgres` / `sqlite` / `mysql`
is required for a useful binary.
Some drivers require their dev libraries (`postgresql`: `libpq-dev`); the `sqlite` feature bundles sqlite.
[Self update](https://github.com/jaemk/self_update) functionality (updating to the latest GitHub release) is available behind the `update` feature.
The binary releases are built with all features.

**Building from source (`crates.io`):**

```shell
# install with `postgres`
cargo install migrant --features postgres

# install with bundled sqlite
cargo install migrant --features sqlite

# all
cargo install migrant --features 'postgres sqlite mysql update'
```

### Simple Usage

`migrant init [--type <database-type>, --location <project-dir>, --default-from-env, --no-confirm]` - Initialize project by creating a `Migrant.toml` file with db info/credentials.
When run interactively (without `--no-confirm`), `setup` will be run automatically.
`--default-from-env` seeds all settings values as `env:VAR` references instead of literal values.

`migrant setup` - Verify database info/credentials and setup a `__migrant_migrations` table if missing.

`migrant new <tag>` - Generate new up & down files with the given `<tag>` under the specified `migration_location`.

`migrant edit <tag> [--down]` - Edit the `up` [or `down`] migration file with the given `<tag>`.

`migrant list` - Display all available .sql files and mark those applied.

`migrant status [--format <text|json>]` - Report every managed migration's applied/pending state with summary counts, as pretty text (default) or JSON.

`migrant apply [--down, --all, --force, --fake, --no-sync]` - Apply the next available migration[s].

`migrant redo [--all, --force, --no-sync]` - Re-apply the latest migration (down then up).

`migrant tui` - Open an interactive terminal UI for viewing and applying migrations.

`migrant shell` - Open a repl

`migrant which-config` - Display the full path of the `Migrant.toml` file being used

`migrant connect-string` - Display either the connection-string generated from config-params or the database-path for sqlite

`migrant self update` - Update to the latest version released on GitHub.

`migrant self bash-completions install [--path <path>]` - Generate a bash completion script and save it to the default or specified path.


### Usage as a library

See [`migrant_lib`](./migrant_lib/) and
[examples](./migrant_lib/examples/).
`migrant` itself is just a thin wrapper around `migrant_lib`, so the full functionality of migration management
can be embedded in your actual project.


### Releases

The library and the CLI are versioned and released separately, triggered by pushing a tag:

- `lib-v<version>` publishes `migrant_lib` to crates.io. The tag must match the version in `migrant_lib/Cargo.toml`.
- `cli-v<version>` publishes `migrant` to crates.io and builds release binaries
  (linux gnu/musl, macos x86_64/aarch64, windows) attached to a GitHub release.
  The tag must match the version in `Cargo.toml`.

When both change, tag `lib-v*` first so the published CLI can resolve its `migrant_lib` dependency.


### Development

See [CONTRIBUTING](https://github.com/jaemk/migrant/blob/main/CONTRIBUTING.md)


### Docker

An image with the binary installed is available at `jaemk/migrant:latest`
