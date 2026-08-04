/*!
Migration application
*/
use std::collections::HashSet;
use std::fmt;

use crate::config::Config;
use crate::errors::*;
use crate::macros::bail;
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

/// How a run handles a migration that fails to apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ForceMode {
    /// A failed migration aborts the run with an error (default).
    #[default]
    Off,
    /// Continue past a failed migration and record it as applied anyway.
    /// The failed migration will *not* be retried on the next run.
    AcceptFailures,
    /// Continue past a failed migration without recording it. The migration
    /// is skipped for the remainder of this run and retried on the next run.
    SkipFailures,
}

impl fmt::Display for ForceMode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ForceMode::Off => write!(f, "off"),
            ForceMode::AcceptFailures => write!(f, "accept-failures"),
            ForceMode::SkipFailures => write!(f, "skip-failures"),
        }
    }
}

impl std::str::FromStr for ForceMode {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        Ok(match s {
            "off" => ForceMode::Off,
            "accept-failures" => ForceMode::AcceptFailures,
            "skip-failures" => ForceMode::SkipFailures,
            _ => bail!(
                Migration,
                "Invalid force mode: `{}`. Expected one of: off, accept-failures, skip-failures",
                s
            ),
        })
    }
}

/// Summary of a migration run returned by [`Migrator::apply`].
///
/// `tags` holds the migration tags whose bookkeeping this run changed, in the
/// order they were processed: for an `Up` run the migrations applied, for a
/// `Down` run the migrations reverted. A `force`d `accept-failures` run includes
/// a tag it recorded despite the migration failing; a `skip-failures` run does
/// not include a skipped tag. An empty report means the database was already up
/// to date (or fully reverted) and nothing ran.
#[derive(Debug, Clone)]
pub struct Report {
    direction: Direction,
    tags: Vec<String>,
}

impl Report {
    fn new(direction: Direction) -> Self {
        Self {
            direction,
            tags: Vec::new(),
        }
    }

    /// The direction this run applied migrations in.
    pub fn direction(&self) -> Direction {
        self.direction
    }

    /// The migration tags whose bookkeeping this run changed, in order.
    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    /// `true` if nothing ran (the database was already up to date, or fully
    /// reverted for a `Down` run).
    pub fn is_empty(&self) -> bool {
        self.tags.is_empty()
    }

    /// The number of migrations applied (`Up`) or reverted (`Down`).
    pub fn len(&self) -> usize {
        self.tags.len()
    }
}

/// Outcome of attempting the next migration in a run.
enum Step {
    /// A migration's bookkeeping was changed (applied/reverted, faked, or
    /// force-recorded); carries its tag.
    Applied(String),
    /// A migration failed and was skipped under `ForceMode::SkipFailures`.
    Skipped,
    /// No further migration is available in this direction.
    Complete,
}

/// Migration applicator
///
/// By default each migration's SQL and its `__migrant_migrations` bookkeeping
/// row are applied in one transaction, so a failure leaves neither behind.
///
/// **MySQL caveat:** MySQL/MariaDB implicitly commit the current transaction on
/// most DDL (`CREATE TABLE`, `ALTER TABLE`, ...). A migration whose `up`/`down`
/// runs such DDL is therefore *not* atomic with its bookkeeping row on MySQL: if
/// a later statement in the same migration fails, the DDL that already ran is not
/// rolled back. Write MySQL DDL migrations to be individually safe to re-run
/// (idempotent), and prefer one schema change per migration. Postgres and sqlite
/// run DDL inside transactions and are unaffected.
#[derive(Debug, Clone)]
pub struct Migrator {
    config: Config,
    direction: Direction,
    force: ForceMode,
    fake: bool,
    all: bool,
    show_output: bool,
    synchronized: bool,
    allow_unknown_tags: bool,
    allow_out_of_order: bool,
}

impl Migrator {
    /// Initialize a new `Migrator` with a given `&Config`
    pub fn with_config(config: &Config) -> Self {
        Self {
            config: config.clone(),
            direction: Direction::Up,
            force: ForceMode::Off,
            fake: false,
            all: false,
            show_output: true,
            synchronized: true,
            allow_unknown_tags: false,
            allow_out_of_order: false,
        }
    }

