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

/// SQL comment directive that opts a single migration direction out of the
/// migrator's automatic transaction wrapping.
///
/// Placed on its own `--` comment line in a direction's SQL (an `up.sql` /
/// `down.sql` file, or the embedded string for that direction), e.g:
///
/// ```sql
/// -- migrant:no-transaction
/// alter type mood add value 'excited';
/// ```
pub(crate) const NO_TRANSACTION_DIRECTIVE: &str = "migrant:no-transaction";

/// Return `true` if `sql` carries the [`NO_TRANSACTION_DIRECTIVE`] on a comment
/// line. It is matched case-insensitively as the first token of a `--` line
/// comment, so a trailing explanation is allowed
/// (`-- migrant:no-transaction (enum add)`).
pub(crate) fn sql_opts_out_of_transaction(sql: &str) -> bool {
    sql.lines().any(|line| {
        matches!(
            line.trim()
                .strip_prefix("--")
                .and_then(|rest| rest.split_whitespace().next()),
            Some(token) if token.eq_ignore_ascii_case(NO_TRANSACTION_DIRECTIVE)
        )
    })
}

/// Whether the migration file at `path` declares the no-transaction directive.
/// A missing or unreadable file is treated as not opting out; the subsequent
/// apply surfaces any real read error.
fn file_opts_out_of_transaction(path: &Option<PathBuf>) -> bool {
    match path {
        Some(p) => std::fs::read_to_string(p)
            .map(|sql| sql_opts_out_of_transaction(&sql))
            .unwrap_or(false),
        None => false,
    }
}

/// Define a migration that uses SQL statements saved in files.
///
/// *Note:* Files defined in this migration must be present at run-time.
/// File paths can be absolute or relative. Relative file paths are relative
/// to the directory from which the program is run.
///
/// *Note:* By default the migrator wraps this migration's SQL and its
/// bookkeeping row in a single transaction, so do **not** add your own
/// `begin`/`commit` to the SQL. To opt a direction out (for statements a backend
/// refuses to run in a transaction block, e.g. a Postgres `CREATE INDEX
/// CONCURRENTLY` or `ALTER TYPE ... ADD VALUE`), put the directive
/// `-- migrant:no-transaction` on a comment line in that direction's SQL file,
/// or call [`no_transaction`](FileMigration::no_transaction) to opt out both
/// directions. A file directive takes precedence over the builder flag, so it
/// works for migrations discovered from disk by the `migrant` CLI.
#[derive(Clone, Debug)]
pub struct FileMigration {
    /// Migration tag
    pub tag: String,
    /// Path to an `up` migration file
    pub up: Option<PathBuf>,
    /// Path to a `down` migration file
    pub down: Option<PathBuf>,
    pub(crate) stamp: Option<DateTime<Utc>>,
    pub(crate) no_transaction: bool,
}

impl FileMigration {
    /// Create a new `FileMigration` with a given tag
    pub fn with_tag(tag: &str) -> Self {
        Self {
            tag: tag.to_owned(),
            up: None,
            down: None,
            stamp: None,
            no_transaction: false,
        }
    }

    /// Opt both of this migration's directions out of the migrator's automatic
    /// transaction wrapping.
    ///
    /// Use this for statements a backend refuses to run inside a transaction
    /// block, such as Postgres `CREATE INDEX CONCURRENTLY`. For per-direction
    /// control, put the `-- migrant:no-transaction` directive on a comment line
    /// in the relevant SQL file instead; a file directive takes precedence over
    /// this flag. When opted out, that direction's SQL and its bookkeeping row
    /// are no longer applied atomically.
    pub fn no_transaction(&mut self) -> &mut Self {
        self.no_transaction = true;
        self
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

    fn use_transaction(&self, direction: Direction) -> bool {
        let file = match direction {
            Direction::Up => &self.up,
            Direction::Down => &self.down,
        };
        // A directive in the migration file takes precedence over the
        // builder-level `no_transaction` flag.
        if file_opts_out_of_transaction(file) {
            return false;
        }
        !self.no_transaction
    }
}

/// Define an embedded migration
///
/// SQL statements provided to `EmbeddedMigration` will be embedded in
/// the executable so no files are required at run-time. The
/// standard [`include_str!`](https://doc.rust-lang.org/std/macro.include_str.html) macro
/// can be used to embed contents of files, or a string literal can be provided.
///
/// *Note:* By default the migrator wraps this migration's SQL and its
/// bookkeeping row in a single transaction, so do **not** add your own
/// `begin`/`commit` to the SQL. To opt a direction out (for statements a backend
/// refuses to run in a transaction block, e.g. a Postgres `CREATE INDEX
/// CONCURRENTLY` or `ALTER TYPE ... ADD VALUE`), put the directive
/// `-- migrant:no-transaction` on a comment line in that direction's SQL (it
/// travels with an `include_str!`ed file), or call
/// [`no_transaction`](EmbeddedMigration::no_transaction) to opt out both
/// directions. A directive in the SQL takes precedence over the builder flag.
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
    pub(crate) no_transaction: bool,
}

