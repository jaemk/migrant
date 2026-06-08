# Contributing

Thanks for contributing!


## Getting Started

- [Install rust](https://www.rust-lang.org/en-US/install.html)
- Install database dependencies (linux):
    - **PostgreSQL**: `apt install postgresql libpq-dev`
    - **SQLite**: `apt install sqlite3 libsqlite3-dev`
    - **MySQL**: See [mysql install docs](https://dev.mysql.com/doc/mysql-apt-repo-quick-guide/en/)
       - [Download the apt repo `.deb`](https://dev.mysql.com/downloads/repo/apt/)
       - `dpkg -i mysql-apt-config_<version>_all.deb`
       - `apt update`
       - `apt install mysql-server mysql-shell`
- `cargo build`


## Making Changes

- Please be mindful of the feature gates used to implement functionality with and without database connection crates.
- After making changes, be sure to run the tests (see below)!
- This crate makes use of [`cargo-readme`](https://github.com/livioribeiro/cargo-readme) (`cargo install cargo-readme`)
  to generate the `README.md` from the crate level documentation in `src/lib.rs`.
  This means `README.md` should never be modified by hand.
  Changes should be made to the crate documentation in `src/lib.rs` and the `readme.sh` script run.


## Running Tests

The `test.sh` script exists to handle setup and tear-down of testing databases before running library tests,
as well as ensuring the tests are run with and without the various feature flags.
Note: Some commands in this script will ask for sudo access for managing a postgres test database,
and your `mysql` root password must be provided when running locally.

```bash
./test.sh <mysql-root-pass>
```


## Submitting Changes

Pull Requests should be made against master.
Travis CI will run the test suite on all PRs.
Remember to update the changelog!

