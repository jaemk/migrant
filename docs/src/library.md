# Using migrant_lib

`migrant_lib` embeds migration management in your own program. The CLI is a thin
wrapper around it, so anything the CLI does can be done from Rust. Enable a
backend feature (`d-sqlite`, `d-postgres`, `d-mysql`, or `d-all`):

```toml
[dependencies]
migrant_lib = { version = "0.35", features = ["d-postgres"] }
```

## The pieces

- `Settings` describes the database connection, built with a typed builder or
  loaded from a `Migrant.toml`.
- `Config` holds the settings, the set of migrations, and a live connection. It
  is cheap to clone; clones share the same connection.
- `Migrator` applies migrations against a `Config`.

## A minimal run

```rust
use migrant_lib::{Config, Migrator, Settings, EmbeddedMigration};

# fn run() -> Result<(), Box<dyn std::error::Error>> {
let settings = Settings::configure_sqlite()
    .database_path("/abs/path/to/db.db")?
    .build()?;

let mut config = Config::with_settings(&settings);
config.setup()?; // create the __migrant_migrations table

config.use_migrations(&[
    EmbeddedMigration::with_tag("create-users")
        .up("create table users (id integer primary key, name text);")
        .down("drop table users;")
        .boxed(),
])?;

// Load applied state, then apply everything.
let config = config.reload()?;
Migrator::with_config(&config)
    .all(true)
    .apply()?;
# Ok(())
# }
```

`reload()` re-reads the applied set from the database and returns a fresh
`Config`. Call it after `setup`/`use_migrations` and after each apply cycle.

## Settings builders

- `Settings::configure_sqlite()`: `database_path(...)`, `memory()` for an
  in-memory database, `migration_location(...)`.
- `Settings::configure_postgres()`: `database_name/user/password/host/port`,
  `ssl_cert_file(...)`, `database_params(...)`.
- `Settings::configure_mysql()`: the same name/user/password/host/port and
  `database_params`.

Or load from a file: `Config::from_settings_file("Migrant.toml")`.

## The Migrator

```rust
Migrator::with_config(&config)
    .direction(migrant_lib::Direction::Up) // or Down
    .all(true)          // every remaining migration, not just the next
    .force(false)       // continue past errors
    .fake(false)        // record without running SQL
    .synchronized(true) // advisory lock for server databases (default)
    .show_output(true)
    .apply()?;
```

`synchronized` controls the advisory lock; see
[Concurrency and locking](concurrency.md). Transaction wrapping is per migration;
see [Migration types](migration-types.md) and [Transactions](transactions.md).

## In-memory SQLite

The path `:memory:` (via `Settings::configure_sqlite().memory()`) selects an
in-memory database. Its entire state lives in one connection, which the `Config`
keeps alive and shares with its clones, so migrations and later queries see the
same database. A function migration reaches it with
`ConnConfig::sqlite_connection()`.

See the [examples](https://github.com/jaemk/migrant/tree/main/migrant_lib/examples)
for complete programs.
