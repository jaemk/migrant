/*!
Migration application
*/
use std::collections::{HashMap, HashSet};
use std::fmt;

use crate::config::Config;
use crate::errors::*;
use crate::macros::bail;
use crate::migratable::{validate_migrations, Migratable};
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
    repeatable_tags: Vec<String>,
}

impl Report {
    fn new(direction: Direction) -> Self {
        Self {
            direction,
            tags: Vec::new(),
            repeatable_tags: Vec::new(),
        }
    }

    /// The direction this run applied migrations in.
    pub fn direction(&self) -> Direction {
        self.direction
    }

    /// The migration tags whose bookkeeping this run changed, in order.
    ///
    /// A tag can appear because a repeatable migration was *re-run* rather than
    /// applied for the first time, so this is not a count of newly-applied
    /// migrations. Use [`Report::repeatable_tags`] to tell the two apart.
    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    /// The repeatable migration tags this run re-ran, in order. A subset of
    /// [`Report::tags`]; always empty for a `Down` run, which never selects a
    /// repeatable migration.
    pub fn repeatable_tags(&self) -> &[String] {
        &self.repeatable_tags
    }

    /// Record a tag this run changed the bookkeeping of.
    fn record(&mut self, tag: String, repeatable: bool) {
        if repeatable {
            self.repeatable_tags.push(tag.clone());
        }
        self.tags.push(tag);
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
    /// force-recorded); carries its tag and whether it is repeatable.
    Applied { tag: String, repeatable: bool },
    /// A migration failed and was skipped under `ForceMode::SkipFailures`.
    Skipped,
    /// No further migration is available in this direction.
    Complete,
}

/// Which `Up`-run consistency checks are bypassed on a run. Grouped so the
/// selection helpers stay within argument-count limits and pass the set as a
/// unit.
#[derive(Debug, Clone, Copy)]
struct StrictChecks {
    allow_unknown_tags: bool,
    allow_out_of_order: bool,
    allow_checksum_mismatch: bool,
}

/// The recorded bookkeeping state that migration selection and the `Up`-run
/// consistency checks are decided against, grouped so it passes as a unit.
#[derive(Debug, Clone, Copy)]
struct AppliedState<'a> {
    /// Applied tags, in recorded application order
    tags: &'a [String],
    /// Checksum recorded per applied tag (`None` where the column is NULL)
    checksums: &'a HashMap<String, Option<String>>,
    /// Applied tags whose row is marked repeatable. Recognizes a recorded
    /// repeatable migration even when its tag is no longer available.
    repeatable: &'a HashSet<String>,
}

impl AppliedState<'_> {
    fn contains(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }

    /// Whether a repeatable migration needs to re-run: it has no row yet, or
    /// its current checksum differs from the recorded one.
    fn is_stale(&self, migration: &dyn Migratable) -> bool {
        match self.checksums.get(&migration.tag()) {
            None => true,
            Some(recorded) => recorded.as_deref() != migration.checksum().as_deref(),
        }
    }
}

