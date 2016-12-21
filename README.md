## Basic migration manager

[![Build Status](https://travis-ci.org/jaemk/migrant.svg?branch=master)](https://travis-ci.org/jaemk/migrant)
[![crates.io](https://img.shields.io/crates/v/migrant.svg)](https://crates.io/crates/migrant)


Currently supports:
 - postgres

----
Running `migrant --new new-tag` creates up & down files under `resources/migrations` with the tag `new-tag`

Migrant expects a `.migrant` file at the base of your project (be sure to `.gitignore` it). Run `migrant --init` to generate a fresh `.migrant` settings file.


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

