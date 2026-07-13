/*!
Migration application
*/
use std::fmt;

use crate::config::Config;
use crate::errors::*;
use crate::macros::{bail, err};
use crate::migratable::Migratable;
use crate::ops;
use crate::util::print_flush;
use crate::DbKind;

/// Represents direction to apply migrations.
/// `Up`   -> up.sql
/// `Down` -> down.sql
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Apply `up` migrations
    Up,
    /// Apply `down` migrations
    Down,
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Direction::Up => write!(f, "Up"),
            Direction::Down => write!(f, "Down"),
        }
    }
}

/// Migration applicator
#[derive(Debug, Clone)]
pub struct Migrator {
    config: Config,
    direction: Direction,
    force: bool,
    fake: bool,
    all: bool,
    show_output: bool,
    swallow_completion: bool,
    synchronized: bool,
}

impl Migrator {
    /// Initialize a new `Migrator` with a given `&Config`
    pub fn with_config(config: &Config) -> Self {
        Self {
            config: config.clone(),
            direction: Direction::Up,
            force: false,
            fake: false,
            all: false,
            show_output: true,
            swallow_completion: false,
            synchronized: true,
        }
    }

    /// Set `direction`. Default is `Up`.
    /// `Up`   => run `up.sql`.
    /// `Down` => run `down.sql`.
    pub fn direction(&mut self, dir: Direction) -> &mut Self {
        self.direction = dir;
        self
    }

    /// Set `force` to forcefully apply migrations regardless of errors
    pub fn force(&mut self, force: bool) -> &mut Self {
        self.force = force;
        self
    }

    /// Set `fake` to fake application of migrations.
    /// Applied migrations table will be updated as if migrations were actually run.
    pub fn fake(&mut self, fake: bool) -> &mut Self {
        self.fake = fake;
        self
    }

    /// Set `all` to run all remaining available migrations in the given `direction`
    pub fn all(&mut self, all: bool) -> &mut Self {
        self.all = all;
        self
    }

    /// Toggle migration application output. Default is `true`
    pub fn show_output(&mut self, show_output: bool) -> &mut Self {
        self.show_output = show_output;
        self
    }

    /// Don't return any `Error::MigrationComplete` errors when running `Migrator::apply`
    ///
    /// All other errors will still be returned
    pub fn swallow_completion(&mut self, swallow_completion: bool) -> &mut Self {
        self.swallow_completion = swallow_completion;
        self
    }

    /// Serialize migration runs across processes using a database advisory lock.
    /// Default is `true`.
    ///
    /// When enabled, a run against a server database (postgres/mysql) takes a
    /// session-level advisory lock for its whole duration, so concurrent
    /// migrators -- for example several application instances booting at once --
    /// apply migrations one at a time instead of racing. The lock is released
    /// when the run finishes, and automatically by the database if the process
    /// dies mid-run. Sqlite has no cross-process migration concurrency to guard
    /// against, so this setting has no effect there.
    ///
    /// Disable it only when an outer mechanism already serializes migrations.
    pub fn synchronized(&mut self, synchronized: bool) -> &mut Self {
        self.synchronized = synchronized;
        self
    }

    /// Apply migrations using current configuration
    ///
    /// Returns an `Error::MigrationComplete` if all migrations in the given
    /// direction have already been applied, unless `swallow_completion` is set to `true`.
    pub fn apply(&self) -> Result<()> {
        let res = self.run();
        if self.swallow_completion {
            match res {
                Err(ref e) if e.is_migration_complete() => Ok(()),
                other => other,
            }
        } else {
            res
        }
    }