/// Tags excluded from selection for the remainder of a run: those that failed
/// under `ForceMode::SkipFailures`, and the repeatable migrations already
/// re-run this run (a repeatable migration runs at most once per run, which
/// also keeps an `all` run from looping on one).
#[derive(Debug, Clone, Copy)]
struct RunExclusions<'a> {
    skipped: &'a HashSet<String>,
    ran_repeatable: &'a HashSet<String>,
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
    allow_checksum_mismatch: bool,
    rerun_repeatable: bool,
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
            allow_checksum_mismatch: false,
            rerun_repeatable: false,
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

    /// Allow an already-applied migration whose current checksum no longer
    /// matches the checksum recorded when it was applied. Default is `false`.
    ///
    /// By default an `Up` run aborts with [`Error::ChecksumMismatch`] before
    /// applying anything if an already-applied migration's up-SQL has changed
    /// since it was recorded -- the situation that arises when a migration file
    /// is edited after it has run somewhere. The comparison only fires when both
    /// the recorded and current checksums are present, so programmatic
    /// migrations (which record no checksum) and legacy rows recorded before
    /// checksums existed are never flagged. Set this to `true` to apply despite
    /// such drift. Independent of `allow_unknown_tags` and `allow_out_of_order`.
    pub fn allow_checksum_mismatch(mut self, allow: bool) -> Self {
        self.allow_checksum_mismatch = allow;
        self
    }

    /// Re-run every repeatable migration on this `Up` run, whether or not its
    /// checksum changed. Default is `false`.
    ///
    /// Normally a repeatable migration re-runs only when its up-SQL changed
    /// (see [`Migratable::is_repeatable`](crate::Migratable::is_repeatable)),
    /// so re-running an unedited one otherwise means touching its SQL. Set this
    /// to `true` to run them all regardless, for example to re-seed after
    /// restoring a database. Everything else is unchanged: they still run after
    /// the pending versioned migrations, still at most once per run, and each
    /// still records its checksum afterwards.
    ///
    /// Has no effect on a `Down` run, which never selects a repeatable
    /// migration.
    pub fn rerun_repeatable(mut self, rerun: bool) -> Self {
        self.rerun_repeatable = rerun;
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
        // Repeatable tags already re-run this run. A repeatable migration runs
        // at most once per run, so an `all` run cannot loop on one whose
        // bookkeeping did not end up matching (a `fake` or force-recorded run
        // still records, but this keeps the loop bounded regardless).
        let mut ran_repeatable = HashSet::new();
        let mut report = Report::new(self.direction);
        loop {
            self.check_lock_still_held(&config, lock_generation)?;
            match self.apply_next(&config, &mut skipped, &ran_repeatable, lock_generation)? {
                Step::Applied { tag, repeatable } => {
                    if repeatable {
                        ran_repeatable.insert(tag.clone());
                    }
                    report.record(tag, repeatable);
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
        let migrations: Vec<Box<dyn Migratable>> = match config.migrations {
            Some(ref migrations) => migrations.clone(),
            None => {
                let location = config.migration_location()?;
                ops::search_for_migrations(&location)?
                    .into_iter()
                    .map(|fm| fm.boxed())
                    .collect()
            }
        };
        // Explicit sets are validated when registered, but file-discovered ones
        // declare themselves repeatable through a directive in their SQL, so the
        // same rules are enforced here.
        validate_migrations(&migrations)?;
        Ok(migrations)
    }

    /// Return the next available up or down migration, excluding any tags
    /// skipped earlier in this run (`ForceMode::SkipFailures`) or already re-run
    /// this run (repeatable migrations).
    ///
    /// For an `Up` run this first enforces the strictness checks (unknown applied
    /// tags, out-of-order application, checksum drift) unless they have been
    /// opted out of. Pending versioned migrations are selected first, in
    /// definition order; only once none remain are the stale repeatable
    /// migrations selected, also in definition order. `rerun_repeatable` treats
    /// every repeatable migration as stale.
    fn next_available<'a>(
        direction: Direction,
        available: &'a [Box<dyn Migratable>],
        state: AppliedState<'_>,
        exclusions: RunExclusions<'_>,
        checks: StrictChecks,
        rerun_repeatable: bool,
    ) -> Result<Option<&'a dyn Migratable>> {
        Ok(match direction {
            Direction::Up => {
                Self::check_applied_consistency(available, state, exclusions.skipped, checks)?;
                let pending = available.iter().find(|m| {
                    !m.is_repeatable()
                        && !state.contains(&m.tag())
                        && !exclusions.skipped.contains(&m.tag())
                });
                if let Some(pending) = pending {
                    return Ok(Some(pending.as_ref()));
                }
                // Every pending versioned migration has been applied, so the
                // repeatable migrations whose SQL changed (or that have never
                // run) go next. `rerun_repeatable` takes them all.
                available
                    .iter()
                    .find(|m| {
                        m.is_repeatable()
                            && !exclusions.skipped.contains(&m.tag())
                            && !exclusions.ran_repeatable.contains(&m.tag())
                            && (rerun_repeatable || state.is_stale(m.as_ref()))
                    })
                    .map(AsRef::as_ref)
            }
            Direction::Down => {
                // Recorded application order is authoritative, so the most
                // recently applied migration is the last applied tag. Walk
                // backwards, skipping tags that failed earlier this run, and
                // return the corresponding available migration. A target tag
                // absent from the available set is a hard error, matching the
                // previous behavior.
                for tag in state.tags.iter().rev() {
                    if exclusions.skipped.contains(tag) {
                        continue;
                    }
                    match available.iter().find(|m| &m.tag() == tag) {
                        // Repeatable migrations are forward-only: a `Down` run
                        // neither reverts them nor removes their bookkeeping
                        // row. The available set is authoritative on kind for a
                        // tag it still defines, so a migration converted back to
                        // versioned is reverted normally despite what its row
                        // recorded.
                        Some(m) if m.is_repeatable() => continue,
                        Some(m) => return Ok(Some(m.as_ref())),
                        // Absent from the available set, so only the recorded
                        // row says what kind it was: a repeatable one is not
                        // part of the versioned sequence and is skipped rather
                        // than treated as a missing down target.
                        None if state.repeatable.contains(tag) => continue,
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
        state: AppliedState<'_>,
        skipped: &HashSet<String>,
        checks: StrictChecks,
    ) -> Result<()> {
        if !checks.allow_unknown_tags {
            for tag in state.tags {
                if skipped.contains(tag) {
                    continue;
                }
                // A repeatable tag is not part of the versioned sequence, so a
                // recorded repeatable row is never an unknown versioned tag
                // even once it is gone from the available set.
                if state.repeatable.contains(tag) {
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
        if !checks.allow_out_of_order {
            // Walk definition order tracking the first un-applied (and un-skipped)
            // migration. An applied migration appearing after it was applied out
            // of order.
            let mut first_unapplied: Option<String> = None;
            for m in available {
                let tag = m.tag();
                if skipped.contains(&tag) {
                    continue;
                }
                // Repeatable migrations run after the versioned sequence and
                // re-run repeatedly, so they take no part in its ordering.
                if m.is_repeatable() {
                    continue;
                }
                if state.contains(&tag) {
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
        if !checks.allow_checksum_mismatch {
            // Compare each already-applied migration's current checksum against
            // the checksum recorded when it was applied. The checksum map is
            // keyed by applied tag, so a lookup hit means the migration is both
            // applied and available. The comparison only fires when both the
            // recorded and current checksums are present -- a null on either
            // side (programmatic migration, or a legacy/backfilled row) is not a
            // mismatch.
            for m in available {
                let tag = m.tag();
                if skipped.contains(&tag) {
                    continue;
                }
                // For a repeatable migration a changed checksum is the signal to
                // re-run, not drift. This walks the available migrations, which
                // are authoritative on kind: a migration converted back to
                // versioned is drift-checked again even though its recorded row
                // still says repeatable.
                if m.is_repeatable() {
                    continue;
                }
                if let Some(Some(recorded)) = state.checksums.get(&tag) {
                    if let Some(current) = m.checksum() {
                        if *recorded != current {
                            bail!(
                                ChecksumMismatch,
                                "Migration `{}` has changed since it was applied: recorded \
                                 checksum `{}` does not match its current checksum `{}`. Revert \
                                 the change, or pass `allow_checksum_mismatch(true)` to apply \
                                 despite the drift.",
                                tag,
                                recorded,
                                current
                            )
                        }
                    }
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
        ran_repeatable: &HashSet<String>,
        lock_generation: Option<u64>,
    ) -> Result<Step> {
        let migrations = Self::available_migrations(config)?;
        let next = match Self::next_available(
            self.direction,
            &migrations,
            AppliedState {
                tags: &config.applied,
                checksums: &config.recorded_checksums,
                repeatable: &config.recorded_repeatable,
            },
            RunExclusions {
                skipped,
                ran_repeatable,
            },
            StrictChecks {
                allow_unknown_tags: self.allow_unknown_tags,
                allow_out_of_order: self.allow_out_of_order,
                allow_checksum_mismatch: self.allow_checksum_mismatch,
            },
            self.rerun_repeatable,
        )? {
            Some(next) => next,
            None => return Ok(Step::Complete),
        };

        let tag = next.tag();
        let repeatable = next.is_repeatable();
        // A repeatable migration that already has a bookkeeping row is being
        // re-run rather than applied for the first time.
        let verb = if repeatable && config.applied.contains(&tag) {
            "Re-applying"
        } else {
            "Applying"
        };
        self.print(&format!(
            "{}[{}]: {}",
            verb,
            self.direction,
            next.description(self.direction)
        ));

        if self.fake {
            self.println("  ✓ (fake)");
            self.record_tag(config, next)?;
            return Ok(Step::Applied { tag, repeatable });
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
                Ok(Step::Applied { tag, repeatable })
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
                        Ok(Step::Applied { tag, repeatable })
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
    /// checksum (`None` for programmatic migrations, stored as NULL) and
    /// whether it is repeatable.
    ///
    /// A repeatable migration that already has a row is updated in place rather
    /// than inserted again, so it keeps one row (and its recorded application
    /// order) across re-runs.
    fn record_tag(&self, config: &Config, next: &dyn Migratable) -> Result<()> {
        let tag = next.tag();
        let checksum = next.checksum();
        match self.direction {
            Direction::Up if next.is_repeatable() && config.applied.contains(&tag) => {
                config.update_migration_tag(&tag, checksum.as_deref())
            }
            Direction::Up => {
                config.insert_migration_tag(&tag, checksum.as_deref(), next.is_repeatable())
            }
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

    /// Empty recorded-checksum map: tag-only embedded migrations record no
    /// checksum, so the drift check is a no-op for these selection tests.
    fn no_checksums() -> HashMap<String, Option<String>> {
        HashMap::new()
    }

    /// No applied row is marked repeatable.
    fn no_repeatable() -> HashSet<String> {
        HashSet::new()
    }

    /// All consistency checks enabled (no opt-outs) -- the migrator default.
    fn all_checks() -> StrictChecks {
        StrictChecks {
            allow_unknown_tags: false,
            allow_out_of_order: false,
            allow_checksum_mismatch: false,
        }
    }

    /// Nothing excluded: no run-skipped tags and no repeatable migration run yet.
    fn no_exclusions<'a>(
        skipped: &'a HashSet<String>,
        ran_repeatable: &'a HashSet<String>,
    ) -> RunExclusions<'a> {
        RunExclusions {
            skipped,
            ran_repeatable,
        }
    }

    /// Strict selection with all opt-outs off -- the default the migrator uses.
    fn next_strict<'a>(
        direction: Direction,
        available: &'a [Box<dyn Migratable>],
        applied: &[String],
        skipped: &HashSet<String>,
    ) -> Result<Option<&'a dyn Migratable>> {
        let checksums = no_checksums();
        let repeatable = no_repeatable();
        let ran = no_skips();
        Migrator::next_available(
            direction,
            available,
            AppliedState {
                tags: applied,
                checksums: &checksums,
                repeatable: &repeatable,
            },
            no_exclusions(skipped, &ran),
            all_checks(),
            false,
        )
    }

    /// Selection with a specific set of check opt-outs, nothing else excluded.
    fn next_with_checks<'a>(
        direction: Direction,
        available: &'a [Box<dyn Migratable>],
        applied: &[String],
        checks: StrictChecks,
    ) -> Result<Option<&'a dyn Migratable>> {
        let checksums = no_checksums();
        let repeatable = no_repeatable();
        let skipped = no_skips();
        let ran = no_skips();
        Migrator::next_available(
            direction,
            available,
            AppliedState {
                tags: applied,
                checksums: &checksums,
                repeatable: &repeatable,
            },
            no_exclusions(&skipped, &ran),
            checks,
            false,
        )
    }

    /// Selection against a full recorded state, for the repeatable cases where
    /// the recorded checksums decide what runs next.
    fn next_with_state<'a>(
        direction: Direction,
        available: &'a [Box<dyn Migratable>],
        applied: &[String],
        checksums: &HashMap<String, Option<String>>,
        ran_repeatable: &HashSet<String>,
    ) -> Result<Option<&'a dyn Migratable>> {
        next_with_state_rerunning(
            direction,
            available,
            applied,
            checksums,
            ran_repeatable,
            false,
        )
    }

    /// As `next_with_state`, with control over the `rerun_repeatable` override.
    fn next_with_state_rerunning<'a>(
        direction: Direction,
        available: &'a [Box<dyn Migratable>],
        applied: &[String],
        checksums: &HashMap<String, Option<String>>,
        ran_repeatable: &HashSet<String>,
        rerun_repeatable: bool,
    ) -> Result<Option<&'a dyn Migratable>> {
        let repeatable = no_repeatable();
        let skipped = no_skips();
        Migrator::next_available(
            direction,
            available,
            AppliedState {
                tags: applied,
                checksums,
                repeatable: &repeatable,
            },
            no_exclusions(&skipped, ran_repeatable),
            all_checks(),
            rerun_repeatable,
        )
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
        let next = next_with_checks(
            Direction::Up,
            &avail,
            &applied,
            StrictChecks {
                allow_unknown_tags: true,
                ..all_checks()
            },
        )
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
        let next = next_with_checks(
            Direction::Up,
            &avail,
            &applied,
            StrictChecks {
                allow_out_of_order: true,
                ..all_checks()
            },
        )
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
        match next_with_checks(
            Direction::Up,
            &avail,
            &applied,
            StrictChecks {
                allow_unknown_tags: true,
                ..all_checks()
            },
        ) {
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
        match next_with_checks(
            Direction::Up,
            &avail,
            &applied,
            StrictChecks {
                allow_out_of_order: true,
                ..all_checks()
            },
        ) {
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

    /// Available migrations carrying up-SQL, so each reports a real `checksum()`.
    fn available_with_up(pairs: &[(&str, &str)]) -> Vec<Box<dyn Migratable>> {
        pairs
            .iter()
            .map(|(tag, up)| EmbeddedMigration::with_tag(tag).up(up.to_string()).boxed())
            .collect()
    }

    fn recorded(pairs: &[(&str, Option<&str>)]) -> HashMap<String, Option<String>> {
        pairs
            .iter()
            .map(|(tag, sum)| ((*tag).to_owned(), sum.map(|s| s.to_owned())))
            .collect()
    }

    /// Run the Up consistency checks with only the drift check able to fire.
    fn check_drift(
        available: &[Box<dyn Migratable>],
        applied: &[String],
        recorded: &HashMap<String, Option<String>>,
        allow_checksum_mismatch: bool,
    ) -> Result<()> {
        let repeatable = no_repeatable();
        Migrator::check_applied_consistency(
            available,
            AppliedState {
                tags: applied,
                checksums: recorded,
                repeatable: &repeatable,
            },
            &no_skips(),
            StrictChecks {
                allow_checksum_mismatch,
                ..all_checks()
            },
        )
    }

    #[test]
    fn up_checksum_mismatch_aborts_by_default() {
        let avail = available_with_up(&[("a", "select 1;")]);
        let applied = tags(&["a"]);
        // `a` was recorded with a different checksum than its current up-SQL.
        let recorded = recorded(&[("a", Some("stale-checksum"))]);
        match check_drift(&avail, &applied, &recorded, false) {
            Err(Error::ChecksumMismatch(msg)) => {
                assert!(
                    msg.contains("a"),
                    "message should name the drifted tag: {msg}"
                );
            }
            other => panic!("expected ChecksumMismatch, got: {other:?}"),
        }
    }

    #[test]
    fn up_checksum_match_passes() {
        let avail = available_with_up(&[("a", "select 1;")]);
        let applied = tags(&["a"]);
        // Record the migration's actual current checksum: no drift.
        let current = avail[0].checksum().expect("embedded up-SQL has a checksum");
        let recorded = recorded(&[("a", Some(&current))]);
        check_drift(&avail, &applied, &recorded, false).expect("matching checksum must pass");
    }

    #[test]
    fn up_null_recorded_checksum_skips_drift_check() {
        // A recorded NULL checksum (programmatic migration, or a legacy row) is
        // not comparable, so it is skipped rather than treated as a mismatch.
        let avail = available_with_up(&[("a", "select 1;")]);
        let applied = tags(&["a"]);
        let recorded = recorded(&[("a", None)]);
        check_drift(&avail, &applied, &recorded, false).expect("null recorded checksum skips");
    }

    #[test]
    fn up_null_current_checksum_skips_drift_check() {
        // A tag-only migration reports no current checksum, so there is nothing
        // to compare against a recorded value: skipped, not a mismatch.
        let avail = available(&["a"]);
        let applied = tags(&["a"]);
        let recorded = recorded(&[("a", Some("anything"))]);
        check_drift(&avail, &applied, &recorded, false).expect("null current checksum skips");
    }

    #[test]
    fn up_checksum_mismatch_allowed_when_opted_out() {
        let avail = available_with_up(&[("a", "select 1;")]);
        let applied = tags(&["a"]);
        let recorded = recorded(&[("a", Some("stale-checksum"))]);
        check_drift(&avail, &applied, &recorded, true)
            .expect("allow_checksum_mismatch applies despite drift");
    }

    #[test]
    fn up_checksum_drift_of_unapplied_migration_is_ignored() {
        // Drift only concerns already-applied migrations. A recorded checksum for
        // a tag that is not applied (absent from the applied set, so not in the
        // recorded map) never fires the check.
        let avail = available_with_up(&[("a", "select 1;"), ("b", "select 2;")]);
        let applied = tags(&["a"]);
        let current = avail[0].checksum().unwrap();
        let recorded = recorded(&[("a", Some(&current))]);
        // `b` is pending; its checksum is irrelevant to drift.
        check_drift(&avail, &applied, &recorded, false).expect("pending migration is not checked");
    }

    /// A mixed available set: versioned migrations plus repeatable ones,
    /// identified by the trailing `repeatable` flag of each entry.
    fn available_mixed(entries: &[(&str, &str, bool)]) -> Vec<Box<dyn Migratable>> {
        entries
            .iter()
            .map(|(tag, up, repeatable)| {
                let m = EmbeddedMigration::with_tag(tag).up(up.to_string());
                if *repeatable { m.repeatable() } else { m }.boxed()
            })
            .collect()
    }

    /// The current checksum of the available migration with the given tag.
    fn current_sum(available: &[Box<dyn Migratable>], tag: &str) -> String {
        available
            .iter()
            .find(|m| m.tag() == tag)
            .expect("tag is available")
            .checksum()
            .expect("embedded up-SQL has a checksum")
    }

    // REPEAT-5
    #[test]
    fn up_runs_pending_versioned_migrations_before_repeatable_ones() {
        // `seed` is repeatable and stale (never run), but the pending versioned
        // `b` must still be selected first: repeatables run after the versioned
        // sequence even when they come earlier in definition order.
        let avail = available_mixed(&[
            ("seed", "insert into roles values ('admin');", true),
            ("a", "select 1;", false),
            ("b", "select 2;", false),
        ]);
        let applied = tags(&["a"]);
        let checksums = recorded(&[("a", Some(&current_sum(&avail, "a")))]);
        let next = next_with_state(Direction::Up, &avail, &applied, &checksums, &no_skips())
            .unwrap()
            .expect("expected a migration");
        assert_eq!(next.tag(), "b");
    }

    // REPEAT-1
    #[test]
    fn up_runs_a_repeatable_migration_that_has_never_run() {
        let avail = available_mixed(&[("a", "select 1;", false), ("seed", "select 2;", true)]);
        let applied = tags(&["a"]);
        // Only `a` has a row; `seed` has never run, so it is stale.
        let checksums = recorded(&[("a", Some(&current_sum(&avail, "a")))]);
        let next = next_with_state(Direction::Up, &avail, &applied, &checksums, &no_skips())
            .unwrap()
            .expect("expected the repeatable migration");
        assert_eq!(next.tag(), "seed");
    }

    // REPEAT-1
    #[test]
    fn up_reruns_a_repeatable_migration_whose_checksum_changed() {
        let avail = available_mixed(&[("a", "select 1;", false), ("seed", "select 2;", true)]);
        let applied = tags(&["a", "seed"]);
        // `seed` ran before with different SQL: its recorded checksum no longer
        // matches, which is the signal to re-run it (not drift).
        let checksums = recorded(&[
            ("a", Some(&current_sum(&avail, "a"))),
            ("seed", Some("checksum-of-the-old-sql")),
        ]);
        let next = next_with_state(Direction::Up, &avail, &applied, &checksums, &no_skips())
            .unwrap()
            .expect("expected the repeatable migration to re-run");
        assert_eq!(next.tag(), "seed");
    }

    // REPEAT-1
    #[test]
    fn up_skips_a_repeatable_migration_whose_checksum_is_unchanged() {
        let avail = available_mixed(&[("a", "select 1;", false), ("seed", "select 2;", true)]);
        let applied = tags(&["a", "seed"]);
        let checksums = recorded(&[
            ("a", Some(&current_sum(&avail, "a"))),
            ("seed", Some(&current_sum(&avail, "seed"))),
        ]);
        let next =
            next_with_state(Direction::Up, &avail, &applied, &checksums, &no_skips()).unwrap();
        assert!(
            next.is_none(),
            "an up-to-date repeatable migration must not re-run"
        );
    }

    // REPEAT-12
    #[test]
    fn rerun_repeatable_runs_an_unchanged_repeatable_migration() {
        let avail = available_mixed(&[("a", "select 1;", false), ("seed", "select 2;", true)]);
        let applied = tags(&["a", "seed"]);
        // Both checksums match, so nothing is stale.
        let checksums = recorded(&[
            ("a", Some(&current_sum(&avail, "a"))),
            ("seed", Some(&current_sum(&avail, "seed"))),
        ]);
        assert!(
            next_with_state(Direction::Up, &avail, &applied, &checksums, &no_skips())
                .unwrap()
                .is_none(),
            "nothing is stale without the override"
        );

        let next = next_with_state_rerunning(
            Direction::Up,
            &avail,
            &applied,
            &checksums,
            &no_skips(),
            true,
        )
        .unwrap()
        .expect("the override re-runs it regardless of checksum");
        assert_eq!(next.tag(), "seed");
    }

    // REPEAT-12
    #[test]
    fn rerun_repeatable_still_runs_each_at_most_once_and_leaves_versioned_alone() {
        let avail = available_mixed(&[("a", "select 1;", false), ("seed", "select 2;", true)]);
        // `a` is not applied, so it is selected first even with the override:
        // the override only makes repeatable migrations eligible, it does not
        // reorder the run or re-apply versioned migrations.
        let next = next_with_state_rerunning(
            Direction::Up,
            &avail,
            &tags(&[]),
            &no_checksums(),
            &no_skips(),
            true,
        )
        .unwrap()
        .expect("expected the pending versioned migration");
        assert_eq!(next.tag(), "a");

        // An applied versioned migration is never re-selected by the override.
        let applied = tags(&["a", "seed"]);
        let checksums = recorded(&[
            ("a", Some(&current_sum(&avail, "a"))),
            ("seed", Some(&current_sum(&avail, "seed"))),
        ]);
        // Once `seed` has run this run, the override does not select it again,
        // so an `all` run still terminates.
        let ran = skips(&["seed"]);
        assert!(
            next_with_state_rerunning(Direction::Up, &avail, &applied, &checksums, &ran, true)
                .unwrap()
                .is_none(),
            "the override must not defeat the once-per-run guard"
        );
    }

    // REPEAT-12
    #[test]
    fn rerun_repeatable_has_no_effect_on_a_down_run() {
        let avail = available_mixed(&[("a", "select 1;", false), ("seed", "select 2;", true)]);
        let applied = tags(&["a", "seed"]);
        let next = next_with_state_rerunning(
            Direction::Down,
            &avail,
            &applied,
            &no_checksums(),
            &no_skips(),
            true,
        )
        .unwrap()
        .expect("expected a down migration");
        assert_eq!(
            next.tag(),
            "a",
            "a Down run never selects a repeatable migration, override or not"
        );
    }

    // REPEAT-1
    #[test]
    fn up_runs_a_repeatable_migration_at_most_once_per_run() {
        // Having re-run `seed` this run, it is excluded for the rest of the run
        // even though the recorded checksum here still looks stale. This is what
        // keeps an `all` run from looping on a single repeatable migration.
        let avail = available_mixed(&[("seed", "select 2;", true)]);
        let applied = tags(&["seed"]);
        let checksums = recorded(&[("seed", Some("stale"))]);
        let ran = skips(&["seed"]);
        let next = next_with_state(Direction::Up, &avail, &applied, &checksums, &ran).unwrap();
        assert!(next.is_none(), "a repeatable migration runs once per run");
    }

    // REPEAT-5
    #[test]
    fn up_runs_stale_repeatable_migrations_in_definition_order() {
        let avail =
            available_mixed(&[("seed-b", "select 2;", true), ("seed-a", "select 1;", true)]);
        // Neither has ever run, so both are stale; definition order decides.
        let next = next_with_state(
            Direction::Up,
            &avail,
            &tags(&[]),
            &no_checksums(),
            &no_skips(),
        )
        .unwrap()
        .expect("expected a repeatable migration");
        assert_eq!(next.tag(), "seed-b");
    }

    // REPEAT-4
    #[test]
    fn down_never_selects_a_repeatable_migration() {
        let avail = available_mixed(&[("a", "select 1;", false), ("seed", "select 2;", true)]);
        // `seed` was applied most recently, but a Down run walks past it to the
        // versioned `a`.
        let applied = tags(&["a", "seed"]);
        let next = next_with_state(
            Direction::Down,
            &avail,
            &applied,
            &no_checksums(),
            &no_skips(),
        )
        .unwrap()
        .expect("expected a down migration");
        assert_eq!(next.tag(), "a");
    }

    // REPEAT-4
    #[test]
    fn down_with_only_repeatable_migrations_applied_returns_none() {
        let avail = available_mixed(&[("seed", "select 2;", true)]);
        let applied = tags(&["seed"]);
        let next = next_with_state(
            Direction::Down,
            &avail,
            &applied,
            &no_checksums(),
            &no_skips(),
        )
        .unwrap();
        assert!(
            next.is_none(),
            "there is nothing to revert when only repeatable rows are recorded"
        );
    }

    /// Run the Up consistency checks against a state that also carries the
    /// applied tags recorded as repeatable.
    fn check_with_recorded_repeatable(
        available: &[Box<dyn Migratable>],
        applied: &[String],
        checksums: &HashMap<String, Option<String>>,
        recorded_repeatable: &HashSet<String>,
    ) -> Result<()> {
        Migrator::check_applied_consistency(
            available,
            AppliedState {
                tags: applied,
                checksums,
                repeatable: recorded_repeatable,
            },
            &no_skips(),
            all_checks(),
        )
    }

    // REPEAT-2
    #[test]
    fn repeatable_checksum_change_is_not_drift() {
        // The same recorded-vs-current mismatch that aborts a versioned run is
        // the expected re-run signal for a repeatable migration, so the drift
        // check must not fire on it.
        let avail = available_mixed(&[("seed", "select 2;", true)]);
        let applied = tags(&["seed"]);
        let checksums = recorded(&[("seed", Some("checksum-of-the-old-sql"))]);
        check_with_recorded_repeatable(&avail, &applied, &checksums, &no_repeatable())
            .expect("a repeatable migration's changed checksum is not drift");
    }

    // REPEAT-5
    #[test]
    fn repeatable_migrations_do_not_trigger_the_ordering_check() {
        // `seed` is repeatable, applied, and sits before the un-applied `b` in
        // definition order. For a versioned migration that is an out-of-order
        // violation; a repeatable one takes no part in the versioned sequence.
        let avail = available_mixed(&[
            ("a", "select 1;", false),
            ("seed", "select 2;", true),
            ("b", "select 3;", false),
        ]);
        let applied = tags(&["seed"]);
        let checksums = recorded(&[("seed", Some(&current_sum(&avail, "seed")))]);
        check_with_recorded_repeatable(&avail, &applied, &checksums, &no_repeatable())
            .expect("a repeatable migration is not part of the versioned ordering");
    }

    // REPEAT-5
    #[test]
    fn a_removed_repeatable_tag_is_not_an_unknown_tag() {
        // `seed` was recorded as repeatable and has since been dropped from the
        // available set. Its row is recognized by the `is_repeatable` column, so
        // it is not reported as an unknown versioned tag.
        let avail = available_mixed(&[("a", "select 1;", false)]);
        let applied = tags(&["a", "seed"]);
        let checksums = recorded(&[("a", Some(&current_sum(&avail, "a")))]);
        let recorded_repeatable = skips(&["seed"]);
        check_with_recorded_repeatable(&avail, &applied, &checksums, &recorded_repeatable)
            .expect("a recorded repeatable row is never an unknown versioned tag");

        // A removed *versioned* tag is still an error, so the exemption is not
        // blanket.
        match check_with_recorded_repeatable(&avail, &applied, &checksums, &no_repeatable()) {
            Err(Error::MigrationNotFound(_)) => {}
            other => panic!("expected MigrationNotFound for a removed versioned tag: {other:?}"),
        }
    }

    // REPEAT-6
    #[test]
    fn the_available_set_outranks_a_stale_recorded_kind() {
        // A migration recorded as repeatable that is now declared versioned
        // must be drift-checked again: the available set is authoritative on
        // kind for a tag it still defines, so the recorded flag cannot become a
        // one-way door that disables drift detection forever.
        let avail = available_with_up(&[("a", "select 1;")]);
        let applied = tags(&["a"]);
        let recorded = recorded(&[("a", Some("checksum-from-when-it-was-repeatable"))]);
        let still_marked_repeatable = skips(&["a"]);
        match check_with_recorded_repeatable(&avail, &applied, &recorded, &still_marked_repeatable)
        {
            Err(Error::ChecksumMismatch(_)) => {}
            other => panic!("a now-versioned migration must be drift-checked: {other:?}"),
        }
    }

    // REPEAT-4, REPEAT-6
    #[test]
    fn down_reverts_a_migration_converted_back_to_versioned() {
        // Mirror case for selection: the row still says repeatable, but the
        // available set now declares it versioned, so Down targets it.
        let avail = available_mixed(&[("a", "select 1;", false), ("seed", "select 2;", false)]);
        let applied = tags(&["a", "seed"]);
        let checksums = no_checksums();
        let still_marked_repeatable = skips(&["seed"]);
        let skipped = no_skips();
        let ran = no_skips();
        let next = Migrator::next_available(
            Direction::Down,
            &avail,
            AppliedState {
                tags: &applied,
                checksums: &checksums,
                repeatable: &still_marked_repeatable,
            },
            no_exclusions(&skipped, &ran),
            all_checks(),
            false,
        )
        .unwrap()
        .expect("expected a down migration");
        assert_eq!(
            next.tag(),
            "seed",
            "the available set decides kind for a tag it still defines"
        );
    }

    // REPEAT-5
    #[test]
    fn down_skips_a_removed_repeatable_tag_instead_of_erroring() {
        // The tag is gone from the available set, so only its recorded row says
        // what it was. A repeatable one is skipped rather than raising
        // MigrationNotFound for a down target that never existed.
        let avail = available_mixed(&[("a", "select 1;", false)]);
        let applied = tags(&["a", "seed"]);
        let checksums = no_checksums();
        let recorded_repeatable = skips(&["seed"]);
        let skipped = no_skips();
        let ran = no_skips();
        let next = Migrator::next_available(
            Direction::Down,
            &avail,
            AppliedState {
                tags: &applied,
                checksums: &checksums,
                repeatable: &recorded_repeatable,
            },
            no_exclusions(&skipped, &ran),
            all_checks(),
            false,
        )
        .unwrap()
        .expect("expected a down migration");
        assert_eq!(next.tag(), "a");
    }

    // REPEAT-8
    #[test]
    fn report_separates_repeatable_tags_from_the_full_tag_list() {
        let mut report = Report::new(Direction::Up);
        report.record("a".to_string(), false);
        report.record("seed".to_string(), true);
        assert_eq!(report.tags(), ["a", "seed"]);
        assert_eq!(report.repeatable_tags(), ["seed"]);
        assert_eq!(report.len(), 2);
        assert!(!report.is_empty());
    }
}
