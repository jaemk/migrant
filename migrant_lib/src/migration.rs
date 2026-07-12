/*!
Embedded / programmable migrations
*/
use std::borrow::Cow;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::config::Config;
use crate::connection::ConnConfig;
use crate::errors::*;
use crate::macros::bail;
use crate::migratable::Migratable;
use crate::migrator::Direction;
use crate::DT_FORMAT;

/// Define a migration that uses SQL statements saved in files.
///
/// *Note:* Files defined in this migration must be present at run-time.
/// File paths can be absolute or relative. Relative file paths are relative
/// to the directory from which the program is run.
///
/// *Note:* SQL statements are batch executed as is. If you want your migration
/// to happen atomically in a transaction you should manually wrap your statements
/// in a transaction (`begin transaction; ... commit;`).
#[derive(Clone, Debug)]
pub struct FileMigration {
    /// Migration tag
    pub tag: String,
    /// Path to an `up` migration file
    pub up: Option<PathBuf>,
    /// Path to a `down` migration file
    pub down: Option<PathBuf>,
    pub(crate) stamp: Option<DateTime<Utc>>,
}

impl FileMigration {
    /// Create a new `FileMigration` with a given tag
    pub fn with_tag(tag: &str) -> Self {
        Self {
            tag: tag.to_owned(),
            up: None,
            down: None,
            stamp: None,
        }
    }

    fn check_path(path: &Path) -> Result<()> {
        if !path.exists() {
            bail!(MigrationNotFound, "Migration file not found: {:?}", path)
        }
        Ok(())
    }

    /// Define the file to use for running `up` migrations.
    ///
    /// *Note:* Files defined in this migration must be present at run-time.
    /// File paths can be absolute or relative. Relative file paths are relative
    /// to the directory from which the program is run.
    pub fn up<T: AsRef<Path>>(&mut self, up_file: T) -> Result<&mut Self> {
        let path = up_file.as_ref();
        Self::check_path(path)?;
        self.up = Some(path.to_owned());
        Ok(self)
    }

    /// Define the file to use for running `down` migrations.
    ///
    /// *Note:* Files defined in this migration must be present at run-time.
    /// File paths can be absolute or relative. Relative file paths are relative
    /// to the directory from which the program is run.
    pub fn down<T: AsRef<Path>>(&mut self, down_file: T) -> Result<&mut Self> {
        let path = down_file.as_ref();
        Self::check_path(path)?;
        self.down = Some(path.to_owned());
        Ok(self)
    }

    /// Box this migration up so it can be stored with other migrations
    pub fn boxed(&self) -> Box<dyn Migratable> {
        Box::new(self.clone())
    }

    fn apply_file(
        config: &Config,
        file: &Option<PathBuf>,
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        if let Some(file) = file {
            let sql = std::fs::read_to_string(file)?;
            config.execute_sql(&sql)?;
        }
        Ok(())
    }
}

impl Migratable for FileMigration {
    fn apply_up(&self, config: &Config) -> std::result::Result<(), Box<dyn std::error::Error>> {
        Self::apply_file(config, &self.up)
    }

    fn apply_down(&self, config: &Config) -> std::result::Result<(), Box<dyn std::error::Error>> {
        Self::apply_file(config, &self.down)
    }

    fn tag(&self) -> String {
        match self.stamp.as_ref() {
            Some(dt) => format!("{}_{}", dt.format(DT_FORMAT), self.tag),
            None => self.tag.to_owned(),
        }
    }

    fn description(&self, direction: &Direction) -> String {
        let file = match direction {
            Direction::Up => &self.up,
            Direction::Down => &self.down,
        };
        file.as_ref()
            .map(|p| format!("{:?}", p))
            .unwrap_or_else(|| self.tag())
    }
}

/// Define an embedded migration
///
/// SQL statements provided to `EmbeddedMigration` will be embedded in
/// the executable so no files are required at run-time. The
/// standard [`include_str!`](https://doc.rust-lang.org/std/macro.include_str.html) macro
/// can be used to embed contents of files, or a string literal can be provided.
///
/// *Note:* SQL statements are batch executed as is. If you want your migration
/// to happen atomically in a transaction you should manually wrap your statements
/// in a transaction (`begin transaction; ... commit;`).
///
/// A database feature (`d-postgres` / `d-sqlite` / `d-mysql`) is required to
/// apply this type of migration.
///
/// # Example
///
/// ```rust,no_run
/// # use migrant_lib::EmbeddedMigration;
/// # fn main() { run().unwrap(); }
/// # fn run() -> Result<(), Box<dyn std::error::Error>> {
/// EmbeddedMigration::with_tag("create-users-table")
///     .up(include_str!("../migrations/embedded/create_users_table/up.sql"))
///     .down(include_str!("../migrations/embedded/create_users_table/down.sql"));
/// # Ok(())
/// # }
/// ```
///
/// ```rust,no_run
/// # use migrant_lib::EmbeddedMigration;
/// # fn main() { run().unwrap(); }
/// # fn run() -> Result<(), Box<dyn std::error::Error>> {
/// EmbeddedMigration::with_tag("create-places-table")
///     .up("create table places(id integer);")
///     .down("drop table places;");
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct EmbeddedMigration {
    /// Migration tag
    pub tag: String,
    /// Statements to run for `up` migrations
    pub up: Option<Cow<'static, str>>,
    /// Statements to run for `down` migrations
    pub down: Option<Cow<'static, str>>,
}

