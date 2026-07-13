# Contributing

Thanks for contributing!


## Getting Started

- [Install rust](https://www.rust-lang.org/en-US/install.html)
- `cargo build`

No database services or dev libraries are needed for the default build.
Sqlite tests use a bundled sqlite; postgres/mysql tests run against
disposable docker containers (see below).


## Making Changes

- Please be mindful of the feature gates used to implement functionality with and without database connection crates.
- After making changes, be sure to run the tests (see below)!
- This crate makes use of [`cargo-readme`](https://github.com/livioribeiro/cargo-readme) (`cargo install cargo-readme`)
  to generate the `README.md` from the crate level documentation in `src/lib.rs`.
  This means `README.md` should never be modified by hand.
  Changes should be made to the crate documentation in `src/lib.rs` and the `readme.sh` script run.
- Add an entry to the CHANGELOG


## Running Tests

```bash
# no database features
cargo test

# sqlite only (bundled, no services needed)
cargo test --features d-sqlite

# everything, using throwaway docker databases for postgres/mysql
./test.sh
```

Postgres and mysql tests are skipped unless `POSTGRES_TEST_CONN_STR` /
`MYSQL_TEST_CONN_STR` are set; `test.sh` starts docker containers and sets both.


## Submitting Changes

Pull Requests should be made against main.
GitHub Actions will run the test suite on all PRs.
Remember to update the changelog!
