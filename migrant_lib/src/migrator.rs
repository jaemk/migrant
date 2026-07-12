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
            Direction::Down => match applied.last() {
                None => None,
                Some(tag) => {
                    let mig = available.iter().rev().find(|m| &m.tag() == tag);
                    match mig {
                        None => bail!(MigrationNotFound, "Tag not found: {}", tag),
                        Some(mig) => Some(mig.as_ref()),
                    }
                }
            },
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

        if self.fake {
            self.println("  ✓ (fake)");
        } else {
            let res = match self.direction {
                Direction::Up => next.apply_up(config),
                Direction::Down => next.apply_down(config),
            };
            match res {
                Ok(_) => self.println("  ✓"),
                Err(ref e) => {
                    self.println("");
                    if self.force {
                        self.println(&format!(
                            " ** Error ** (Continuing because `--force` flag was specified)\n ** {}",
                            e
                        ));
                    } else {
                        bail!(Migration, "Migration was unsucessful...\n{}", e);
                    }
                }
            }
        }

        let tag = next.tag();
        match self.direction {
            Direction::Up => config.insert_migration_tag(&tag)?,
            Direction::Down => config.delete_migration_tag(&tag)?,
        }
        Ok(())
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
