/*!
The `Migratable` trait
*/
use std::fmt;

use crate::migrator::Direction;
use crate::Config;

/// Helper trait so boxed `Migratable` trait objects can be cloned
pub trait MigratableClone {
    /// Clone into a new boxed trait object
    fn clone_migratable_box(&self) -> Box<dyn Migratable>;
}
impl<T> MigratableClone for T
where
    T: 'static + Migratable + Clone,
{
    fn clone_migratable_box(&self) -> Box<dyn Migratable> {
        Box::new(self.clone())
    }
}

/// A type that can be used to define database migrations
pub trait Migratable: MigratableClone {
    /// Define functionality that runs for `up` migrations
    fn apply_up(&self, _: &Config) -> std::result::Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    /// Define functionality that runs for `down` migrations
    fn apply_down(&self, _: &Config) -> std::result::Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    /// A unique identifying tag
    fn tag(&self) -> String;

    /// Optional migration description. Defaults to `Migratable::tag`
    fn description(&self, _: &Direction) -> String {
        self.tag()
    }

    /// Whether the migrator should wrap this migration's application and its
    /// bookkeeping row (the insert/delete in `__migrant_migrations`) in a
    /// single database transaction, so the two commit or roll back together.
    ///
    /// Defaults to `true`. Override to `false` for migrations the migrator
    /// cannot meaningfully wrap in one transaction on its own connection --
    /// for example arbitrary-code migrations (`FnMigration` returns `false`),
    /// or statements a backend refuses to run inside a transaction block
    /// (e.g. Postgres `CREATE INDEX CONCURRENTLY`). See `no_transaction` on
    /// [`EmbeddedMigration`](crate::EmbeddedMigration) and
    /// [`FileMigration`](crate::FileMigration).
    ///
    /// Note: MySQL/MariaDB commit DDL implicitly, so transactional wrapping
    /// there only makes pure-DML migrations atomic; DDL cannot be rolled back
    /// regardless of this setting.
    fn use_transaction(&self) -> bool {
        true
    }
}

impl Clone for Box<dyn Migratable> {
    fn clone(&self) -> Box<dyn Migratable> {
        self.clone_migratable_box()
    }
}

impl fmt::Debug for Box<dyn Migratable> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Migration: {}", self.tag())
    }
}
