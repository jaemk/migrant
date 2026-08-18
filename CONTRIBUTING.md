# Contributing

Thanks for contributing!


## Getting Started

- [Install rust](https://www.rust-lang.org/en-US/install.html)
- `cargo build`

The `sqlite` feature bundles sqlite, so no system libraries are needed for it.
The `postgres` feature needs `libpq-dev` at build time on linux.


## Making Changes

- After making changes, be sure to run the tests (see below)!
- Add an entry to the CHANGELOG


## Running Tests

```bash
cargo test --features sqlite,integration_tests
```

The CLI integration tests copy the repo's `Migrant.toml` (sqlite) and `migrations/` directory
into a tempdir and run there, so each run starts from an empty database and nothing is written
to your working tree. There is no dev database to set up or reset.

`db/migrant.db` is only created if you run `migrant` against the repo yourself, and is
gitignored. It is a scratch playground: delete it any time, and `migrant setup` recreates it.
Nothing in the test suite reads it, so a stale one cannot break a run.

Library tests live in `migrant_lib/` (see its CONTRIBUTING for postgres/mysql setup).


## Submitting Changes

Pull Requests should be made against main.
GitHub Actions will run the test suite on all PRs.
Remember to update the changelog!


## Releasing

- `lib-v<version>` tags publish `migrant_lib` to crates.io
- `cli-v<version>` tags publish `migrant` to crates.io and attach release binaries to a GitHub release
- Tags must match the corresponding `Cargo.toml` version; tag `lib-v*` first when both change