    /// Set `direction`. Default is `Up`.
    /// `Up`   => run `up.sql`.
    /// `Down` => run `down.sql`.
    pub fn direction(mut self, dir: Direction) -> Self {
        self.direction = dir;
        self
    }

    /// Set how the run handles a migration that fails to apply.
    /// Default is `ForceMode::Off`: a failed migration aborts the run.
    ///
    /// `ForceMode::AcceptFailures` continues past a failed migration and
    /// records it as applied anyway, so it will *not* be retried on the next
    /// run. `ForceMode::SkipFailures` continues without recording it: the
    /// migration is skipped for the remainder of this run and retried on the
    /// next run, so it must be safe to re-attempt after a partial application.
    pub fn force(mut self, force: ForceMode) -> Self {
        self.force = force;
        self
    }

    /// Set `fake` to fake application of migrations.
    /// Applied migrations table will be updated as if migrations were actually run.
    pub fn fake(mut self, fake: bool) -> Self {
        self.fake = fake;
        self
    }

    /// Set `all` to run all remaining available migrations in the given `direction`
    pub fn all(mut self, all: bool) -> Self {
        self.all = all;
        self
    }

    /// Toggle migration application output. Default is `true`
    pub fn show_output(mut self, show_output: bool) -> Self {
        self.show_output = show_output;
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
    pub fn synchronized(mut self, synchronized: bool) -> Self {
        self.synchronized = synchronized;
        self
    }

    /// Allow applied migration tags that are not among the available
    /// migrations. Default is `false`.
    ///
    /// By default an `Up` run aborts with [`Error::MigrationNotFound`] if the
    /// database records a tag that is not in the managed/available set -- for
    /// example a migration that was applied and later removed from the codebase,
    /// which usually signals the wrong migration set or database. Set this to
    /// `true` to tolerate such unknown tags and apply the remaining available
    /// migrations anyway.
    pub fn allow_unknown_tags(mut self, allow: bool) -> Self {
        self.allow_unknown_tags = allow;
        self
    }

    /// Allow applied migrations that are out of order relative to definition
    /// order. Default is `false`.
    ///
    /// By default an `Up` run aborts with [`Error::MigrationOrdering`] if a later
    /// migration (in definition order) was applied while an earlier one was not
    /// -- the situation that arises when a migration is merged behind others that
    /// already ran. Set this to `true` to apply the intervening un-applied
    /// migrations anyway.
    pub fn allow_out_of_order(mut self, allow: bool) -> Self {
        self.allow_out_of_order = allow;
        self
    }

    /// Apply migrations using the current configuration.
    ///
    /// Returns a [`Report`] of the migration tags whose bookkeeping this run
    /// changed (applied for `Up`, reverted for `Down`), in order. When the
    /// database is already up to date (or fully reverted) nothing runs and the
    /// report is empty ([`Report::is_empty`]) -- this is not an error.
    pub fn apply(&self) -> Result<Report> {
        self.run()
    }

    /// Apply migrations until complete (`all`) or a single one has been applied
    fn run(&self) -> Result<Report> {
        let mut config = self.config.clone();

        // For server databases, take the migration advisory lock so concurrent
        // migrators (e.g. several app instances booting at once) serialize
        // instead of racing. Acquire it *before* re-reading applied state so we
        // observe any migrations a peer committed while we were waiting and
        // don't re-run them. Sqlite has no such lock (and no cross-process
        // concurrency), so it skips the lock.
        let lock = if self.synchronized && config.database_type() != DbKind::Sqlite {
            config.acquire_migration_lock()?;
            Some(MigrationLock::new(&config))
        } else {
            None
        };
        // Generation of the connection the lock was taken on. If that
        // connection is ever dropped and re-established mid-run, the session
        // -- and the advisory lock with it -- is gone, so a synchronized run
        // must abort rather than continue unserialized.
        let lock_generation = lock.as_ref().map(|_| config.connection_generation());

        // Re-read applied state from the database on the (locked) connection.
        // This intentionally does not use `Config::reload`, which re-reads the
        // settings file and can swap in a *new* connection if the settings
        // changed -- the whole run must stay on the connection the lock was
        // acquired on. It also means consumers don't need to remember to call
        // `Config::reload` themselves before applying.
        config.refresh_applied()?;

        // Tags that failed under `ForceMode::SkipFailures`, excluded from
        // migration selection for the remainder of this run.
        let mut skipped = HashSet::new();
        let mut report = Report::new(self.direction);
        loop {
            self.check_lock_still_held(&config, lock_generation)?;
            match self.apply_next(&config, &mut skipped, lock_generation)? {
                Step::Applied(tag) => {
                    report.tags.push(tag);
                    if !self.all {
                        return Ok(report);
                    }
                }
                Step::Skipped => {
                    // The migration failed and was left unrecorded; a single-step
                    // run has taken its one attempt, so stop.
                    if !self.all {
                        return Ok(report);
                    }
                }
                Step::Complete => return Ok(report),
            }
            config.refresh_applied()?;
        }
    }

    /// Bail out of a synchronized run if the connection the advisory lock was
    /// acquired on has been dropped and re-established: the lock died with the
    /// original session, so continuing would run unserialized.
    fn check_lock_still_held(&self, config: &Config, lock_generation: Option<u64>) -> Result<()> {
        if let Some(generation) = lock_generation {
            if config.connection_generation() != generation {
                bail!(
                    Migration,
                    "The database connection was lost mid-run and re-established; \
                     the migration advisory lock was released with the original \
                     session. Aborting this run -- re-run migrations."
                )
            }
        }
        Ok(())
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

    /// Return the next available up or down migration, excluding any tags
    /// skipped earlier in this run (`ForceMode::SkipFailures`).
    ///
    /// For an `Up` run this first enforces the strictness checks (unknown applied
    /// tags, out-of-order application) unless they have been opted out of.
    fn next_available<'a>(
        direction: Direction,
        available: &'a [Box<dyn Migratable>],
        applied: &[String],
        skipped: &HashSet<String>,
        allow_unknown_tags: bool,
        allow_out_of_order: bool,
    ) -> Result<Option<&'a dyn Migratable>> {
        Ok(match direction {
            Direction::Up => {
                Self::check_applied_consistency(
                    available,
                    applied,
                    skipped,
                    allow_unknown_tags,
                    allow_out_of_order,
                )?;
                available
                    .iter()
                    .find(|m| !applied.contains(&m.tag()) && !skipped.contains(&m.tag()))
                    .map(AsRef::as_ref)
            }
            Direction::Down => {
                // Recorded application order is authoritative, so the most
                // recently applied migration is `applied.last()`. Walk backwards,
                // skipping tags that failed earlier this run, and return the
                // corresponding available migration. A target tag absent from the
                // available set is a hard error, matching the previous behavior.
                for tag in applied.iter().rev() {
                    if skipped.contains(tag) {
                        continue;
                    }
                    match available.iter().find(|m| &m.tag() == tag) {
                        Some(m) => return Ok(Some(m.as_ref())),
                        None => bail!(
                            MigrationNotFound,
                            "Applied migration not found in available migrations: {}",
                            tag
                        ),
                    }
                }
                None
            }
        })
    }