    /// Apply migrations until complete (`all`) or a single one has been applied
    fn run(&self) -> Result<()> {
        let mut config = self.config.clone();

        // For server databases, take the migration advisory lock so concurrent
        // migrators (e.g. several app instances booting at once) serialize
        // instead of racing. Acquire it *before* re-reading applied state, then
        // reload under the lock so we observe any migrations a peer committed
        // while we were waiting and don't re-run them. Sqlite has no such lock
        // (and no cross-process concurrency), so its path is unchanged.
        let _lock = if self.synchronized && config.database_type() != DbKind::Sqlite {
            config.acquire_migration_lock()?;
            // Guard is created before `reload` so the lock is released even if
            // the reload fails; the reloaded config shares this connection.
            let guard = MigrationLock::new(&config);
            config = config.reload()?;
            Some(guard)
        } else {
            None
        };

        let mut applied_any = false;
        loop {
            match self.apply_next(&config) {
                Ok(()) => {}
                Err(e) if e.is_migration_complete() && self.all && applied_any => return Ok(()),
                Err(e) => return Err(e),
            }
            applied_any = true;
            if !self.all {
                return Ok(());
            }
            config = config.reload()?;
        }
    }

    /// The set of migrations being managed: either those explicitly defined
    /// on the config, or file-migrations discovered under `migration_location`
    fn available_migrations(config: &Config) -> Result<Vec<Box<dyn Migratable>>> {
        Ok(match config.migrations {
            Some(ref migrations) => migrations.clone(),
            None => {
                let location = config.migration_location()?;
                ops::search_for_migrations(&location)?
                    .into_iter()
                    .map(|fm| fm.boxed())
                    .collect()
            }
        })
    }

    /// Return the next available up or down migration
    fn next_available<'a>(
        direction: Direction,
        available: &'a [Box<dyn Migratable>],
        applied: &[String],
    ) -> Result<Option<&'a dyn Migratable>> {
        Ok(match direction {
            Direction::Up => available
                .iter()
                .find(|m| !applied.contains(&m.tag()))
                .map(AsRef::as_ref),
            Direction::Down => {
                if applied.is_empty() {
                    None
                } else {
                    // Select the Down target by definition order: the last
                    // migration in `available` order whose tag is applied. The
                    // `applied` slice may be unordered (it comes from an
                    // unordered `select tag from __migrant_migrations` unless
                    // running in cli-compatible mode), so we must not rely on
                    // `applied.last()`.
                    match available.iter().rev().find(|m| applied.contains(&m.tag())) {
                        Some(mig) => Some(mig.as_ref()),
                        None => bail!(
                            MigrationNotFound,
                            "Applied migration not found in available migrations: {}",
                            applied[0]
                        ),
                    }
                }
            }
        })
    }

    /// Try applying the next available migration in the specified `Direction`
    fn apply_next(&self, config: &Config) -> Result<()> {
        let migrations = Self::available_migrations(config)?;
        let next = Self::next_available(self.direction, &migrations, &config.applied)?.ok_or_else(
            || {
                err!(
                    MigrationComplete,
                    "No un-applied `{}` migrations found",
                    self.direction
                )
            },
        )?;

        self.print(&format!(
            "Applying[{}]: {}",
            self.direction,
            next.description(&self.direction)
        ));

        let tag = next.tag();

        if self.fake {
            self.println("  ✓ (fake)");
            return self.record_tag(config, &tag);
        }

        // Wrap the migration's SQL and its bookkeeping row in one transaction so
        // they commit or roll back together, per direction (see
        // `Migratable::use_transaction`).
        let transactional = next.use_transaction(self.direction);
        if transactional {
            config.begin_transaction()?;
        }

        match self.apply_and_record(config, next, &tag) {
            Ok(()) => {
                if transactional {
                    config.commit_transaction()?;
                }
                self.println("  ✓");
                Ok(())
            }
            Err(msg) => {
                if transactional {
                    // `with_conn` already rolled the connection back in place on
                    // the error (preserving the session and its advisory lock);
                    // this explicit rollback is a harmless best-effort backstop.
                    config.rollback_transaction();
                }
                self.println("");
                if self.force {
                    self.println(&format!(
                        " ** Error ** (Continuing because `--force` flag was specified)\n ** {}",
                        msg
                    ));
                    // Legacy `force` behavior: record the tag anyway. The
                    // transaction (if any) was rolled back, so this stands alone.
                    self.record_tag(config, &tag)
                } else {
                    bail!(Migration, "Migration was unsucessful...\n{}", msg)
                }
            }
        }
    }

    /// Apply the migration in the current direction and record its bookkeeping
    /// row. Runs inside the caller's transaction (when one is active) so the two
    /// are atomic. Returns the failure's display string on error.
    fn apply_and_record(
        &self,
        config: &Config,
        next: &dyn Migratable,
        tag: &str,
    ) -> std::result::Result<(), String> {
        match self.direction {
            Direction::Up => next.apply_up(config),
            Direction::Down => next.apply_down(config),
        }
        .map_err(|e| e.to_string())?;
        self.record_tag(config, tag).map_err(|e| e.to_string())
    }

    /// Record the migration as applied (`Up`) or un-applied (`Down`) in the
    /// `__migrant_migrations` table.
    fn record_tag(&self, config: &Config, tag: &str) -> Result<()> {
        match self.direction {
            Direction::Up => config.insert_migration_tag(tag),
            Direction::Down => config.delete_migration_tag(tag),
        }
    }

    fn print(&self, s: &str) {
        if self.show_output {
            print_flush(s);
        }
    }

    fn println(&self, s: &str) {
        if self.show_output {
            println!("{}", s);
        }
    }
}

