# Migration types

`Config::use_migrations(&[...])` registers an explicit, ordered list of boxed
`Migratable` values. Three types are built in.

## FileMigration

Runs `up`/`down` SQL loaded from files at runtime.

```rust
use migrant_lib::FileMigration;

# fn run() -> Result<(), Box<dyn std::error::Error>> {
FileMigration::with_tag("create-users")
    .up("migrations/create_users/up.sql")?
    .down("migrations/create_users/down.sql")?
    .boxed();
# Ok(())
# }
```

The files must exist at runtime; relative paths resolve from the working
directory.

## EmbeddedMigration

Runs `up`/`down` SQL from strings compiled into the binary, so no files are
needed at runtime. `include_str!` embeds a file's contents.

```rust
use migrant_lib::EmbeddedMigration;

# fn run() {
EmbeddedMigration::with_tag("create-places")
    .up(include_str!("../migrations/create_places/up.sql"))
    .down("drop table places;")
    .boxed();
# }
```

## FnMigration

Runs arbitrary Rust with the signature
`fn(ConnConfig) -> Result<(), Box<dyn std::error::Error>>`. Use it for data
migrations or anything SQL alone cannot express.

```rust
use migrant_lib::{FnMigration, ConnConfig};

fn seed(conn: ConnConfig) -> Result<(), Box<dyn std::error::Error>> {
    // open a connection from conn.connect_string()? / conn.sqlite_connection()?
    Ok(())
}

# fn run() {
FnMigration::with_tag("seed-users")
    .up(seed)
    .down(migrant_lib::migration::noop)
    .boxed();
# }
```

## Transactions per migration

`Migratable::use_transaction(direction)` decides whether migrant wraps a
migration in a transaction for that direction. The default is `true`.

- `FileMigration` and `EmbeddedMigration` are wrapped by default. Opt out with
  `no_transaction()` (both directions) or a `-- migrant:no-transaction` directive
  in the SQL (per direction). A directive in the SQL takes precedence over the
  builder flag.
- `FnMigration` runs arbitrary code and may open its own connections, so it is
  never wrapped.

See [Transactions](transactions.md) for the directive rules and backend
behavior.

## Tags and CLI compatibility

Tags must be unique and contain `[a-z0-9-]`. To interoperate with the `migrant`
CLI (whose file migrations are timestamp-prefixed), call
`Config::use_cli_compatible_tags(true)` before `use_migrations`/`reload`, which
requires tags of the form `[0-9]{14}_[a-z0-9-]+`.