    /// Enforce the `Up`-run strictness checks against the applied set. Tags in
    /// `skipped` (failed earlier this run under `ForceMode::SkipFailures`) count
    /// as neither applied nor blocking, so a `skip-failures` run is not
    /// self-defeating.
    fn check_applied_consistency(
        available: &[Box<dyn Migratable>],
        applied: &[String],
        skipped: &HashSet<String>,
        allow_unknown_tags: bool,
        allow_out_of_order: bool,
    ) -> Result<()> {
        if !allow_unknown_tags {
            for tag in applied {
                if skipped.contains(tag) {
                    continue;
                }
                if !available.iter().any(|m| &m.tag() == tag) {
                    bail!(
                        MigrationNotFound,
                        "Applied migration `{}` is not among the available migrations. \
                         Pass `allow_unknown_tags(true)` to ignore unknown applied tags.",
                        tag
                    )
                }
            }
        }
        if !allow_out_of_order {
            // Walk definition order tracking the first un-applied (and un-skipped)
            // migration. An applied migration appearing after it was applied out
            // of order.
            let mut first_unapplied: Option<String> = None;
            for m in available {
                let tag = m.tag();
                if skipped.contains(&tag) {
                    continue;
                }
                if applied.contains(&tag) {
                    if let Some(ref earlier) = first_unapplied {
                        bail!(
                            MigrationOrdering,
                            "Migration `{}` was applied out of order: it comes after `{}` \
                             in definition order, which has not been applied. Pass \
                             `allow_out_of_order(true)` to apply the intervening migrations.",
                            tag,
                            earlier
                        )
                    }
                } else if first_unapplied.is_none() {
                    first_unapplied = Some(tag);
                }
            }
        }
        Ok(())
    }

