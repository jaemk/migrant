# Changelog

## [0.15.0]
### Added
- `migrant tui` subcommand: interactive terminal UI for viewing and applying migrations
- GitHub Actions CI and release workflows. Releases are triggered by tags:
  `lib-v*` publishes `migrant_lib`, `cli-v*` publishes `migrant` and builds release binaries
- `vendored-openssl` feature for static (musl) builds

### Changed
- Update `migrant_lib` to 0.35
- Port CLI from clap 2 to clap 4, preserving the existing interface
- `self update` now resolves `cli-v*` release tags
- Replace `dotenv` with `dotenvy`, `error-chain` with plain error types
- Update to edition 2021

### Removed
- Travis CI

----

## [0.12.0]
### Added

### Changed
- Update `migrant_lib`

### Removed

----

## [0.11.4]
### Added
- Integration tests

### Changed
- Crate / program description

### Removed

----

## [0.11.3]
### Added

### Changed
- Update migrant_lib - improves invalid tag error messages

### Removed

----

## [0.11.2]
### Added

### Changed
- Update `migrant_lib`

### Removed

----

## [0.11.1]
### Added

### Changed
- Update `migrant_lib`

### Removed

----

## [0.11.0]
### Added
- MySQL support!

### Changed
- Update database feature flags to be more consistent
    - `postgres, sqlite, mysql`
- Update crate keywords

### Removed

----

## [0.10.4]
### Added

### Changed
- Update deps (migrant_lib postgres default port fix)

### Removed

----

## [0.10.3]
### Added

### Changed
- For the `bash-completions` subcommand, write the success message to stderr so the
  stdout of the command can be redirected to a file
- Update deps
- Cleanup
- Update cargo excluded items

### Removed

----

## [0.10.2]
### Added

### Changed
- Update deps
- Update readme

### Removed

----

## [0.10.1]
### Added

### Changed
- Fix 0.10.0 changelog entry
- Update config file name in README and `help`

### Removed

----

## [0.10.0]
### Added

### Changed
- Config file renamed from `.migrant.toml` to `Migrant.toml`
    - In sqlite configs, `database_name` parameter is now `database_path` and can be either an absolute
      or relative (to the config file dir) path.
    - Config file must be renamed (and `database_name` changed to `database_path`) or re-initialized.

### Removed

----

## [0.9.11]
### Added

### Changed
- Update dependencies

### Removed

