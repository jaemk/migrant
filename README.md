# Migrant [![Build Status](https://travis-ci.org/jaemk/migrant.svg?branch=master)](https://travis-ci.org/jaemk/migrant) [![crates.io](https://img.shields.io/crates/v/migrant.svg)](https://crates.io/crates/migrant)

> Basic migration manager

Currently supports:
 * postgres
 * sqlite


### Installation

```shell
cargo install migrant
```

### Usage

`migrant --init` - initialize project and create a `.migrant` file (which should be `.gitignore'd`) with db credentials. The default migration location is `resources/migrations`. This can be modified in your `.migrant` file (`"migration_folder"`). If the directory doesn't exist, it will be created the first time you run `migrant --new <tag>`.

`migrant --new initial` - generate new up & down files with the tag `initial` under the specified `migration_folder`.

`migrant --list` - display all available .sql files and mark those applied.

`migrant --up [--force, --fake]` - apply the next available migration.

`migrant --down [--force, --fake]` - apply the down file of the most recent migration.

`migrant --shell` - open a repl

```
$ migrant --help

  Migrant 0.1.0
  James K. <james.kominick@gmail.com>
  Postgres migration manager

  USAGE:
      migrant [FLAGS] [OPTIONS]

  FLAGS:
      -d, --down       Moves down (applies .down.sql) one migration
          --fake       Updates the .meta file as if the specified migration was applied
          --force      Applies the migration and treats it as if it were successful
      -h, --help       Prints help information
          --init       Initialize project
      -l, --list       List status of applied and available migrations
      -s, --shell      Open a repl connection
      -u, --up         Moves up (applies .up.sql) one migration
      -V, --version    Prints version information

  OPTIONS:
      -n, --new <MIGRATION_TAG>    Creates a new migrations folder with up&down templates
```
