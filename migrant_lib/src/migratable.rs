/*!
The `Migratable` trait
*/
use std::fmt;

use crate::errors::*;
use crate::macros::bail;
use crate::migrator::Direction;
use crate::Config;

mod private {
    /// Sealing marker: only types in this crate that satisfy the blanket impl
    /// below can implement [`MigratableClone`](super::MigratableClone).
    pub trait Sealed {}
}
impl<T: 'static + Migratable + Clone> private::Sealed for T {}

/// Helper trait so boxed `Migratable` trait objects can be cloned.
///
/// This trait is sealed: it is implemented automatically for every
/// `'static + Migratable + Clone` type and cannot be implemented directly.
pub trait MigratableClone: private::Sealed {
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

    /// The lowercase hex sha256 of this migration's raw up-direction SQL bytes,
    /// recorded in the `checksum` column of `__migrant_migrations` when the
    /// migration is applied.
    ///
    /// Defaults to `None`. Programmatic migrations (`FnMigration` and custom
    /// implementations) have no SQL to hash, so they store NULL by design.
    fn checksum(&self) -> Option<String> {
        None
    }

    /// Optional migration description. Defaults to `Migratable::tag`
    fn description(&self, _: Direction) -> String {
        self.tag()
    }

    /// Whether the migrator should wrap this migration's application in the
    /// given `direction`, together with its bookkeeping row (the insert/delete
    /// in `__migrant_migrations`), in a single database transaction so the two
    /// commit or roll back together.
    ///
    /// Defaults to `true` for both directions. Override to return `false` for a
    /// direction the migrator cannot meaningfully wrap in one transaction on its
    /// own connection -- for example arbitrary-code migrations (`FnMigration`
    /// returns `false`), or statements a backend refuses to run inside a
    /// transaction block (e.g. Postgres `CREATE INDEX CONCURRENTLY` or
    /// `ALTER TYPE ... ADD VALUE`). See `no_transaction` and the
    /// `-- migrant:no-transaction` file directive on
    /// [`EmbeddedMigration`](crate::EmbeddedMigration) and
    /// [`FileMigration`](crate::FileMigration).
    ///
    /// Note: MySQL/MariaDB commit DDL implicitly, so transactional wrapping
    /// there only makes pure-DML migrations atomic; DDL cannot be rolled back
    /// regardless of this setting.
    fn use_transaction(&self, direction: Direction) -> bool {
        let _ = direction;
        true
    }

    /// Whether this migration re-runs its up-direction every time its
    /// [`checksum`](Migratable::checksum) changes, instead of applying exactly
    /// once.
    ///
    /// Defaults to `false` (a versioned migration). A repeatable migration is
    /// re-run whenever its current checksum differs from the one recorded in
    /// `__migrant_migrations`, which suits idempotent data work (seeding,
    /// backfills, refreshing views) rather than one-time schema versioning.
    /// [`EmbeddedMigration`](crate::EmbeddedMigration) and
    /// [`FileMigration`](crate::FileMigration) declare it with their
    /// `repeatable()` builder method or a `-- migrant:repeatable` directive in
    /// their up-SQL.
    ///
    /// A repeatable migration must report a `checksum` (there is otherwise no
    /// way to tell that it changed) and must not define a down direction; both
    /// are rejected when the migration set is registered.
    ///
    /// Named `is_repeatable` because `EmbeddedMigration`/`FileMigration` expose
    /// a `repeatable()` *builder* method, mirroring the
    /// `no_transaction()`/[`use_transaction`](Migratable::use_transaction)
    /// split.
    fn is_repeatable(&self) -> bool {
        false
    }

    /// Whether this migration has a down direction to run.
    ///
    /// Defaults to `false`. This exists so a
    /// [`repeatable`](Migratable::is_repeatable) migration that also defines a
    /// down (which would never run, since a `Down`
    /// run never selects a repeatable migration) is rejected rather than
    /// silently ignored. Implementations that are never repeatable can leave it
    /// at the default.
    fn defines_down(&self) -> bool {
        false
    }
}

