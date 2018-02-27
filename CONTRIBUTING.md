# Contributing

Thanks for contributing!


## Getting Started

- [Install rust](https://www.rust-lang.org/en-US/install.html)
- Install dependencies:
    - **SQLite**: `apt install sqlite3 libsqlite3-dev`
    - **PostgreSQL**: `apt install postgresql libpq-dev`
    - **MySQL**: See [mysql install docs](https://dev.mysql.com/doc/mysql-apt-repo-quick-guide/en/)
       - [Download the apt repo `.deb`](https://dev.mysql.com/downloads/repo/apt/)
       - `dpkg -i mysql-apt-config_<version>_all.deb`
       - `apt update`
       - `apt install mysql-server mysql-shell`
- `cargo build`


## Making Changes

- After making changes, be sure to run the tests (see below)!
- Add an entry to the CHANGELOG


## Running Tests

The integration tests assume the `Migrant.toml` configuration file is present and configured for sqlite.

```bash
cargo test
cargo test --features 'sqlite postgres mysql'
```


## Submitting Changes

Pull Requests should be made against master.
Travis CI will run the test suite on all PRs.
Remember to update the changelog!

