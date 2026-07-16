# Install

The `migrant` CLI builds without any database features by default, so pick at
least one of `postgres` / `sqlite` / `mysql`. The `update` feature enables
`migrant self update`, and `vendored-openssl` statically links OpenSSL for
portable (musl) builds.

## Binary releases

Pre-built binaries for Linux (gnu and musl), macOS (x86_64 and aarch64), and
Windows are attached to each [GitHub release](https://github.com/jaemk/migrant/releases).
Release binaries are built with all features, so they can self-update:

```sh
migrant self update
```

## cargo

```sh
# with postgres
cargo install migrant --features postgres

# with bundled sqlite
cargo install migrant --features sqlite

# everything, including self-update
cargo install migrant --features 'postgres sqlite mysql update'
```

Note that a `cargo install` without the `update` feature cannot self-update, and
the `postgres` driver needs its dev library (`libpq-dev`). The `sqlite` feature
bundles SQLite.

## Docker

An image with the binary installed is published as `jaemk/migrant:latest`:

```sh
docker run --rm jaemk/migrant:latest migrant --help
```

## As a library

Add `migrant_lib` to your project and enable a backend feature. See
[Using migrant_lib](library.md).

```toml
[dependencies]
migrant_lib = { version = "1.0.0-rc.1", features = ["postgres"] }
```

Next: the [Quickstart](quickstart.md).