/// Reject migration sets that declare a combination the migrator cannot honor.
///
/// A repeatable migration must have a checksum to compare against the recorded
/// one, and must not define a down direction it would never run. Checked when
/// an explicit set is registered with
/// [`Config::use_migrations`](crate::Config::use_migrations) and again when the
/// migrator loads the available set, so file-discovered migrations declaring
/// `-- migrant:repeatable` are covered too.
pub(crate) fn validate_migrations(migrations: &[Box<dyn Migratable>]) -> Result<()> {
    for migration in migrations {
        if !migration.is_repeatable() {
            continue;
        }
        let tag = migration.tag();
        if migration.checksum().is_none() {
            bail!(
                Migration,
                "Repeatable migration `{}` has no checksum, so a change to it \
                 could never be detected. Check that its up-SQL is set and, for \
                 a file migration, that the up file exists and is readable. \
                 Programmatic migrations have no SQL to hash and cannot be \
                 repeatable.",
                tag
            )
        }
        if migration.defines_down() {
            bail!(
                Migration,
                "Repeatable migration `{}` defines a down direction, which would \
                 never run: repeatable migrations are forward-only. Remove the \
                 down direction (for a file migration, delete its `down.sql`; an \
                 empty file still counts), or drop the repeatable declaration.",
                tag
            )
        }
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migration::{noop, EmbeddedMigration, FnMigration};

    // REPEAT-7
    #[test]
    fn a_repeatable_migration_without_a_checksum_is_rejected() {
        // A repeatable migration with no up-SQL has nothing to hash, so a
        // change to it could never be detected.
        let migrations = vec![EmbeddedMigration::with_tag("seed").repeatable().boxed()];
        match validate_migrations(&migrations) {
            Err(Error::Migration(msg)) => {
                assert!(msg.contains("seed"), "message should name the tag: {msg}");
                assert!(
                    msg.contains("checksum"),
                    "message should explain the missing checksum: {msg}"
                );
            }
            other => panic!("expected Error::Migration, got: {other:?}"),
        }
    }

    // REPEAT-7
    #[test]
    fn a_programmatic_migration_cannot_be_repeatable() {
        // `FnMigration` has no SQL to hash. It reports `is_repeatable() ==
        // false` by default, so the set is valid; a custom `Migratable` that
        // returns `true` without a checksum is what the check catches.
        #[derive(Clone)]
        struct AlwaysRepeatable;
        impl Migratable for AlwaysRepeatable {
            fn tag(&self) -> String {
                "custom".to_string()
            }
            fn is_repeatable(&self) -> bool {
                true
            }
        }

        let programmatic = vec![FnMigration::with_tag("f").up(noop).down(noop).boxed()];
        validate_migrations(&programmatic).expect("a non-repeatable FnMigration is fine");

        let custom: Vec<Box<dyn Migratable>> = vec![Box::new(AlwaysRepeatable)];
        assert!(
            validate_migrations(&custom).is_err(),
            "a checksum-less custom migration cannot be repeatable"
        );
    }

    // REPEAT-4
    #[test]
    fn a_repeatable_migration_with_a_down_is_rejected() {
        let migrations = vec![EmbeddedMigration::with_tag("seed")
            .repeatable()
            .up("select 1;")
            .down("select -1;")
            .boxed()];
        match validate_migrations(&migrations) {
            Err(Error::Migration(msg)) => {
                assert!(msg.contains("seed"), "message should name the tag: {msg}");
                assert!(
                    msg.contains("down"),
                    "message should explain the down direction: {msg}"
                );
            }
            other => panic!("expected Error::Migration, got: {other:?}"),
        }
    }

    #[test]
    fn valid_sets_pass() {
        let migrations = vec![
            EmbeddedMigration::with_tag("a")
                .up("select 1;")
                .down("select -1;")
                .boxed(),
            // Repeatable: up-SQL to hash, and no down direction.
            EmbeddedMigration::with_tag("seed")
                .repeatable()
                .up("select 2;")
                .boxed(),
        ];
        validate_migrations(&migrations).expect("a well-formed set must pass");
    }
}