    /// Try applying the next available migration in the specified `Direction`
    fn apply_next(
        &self,
        config: &Config,
        skipped: &mut HashSet<String>,
        lock_generation: Option<u64>,
    ) -> Result<Step> {
        let migrations = Self::available_migrations(config)?;
        let next = match Self::next_available(
            self.direction,
            &migrations,
            &config.applied,
            skipped,
            self.allow_unknown_tags,
            self.allow_out_of_order,
        )? {
            Some(next) => next,
            None => return Ok(Step::Complete),
        };

        self.print(&format!(
            "Applying[{}]: {}",
            self.direction,
            next.description(self.direction)
        ));

        let tag = next.tag();

        if self.fake {
            self.println("  ✓ (fake)");
            self.record_tag(config, next)?;
            return Ok(Step::Applied(tag));
        }

        // Wrap the migration's SQL and its bookkeeping row in one transaction so
        // they commit or roll back together, per direction (see
        // `Migratable::use_transaction`).
        let transactional = next.use_transaction(self.direction);
        if transactional {
            config.begin_transaction()?;
        }

        match self.apply_and_record(config, next) {
            Ok(()) => {
                if transactional {
                    config.commit_transaction()?;
                }
                self.println("  ✓");
                Ok(Step::Applied(tag))
            }
            Err(msg) => {
                if transactional {
                    // `with_conn` already rolled the connection back in place on
                    // the error (preserving the session and its advisory lock);
                    // this explicit rollback is a harmless best-effort backstop.
                    config.rollback_transaction();
                }
                self.println("");
                match self.force {
                    ForceMode::Off => bail!(Migration, "Migration was unsuccessful...\n{}", msg),
                    ForceMode::AcceptFailures => {
                        self.println(&format!(
                            " ** Error ** (Continuing and recording the migration \
                             as applied because force is `accept-failures`)\n ** {}",
                            msg
                        ));
                        // The failure may have killed the connection; recording
                        // the tag would silently reconnect without the advisory
                        // lock, so verify the locked session is still alive first.
                        self.check_lock_still_held(config, lock_generation)?;
                        // The transaction (if any) was rolled back, so this
                        // bookkeeping row stands alone.
                        self.record_tag(config, next)?;
                        Ok(Step::Applied(tag))
                    }
                    ForceMode::SkipFailures => {
                        self.println(&format!(
                            " ** Error ** (Continuing without recording because force \
                             is `skip-failures`; the migration will be retried on the \
                             next run)\n ** {}",
                            msg
                        ));
                        skipped.insert(tag);
                        Ok(Step::Skipped)
                    }
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
    ) -> std::result::Result<(), String> {
        match self.direction {
            Direction::Up => next.apply_up(config),
            Direction::Down => next.apply_down(config),
        }
        .map_err(|e| e.to_string())?;
        self.record_tag(config, next).map_err(|e| e.to_string())
    }

    /// Record the migration as applied (`Up`) or un-applied (`Down`) in the
    /// `__migrant_migrations` table. An `Up` record carries the migration's
    /// checksum (`None` for programmatic migrations, stored as NULL).
    fn record_tag(&self, config: &Config, next: &dyn Migratable) -> Result<()> {
        let tag = next.tag();
        match self.direction {
            Direction::Up => config.insert_migration_tag(&tag, next.checksum().as_deref()),
            Direction::Down => config.delete_migration_tag(&tag),
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

    fn no_skips() -> HashSet<String> {
        HashSet::new()
    }

    fn skips(strs: &[&str]) -> HashSet<String> {
        strs.iter().map(|s| (*s).to_owned()).collect()
    }

    /// Strict selection with both opt-outs off -- the default the migrator uses.
    fn next_strict<'a>(
        direction: Direction,
        available: &'a [Box<dyn Migratable>],
        applied: &[String],
        skipped: &HashSet<String>,
    ) -> Result<Option<&'a dyn Migratable>> {
        Migrator::next_available(direction, available, applied, skipped, false, false)
    }

    #[test]
    fn owned_setters_chain_and_apply_each_value() {
        // The setters take owned `self` and return owned `Self`, so a full
        // configuration chains from `with_config` through to a value without an
        // intermediate `mut` binding, and every setter must carry its value.
        let settings = crate::config::Settings::configure_sqlite()
            .memory()
            .build()
            .unwrap();
        let config = Config::with_settings(settings);
        let migrator = Migrator::with_config(&config)
            .direction(Direction::Down)
            .force(ForceMode::AcceptFailures)
            .fake(true)
            .all(true)
            .show_output(false)
            .synchronized(false)
            .allow_unknown_tags(true)
            .allow_out_of_order(true);
        assert_eq!(migrator.direction, Direction::Down);
        assert_eq!(migrator.force, ForceMode::AcceptFailures);
        assert!(migrator.fake);
        assert!(migrator.all);
        assert!(!migrator.show_output);
        assert!(!migrator.synchronized);
        assert!(migrator.allow_unknown_tags);
        assert!(migrator.allow_out_of_order);
    }

    #[test]
    fn strictness_defaults_are_false() {
        let settings = crate::config::Settings::configure_sqlite()
            .memory()
            .build()
            .unwrap();
        let config = Config::with_settings(settings);
        let migrator = Migrator::with_config(&config);
        assert!(!migrator.allow_unknown_tags);
        assert!(!migrator.allow_out_of_order);
    }

    #[test]
    fn up_picks_first_unapplied_in_definition_order() {
        let avail = available(&["a", "b", "c"]);
        let applied = tags(&["a"]);
        let next = next_strict(Direction::Up, &avail, &applied, &no_skips())
            .unwrap()
            .expect("expected an un-applied migration");
        assert_eq!(next.tag(), "b");
    }

    #[test]
    fn up_returns_none_when_all_applied() {
        let avail = available(&["a", "b"]);
        let applied = tags(&["a", "b"]);
        let next = next_strict(Direction::Up, &avail, &applied, &no_skips()).unwrap();
        assert!(next.is_none());
    }

    #[test]
    fn up_skips_run_skipped_tags() {
        let avail = available(&["a", "b", "c"]);
        let applied = tags(&["a"]);
        // `b` failed under skip-failures earlier in the run: `c` is next.
        let next = next_strict(Direction::Up, &avail, &applied, &skips(&["b"]))
            .unwrap()
            .expect("expected an un-applied migration");
        assert_eq!(next.tag(), "c");
    }

    #[test]
    fn up_with_all_remaining_skipped_returns_none() {
        let avail = available(&["a", "b"]);
        let applied = tags(&["a"]);
        let next = next_strict(Direction::Up, &avail, &applied, &skips(&["b"])).unwrap();
        assert!(next.is_none());
    }

    #[test]
    fn down_picks_last_applied_in_recorded_order() {
        let avail = available(&["a", "b", "c", "d"]);
        // Recorded application order is authoritative: `b` is the most recently
        // applied migration (last in the recorded list) even though it comes
        // before `c` in definition order. Down must target `applied.last()` = `b`,
        // not the definition-order-last applied tag `c`.
        let applied = tags(&["a", "c", "b"]);
        let next = next_strict(Direction::Down, &avail, &applied, &no_skips())
            .unwrap()
            .expect("expected a down migration");
        assert_eq!(next.tag(), "b");
    }

    #[test]
    fn down_skips_run_skipped_tags() {
        let avail = available(&["a", "b", "c"]);
        let applied = tags(&["a", "b", "c"]);
        // `c`'s down failed under skip-failures: `b` is next.
        let next = next_strict(Direction::Down, &avail, &applied, &skips(&["c"]))
            .unwrap()
            .expect("expected a down migration");
        assert_eq!(next.tag(), "b");
    }

    #[test]
    fn down_with_all_applied_skipped_returns_none() {
        let avail = available(&["a", "b"]);
        let applied = tags(&["a", "b"]);
        let next = next_strict(Direction::Down, &avail, &applied, &skips(&["a", "b"])).unwrap();
        assert!(next.is_none());
    }

    #[test]
    fn down_with_empty_applied_returns_none() {
        let avail = available(&["a", "b"]);
        let applied: Vec<String> = Vec::new();
        let next = next_strict(Direction::Down, &avail, &applied, &no_skips()).unwrap();
        assert!(next.is_none());
    }

    #[test]
    fn down_with_applied_tags_absent_from_available_errors() {
        let avail = available(&["a", "b"]);
        let applied = tags(&["x", "y"]);
        match next_strict(Direction::Down, &avail, &applied, &no_skips()) {
            Err(Error::MigrationNotFound(_)) => {}
            Err(other) => panic!("expected MigrationNotFound, got: {:?}", other),
            Ok(_) => panic!("expected MigrationNotFound error, got Ok"),
        }
    }

    #[test]
    fn up_unknown_applied_tag_errors_by_default() {
        let avail = available(&["a", "b"]);
        // `x` is applied but not among the available migrations.
        let applied = tags(&["a", "x"]);
        match next_strict(Direction::Up, &avail, &applied, &no_skips()).map(|o| o.map(|m| m.tag()))
        {
            Err(Error::MigrationNotFound(_)) => {}
            other => panic!("expected MigrationNotFound, got: {:?}", other),
        }
    }

    #[test]
    fn up_unknown_applied_tag_allowed_when_opted_out() {
        let avail = available(&["a", "b"]);
        let applied = tags(&["a", "x"]);
        // With `allow_unknown_tags`, the unknown `x` is ignored and the next
        // available migration `b` is selected.
        let next =
            Migrator::next_available(Direction::Up, &avail, &applied, &no_skips(), true, false)
                .unwrap()
                .expect("expected an un-applied migration");
        assert_eq!(next.tag(), "b");
    }

    #[test]
    fn up_out_of_order_applied_tag_errors_by_default() {
        let avail = available(&["a", "b", "c"]);
        // `c` is applied while the earlier `b` is not: out of order.
        let applied = tags(&["a", "c"]);
        match next_strict(Direction::Up, &avail, &applied, &no_skips()).map(|o| o.map(|m| m.tag()))
        {
            Err(Error::MigrationOrdering(msg)) => {
                assert!(
                    msg.contains("c"),
                    "message should name the out-of-order tag: {msg}"
                );
                assert!(
                    msg.contains("b"),
                    "message should name the earlier unapplied tag: {msg}"
                );
            }
            other => panic!("expected MigrationOrdering, got: {:?}", other),
        }
    }

    #[test]
    fn up_out_of_order_allowed_when_opted_out() {
        let avail = available(&["a", "b", "c"]);
        let applied = tags(&["a", "c"]);
        // With `allow_out_of_order`, the intervening `b` is selected next.
        let next =
            Migrator::next_available(Direction::Up, &avail, &applied, &no_skips(), false, true)
                .unwrap()
                .expect("expected an un-applied migration");
        assert_eq!(next.tag(), "b");
    }

    #[test]
    fn up_skipped_tags_do_not_trigger_ordering_or_unknown_errors() {
        // A `skip-failures` run must not be self-defeating: a tag in the skipped
        // set counts as neither applied (so no ordering violation) nor blocking.
        let avail = available(&["a", "b", "c"]);
        let applied = tags(&["a"]);
        // `b` failed and was skipped this run; selecting past it to `c` must not
        // raise an out-of-order error even though `b` (unapplied) precedes `c`.
        let next = next_strict(Direction::Up, &avail, &applied, &skips(&["b"]))
            .unwrap()
            .expect("expected an un-applied migration");
        assert_eq!(next.tag(), "c");
    }

    #[test]
    fn up_unknown_takes_precedence_over_out_of_order() {
        // The applied set contains *both* an unknown tag (`x`, not among the
        // available migrations) and an out-of-order condition (`c` applied while
        // the earlier `a`/`b` are not). With both checks enabled (the default),
        // the unknown-tag check runs first, so `MigrationNotFound` -- not
        // `MigrationOrdering` -- is the error that surfaces.
        let avail = available(&["a", "b", "c"]);
        let applied = tags(&["x", "c"]);
        match next_strict(Direction::Up, &avail, &applied, &no_skips()) {
            Err(Error::MigrationNotFound(_)) => {}
            other => panic!(
                "unknown-tag check must take precedence over ordering, got: {:?}",
                other.map(|o| o.map(|m| m.tag()))
            ),
        }
    }

    #[test]
    fn up_allow_unknown_still_enforces_ordering_independently() {
        // Opting out of the unknown-tag check must not also disable the ordering
        // check: with the same set, once `x` is tolerated the still-active
        // ordering check catches `c` applied ahead of the earlier migrations.
        let avail = available(&["a", "b", "c"]);
        let applied = tags(&["x", "c"]);
        match Migrator::next_available(Direction::Up, &avail, &applied, &no_skips(), true, false) {
            Err(Error::MigrationOrdering(_)) => {}
            other => panic!(
                "ordering check must remain active when only unknown tags are allowed, got: {:?}",
                other.map(|o| o.map(|m| m.tag()))
            ),
        }
    }

    #[test]
    fn up_allow_out_of_order_still_enforces_unknown_independently() {
        // The mirror case: opting out of the ordering check must not disable the
        // unknown-tag check. `x` is unknown and must still raise
        // `MigrationNotFound` even with `allow_out_of_order`.
        let avail = available(&["a", "b"]);
        let applied = tags(&["a", "x"]);
        match Migrator::next_available(Direction::Up, &avail, &applied, &no_skips(), false, true) {
            Err(Error::MigrationNotFound(_)) => {}
            other => panic!(
                "unknown-tag check must remain active when only ordering is allowed, got: {:?}",
                other.map(|o| o.map(|m| m.tag()))
            ),
        }
    }

    #[test]
    fn up_skipped_unknown_tag_does_not_raise_not_found() {
        // A tag in the `skipped` set is excluded from the unknown-tag check too
        // (not only the ordering check): a skipped tag that happens not to be
        // among the available migrations must not raise `MigrationNotFound`.
        let avail = available(&["a", "b"]);
        let applied = tags(&["a", "ghost"]);
        let next = next_strict(Direction::Up, &avail, &applied, &skips(&["ghost"]))
            .unwrap()
            .expect("expected an un-applied migration");
        assert_eq!(next.tag(), "b");
    }

    #[test]
    fn down_does_not_run_the_up_consistency_checks() {
        // The strictness checks are `Up`-only. A `Down` run against an
        // out-of-order applied set must not raise `MigrationOrdering`; it simply
        // targets the most-recently-applied migration by recorded order.
        let avail = available(&["a", "b", "c"]);
        // `c` was applied while `b` was not: an out-of-order set for an Up run.
        let applied = tags(&["a", "c"]);
        let next = next_strict(Direction::Down, &avail, &applied, &no_skips())
            .unwrap()
            .expect("expected a down migration");
        assert_eq!(next.tag(), "c");
    }

    #[test]
    fn down_last_applied_skipped_earlier_unknown_errors() {
        // Down walks recorded order backwards skipping the run-skipped tags. When
        // the most-recently-applied tag is skipped and the next-back tag is not
        // among the available migrations, that unknown tag is the selection
        // target and Down errors with `MigrationNotFound` (Down does not consult
        // the Up-only unknown-tag opt-out).
        let avail = available(&["a", "b"]);
        // Recorded order: `x` (unknown) then `b`; `b` was skipped this run.
        let applied = tags(&["x", "b"]);
        match next_strict(Direction::Down, &avail, &applied, &skips(&["b"])) {
            Err(Error::MigrationNotFound(_)) => {}
            other => panic!(
                "expected MigrationNotFound for the unknown down target, got: {:?}",
                other.map(|o| o.map(|m| m.tag()))
            ),
        }
    }
}