impl EmbeddedMigration {
    /// Create a new `EmbeddedMigration` with the given tag
    pub fn with_tag(tag: &str) -> Self {
        Self {
            tag: tag.to_owned(),
            up: None,
            down: None,
        }
    }

    /// `&'static str` or `String` of statements to use for `up` migrations
    pub fn up<T: Into<Cow<'static, str>>>(&mut self, stmt: T) -> &mut Self {
        self.up = Some(stmt.into());
        self
    }

    /// `&'static str` or `String` of statements to use for `down` migrations
    pub fn down<T: Into<Cow<'static, str>>>(&mut self, stmt: T) -> &mut Self {
        self.down = Some(stmt.into());
        self
    }

    /// Box this migration up so it can be stored with other migrations
    pub fn boxed(&self) -> Box<dyn Migratable> {
        Box::new(self.clone())
    }
}

impl Migratable for EmbeddedMigration {
    fn apply_up(&self, config: &Config) -> std::result::Result<(), Box<dyn std::error::Error>> {
        if let Some(ref up) = self.up {
            config.execute_sql(up.as_ref())?;
        }
        Ok(())
    }

    fn apply_down(&self, config: &Config) -> std::result::Result<(), Box<dyn std::error::Error>> {
        if let Some(ref down) = self.down {
            config.execute_sql(down.as_ref())?;
        }
        Ok(())
    }

    fn tag(&self) -> String {
        self.tag.to_owned()
    }

    fn description(&self, _: &Direction) -> String {
        self.tag()
    }
}

/// No-op to use with `FnMigration`
pub fn noop(_: ConnConfig) -> std::result::Result<(), Box<dyn std::error::Error>> {
    Ok(())
}

/// Define a programmable migration
///
/// `FnMigration`s are provided a `ConnConfig` instance and given free rein to do as they please.
///
/// Note, both an `up` and `down` function must be provided. There is a noop function available
/// (`migrant_lib::migration::noop`) for convenience.
///
/// # Example
///
/// ```rust,no_run
/// # use migrant_lib::{FnMigration, ConnConfig};
/// # fn main() { run().unwrap(); }
/// # fn run() -> Result<(), Box<dyn std::error::Error>> {
/// fn add_data(config: ConnConfig) -> Result<(), Box<dyn std::error::Error>> {
///     // do stuff...
///     Ok(())
/// }
///
/// FnMigration::with_tag("add-user-data")
///     .up(add_data)
///     .down(migrant_lib::migration::noop);
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct FnMigration<T, U> {
    /// Migration tag
    pub tag: String,
    /// Function to run for `up` migrations
    pub up: Option<T>,
    /// Function to run for `down` migrations
    pub down: Option<U>,
}

impl<T, U> FnMigration<T, U>
where
    T: 'static + Clone + Fn(ConnConfig) -> std::result::Result<(), Box<dyn std::error::Error>>,
    U: 'static + Clone + Fn(ConnConfig) -> std::result::Result<(), Box<dyn std::error::Error>>,
{
    /// Create a new `FnMigration` with the given tag
    pub fn with_tag(tag: &str) -> Self {
        Self {
            tag: tag.to_owned(),
            up: None,
            down: None,
        }
    }

    /// Function to use for `up` migrations
    ///
    /// Function must have the signature `fn(ConnConfig) -> Result<(), Box<dyn std::error::Error>>`.
    pub fn up(&mut self, f_up: T) -> &mut Self {
        self.up = Some(f_up);
        self
    }

    /// Function to use for `down` migrations
    ///
    /// Function must have the signature `fn(ConnConfig) -> Result<(), Box<dyn std::error::Error>>`.
    pub fn down(&mut self, f_down: U) -> &mut Self {
        self.down = Some(f_down);
        self
    }

    /// Box this migration up so it can be stored with other migrations
    pub fn boxed(&self) -> Box<dyn Migratable> {
        Box::new(self.clone())
    }
}

impl<T, U> Migratable for FnMigration<T, U>
where
    T: 'static + Clone + Fn(ConnConfig) -> std::result::Result<(), Box<dyn std::error::Error>>,
    U: 'static + Clone + Fn(ConnConfig) -> std::result::Result<(), Box<dyn std::error::Error>>,
{
    fn apply_up(&self, config: &Config) -> std::result::Result<(), Box<dyn std::error::Error>> {
        if let Some(ref up) = self.up {
            up(ConnConfig::new(config))?;
        }
        Ok(())
    }

    fn apply_down(&self, config: &Config) -> std::result::Result<(), Box<dyn std::error::Error>> {
        if let Some(ref down) = self.down {
            down(ConnConfig::new(config))?;
        }
        Ok(())
    }

    fn tag(&self) -> String {
        self.tag.to_owned()
    }

    fn description(&self, _: &Direction) -> String {
        self.tag()
    }
}
