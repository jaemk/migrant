# Changelog


## [Unreleased]
### Added
- Explicit & configurable `Settings` struct.
    - These are the configurable settings used by the `Config` type
      are were previously only configurable in a file
    - Migrant.toml config files can be replaced by `Settings` configured in source.
- `Config::with_settings` for initializing a `Config` from `Settings`

### Changed
- Config file renamed from `.migrant.toml` to `Migrant.toml`
    - In sqlite configs, `database_name` parameter is now `database_path`
      and can be either an absolute or relative (to the config file dir) path.
- `Config::load_file_only` renamed to `Config::from_settings_file`
- `search_for_config` renamed to `search_for_settings_file`
- Output from `Config::setup` is now only shown in debug logs (`debug!` macro)

### Removed