impl EmbeddedMigration {
    /// Create a new `EmbeddedMigration` with the given tag
    pub fn with_tag(tag: &str) -> Self {
        Self {
            tag: tag.to_owned(),
            up: None,
            down: None,
            no_transaction: false,
        }
    }

    /// Opt both of this migration's directions out of the migrator's automatic
    /// transaction wrapping.
    ///
    /// Use this for statements a backend refuses to run inside a transaction
    /// block, such as Postgres `CREATE INDEX CONCURRENTLY`. For per-direction
    /// control, put the `-- migrant:no-transaction` directive on a comment line
    /// in the relevant direction's SQL instead; a directive in the SQL takes
    /// precedence over this flag. When opted out, that direction's SQL and its
    /// bookkeeping row are no longer applied atomically.
    pub fn no_transaction(&mut self) -> &mut Self {
        self.no_transaction = true;
        self
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

    fn use_transaction(&self, direction: Direction) -> bool {
        let sql = match direction {
            Direction::Up => &self.up,
            Direction::Down => &self.down,
        };
        // A directive embedded in this direction's SQL takes precedence over the
        // builder-level `no_transaction` flag.
        let declared = match sql {
            Some(s) => sql_opts_out_of_transaction(s),
            None => false,
        };
        if declared {
            return false;
        }
        !self.no_transaction
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
/// Note, `up` and `down` are each optional. A missing function is a silent no-op for that
/// direction -- for example, applying a `Down` migration whose `down` function was never set
/// will still mark the migration as un-applied in the migration table, even though no SQL or
/// code actually ran. There is a noop function available (`migrant_lib::migration::noop`) for
/// convenience if you want to be explicit about a direction doing nothing.
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

    /// Function migrations run arbitrary code (and may open their own
    /// connections), so the migrator cannot wrap them in a single transaction
    /// on its connection. Always `false`.
    fn use_transaction(&self, _direction: Direction) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_no_transaction_directive() {
        assert!(sql_opts_out_of_transaction(
            "-- migrant:no-transaction\ncreate table t (x integer);"
        ));
        // no space after the comment marker
        assert!(sql_opts_out_of_transaction("--migrant:no-transaction"));
        // case-insensitive
        assert!(sql_opts_out_of_transaction("-- MIGRANT:No-Transaction"));
        // a trailing explanation is allowed
        assert!(sql_opts_out_of_transaction(
            "-- migrant:no-transaction (enum add)\nalter type mood add value 'x';"
        ));
        // not required to be the first line
        assert!(sql_opts_out_of_transaction(
            "create table t (x integer);\n-- migrant:no-transaction"
        ));
    }

    #[test]
    fn ignores_absent_or_unrelated_directive() {
        assert!(!sql_opts_out_of_transaction("create table t (x integer);"));
        assert!(!sql_opts_out_of_transaction("-- just a normal comment"));
        // a longer token that merely starts with the directive must not match
        assert!(!sql_opts_out_of_transaction(
            "-- migrant:no-transaction-please"
        ));
        // the text appearing outside a comment must not match
        assert!(!sql_opts_out_of_transaction(
            "select 'migrant:no-transaction';"
        ));
        assert!(!sql_opts_out_of_transaction(""));
    }

    #[test]
    fn embedded_use_transaction_is_per_direction_and_directive_wins() {
        // `up` opts out via directive, `down` does not: transactionality differs
        // by direction, and the directive overrides the (unset) builder flag.
        let mut m = EmbeddedMigration::with_tag("m");
        m.up("-- migrant:no-transaction\nalter type mood add value 'x';")
            .down("drop type mood;");
        assert!(!m.use_transaction(Direction::Up));
        assert!(m.use_transaction(Direction::Down));

        // builder-level opt-out applies to any direction without a directive
        let mut m2 = EmbeddedMigration::with_tag("m2");
        m2.up("select 1;").down("select 1;").no_transaction();
        assert!(!m2.use_transaction(Direction::Up));
        assert!(!m2.use_transaction(Direction::Down));
    }
}
