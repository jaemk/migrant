# Migrant [![Build Status](https://travis-ci.org/jaemk/migrant.svg?branch=master)](https://travis-ci.org/jaemk/migrant) [![crates.io](https://img.shields.io/crates/v/migrant.svg)](https://crates.io/crates/migrant)

> Basic migration manager

Currently supports:
 * postgres
 * sqlite


### Installation

```shell
cargo install migrant
```

### Simple Usage

`migrant --init` - initialize project and create a `.migrant.toml` file (which should be `.gitignore'd`) with db credentials. The default migration location is `migrations/`. This can be modified in your `.migrant.toml` file (`"migration_location"`). If the directory doesn't exist, it will be created the first time you run `migrant --new <tag>`.

`migrant --new initial` - generate new up & down files with the tag `initial` under the specified `migration_location`.

`migrant --list` - display all available .sql files and mark those applied.

`migrant --up [--all, --force, --fake]` - apply the next available migration.

`migrant --down [--all, --force, --fake]` - apply the down file of the most recent migration.

`migrant --shell` - open a repl

`migrant --which-meta` - display the full path of the .migrant.toml file being used