/// RAII guard that releases the migration advisory lock when dropped, so the
/// lock is freed on every exit path from a synchronized run (success, an early
/// `?` error, or a panic). Holds a `Config` clone, which shares the same live
/// connection the lock was taken on.
struct MigrationLock {
    config: Config,
}

impl MigrationLock {
    fn new(config: &Config) -> Self {
        Self {
            config: config.clone(),
        }
    }
}

impl Drop for MigrationLock {
    fn drop(&mut self) {
        self.config.release_migration_lock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migration::EmbeddedMigration;

    fn available(tags: &[&str]) -> Vec<Box<dyn Migratable>> {
        tags.iter()
            .map(|t| EmbeddedMigration::with_tag(t).boxed())
            .collect()
    }

    fn tags(strs: &[&str]) -> Vec<String> {
        strs.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn up_picks_first_unapplied_in_definition_order() {
        let avail = available(&["a", "b", "c"]);
        let applied = tags(&["a"]);
        let next = Migrator::next_available(Direction::Up, &avail, &applied)
            .unwrap()
            .expect("expected an un-applied migration");
        assert_eq!(next.tag(), "b");
    }

    #[test]
    fn up_returns_none_when_all_applied() {
        let avail = available(&["a", "b"]);
        let applied = tags(&["a", "b"]);
        let next = Migrator::next_available(Direction::Up, &avail, &applied).unwrap();
        assert!(next.is_none());
    }

    #[test]
    fn down_picks_last_applied_in_definition_order_even_when_applied_shuffled() {
        let avail = available(&["a", "b", "c", "d"]);
        // `applied` is intentionally shuffled and does not include the final
        // migration `d`. The Down target must be `c` (the last applied tag in
        // definition order), not `applied.last()` which would be `a`.
        let applied = tags(&["b", "c", "a"]);
        let next = Migrator::next_available(Direction::Down, &avail, &applied)
            .unwrap()
            .expect("expected a down migration");
        assert_eq!(next.tag(), "c");
    }

    #[test]
    fn down_with_empty_applied_returns_none() {
        let avail = available(&["a", "b"]);
        let applied: Vec<String> = Vec::new();
        let next = Migrator::next_available(Direction::Down, &avail, &applied).unwrap();
        assert!(next.is_none());
    }

    #[test]
    fn down_with_applied_tags_absent_from_available_errors() {
        let avail = available(&["a", "b"]);
        let applied = tags(&["x", "y"]);
        match Migrator::next_available(Direction::Down, &avail, &applied) {
            Err(Error::MigrationNotFound(_)) => {}
            Err(other) => panic!("expected MigrationNotFound, got: {:?}", other),
            Ok(_) => panic!("expected MigrationNotFound error, got Ok"),
        }
    }
}
