#![recursion_limit = "1024"]
#[macro_use] extern crate error_chain;
#[macro_use] extern crate lazy_static;
#[macro_use] extern crate serde_derive;
extern crate serde;
extern crate toml;
extern crate chrono;
extern crate walkdir;
extern crate regex;
extern crate percent_encoding;
extern crate url;

#[cfg(feature="postgresql")]
extern crate postgres;

#[cfg(feature="sqlite")]
extern crate rusqlite;

use std::collections::HashMap;
use std::process::Command;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::ffi::OsStr;
use std::fs;
use std::fmt;
use std::env;

use percent_encoding::{percent_encode, DEFAULT_ENCODE_SET};
use chrono::{TimeZone, Utc};
use walkdir::WalkDir;
use regex::Regex;

#[macro_use] mod macros;
mod errors;
mod drivers;
mod migratable;
mod connection;
mod config;
mod migration;

pub mod types;

pub use errors::*;
pub use migratable::Migratable;
pub use config::{ConfigInitializer, Config};
pub use migration::{FileMigration, FnMigration};
pub use connection::DbConn;


static CONFIG_FILE: &'static str = ".migrant.toml";
static DT_FORMAT: &'static str = "%Y%m%d%H%M%S";


static SQLITE_CONFIG_TEMPLATE: &'static str = r#"
# required, do not edit
database_type = "sqlite"

# required: relative path to your database file from this config file dir: `__CONFIG_DIR__/`
# ex.) database_name = "db/db.db"
database_name = ""

migration_location = "migrations"  # default "migrations"
"#;


static PG_CONFIG_TEMPLATE: &'static str = r#"
# required, do not edit
database_type = "postgres"

database_name = ""      # required
database_user = ""      # required
database_password = ""

database_host = "localhost"         # default "localhost"
database_port = "5432"              # default "5432"
migration_location = "migrations"   # default "migrations"

# with the format:
# [database_params]
# key = "value"
[database_params]

"#;


lazy_static! {
    // For verifying new `tag` names
    static ref BAD_TAG_RE: Regex = Regex::new(r"[^a-z0-9-]+").expect("failed to compile regex");

    // For verifying complete stamp+tag names
    static ref FULL_TAG_RE: Regex = Regex::new(r"[0-9]{14}_[a-z0-9-]+").expect("failed to compile regex");
}


/// Write the provided bytes to the specified path
fn write_to_path(path: &Path, content: &[u8]) -> Result<()> {
    let mut file = fs::File::create(path)?;
    file.write_all(content)?;
    Ok(())
}


/// Run the given command in the foreground
fn open_file_in_fg(command: &str, file_path: &str) -> Result<()> {
    let mut p = Command::new(command)
        .arg(file_path)
        .spawn()?;
    let ret = p.wait()?;
    if !ret.success() { bail_fmt!(ErrorKind::ShellCommand, "Command `{}` exited with status `{}`", command, ret) }
    Ok(())
}


/// Percent encode a string
fn encode(s: &str) -> String {
    percent_encode(s.as_bytes(), DEFAULT_ENCODE_SET).to_string()
}


/// Prompt the user and return their input
fn prompt(msg: &str) -> Result<String> {
    print!("{}", msg);
    io::stdout().flush()?;
    let mut resp = String::new();
    io::stdin().read_line(&mut resp)?;
    Ok(resp.trim().to_string())
}


#[derive(Debug, Clone)]
/// Represents direction to apply migrations.
/// `Up`   -> up.sql
/// `Down` -> down.sql
pub enum Direction {
    Up,
    Down,
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use Direction::*;
        match *self {
            Up   => write!(f, "Up"),
            Down => write!(f, "Down"),
        }
    }
}


#[derive(Debug, Clone)]
/// Migration applicator
pub struct Migrator {
    config: Config,
    direction: Direction,
    force: bool,
    fake: bool,
    all: bool,
}

impl Migrator {
    /// Initialize a new `Migrator` with a given config
    pub fn with_config(config: &Config) -> Self {
        Self {
            config: config.clone(),
            direction: Direction::Up,
            force: false,
            fake: false,
            all: false,
        }
    }

    /// Set `direction`. Default is `Up`.
    /// `Up`   => run `up.sql`.
    /// `Down` => run `down.sql`.
    pub fn direction(mut self, dir: Direction) -> Self {
        self.direction = dir;
        self
    }

    /// Set `force` to forcefully apply migrations regardless of errors
    pub fn force(mut self, force: bool) -> Self {
        self.force = force;
        self
    }

    /// Set `fake` to fake application of migrations.
    /// `Config` will be updated as if migrations were actually run.
    pub fn fake(mut self, fake: bool) -> Self {
        self.fake = fake;
        self
    }

    /// Set `all` to run all remaining available migrations in the given `direction`
    pub fn all(mut self, all: bool) -> Self {
        self.all = all;
        self
    }

    /// Apply migrations using current configuration
    pub fn apply(self) -> Result<()> {
        apply_migration(
            &self.config, self.direction,
            self.force, self.fake, self.all,
            )
    }
}


/// Search for a `.migrant.toml` file in the current and parent directories
pub fn search_for_config(base: &PathBuf) -> Option<PathBuf> {
    let mut base = base.clone();
    loop {
        for path in fs::read_dir(&base).unwrap() {
            let path = path.unwrap().path();
            if let Some(file) = path.file_name() {
                if file == CONFIG_FILE { return Some(path.clone()); }
            }
        }
        if base.parent().is_some() {
            base.pop();
        } else {
            return None;
        }
    }
}


/// Search for available migrations in the given migration directory
///
/// Intended only for use with `FileMigration`s not managed directly in source
/// with `Config::use_migrations`.
fn search_for_migrations(mig_root: &PathBuf) -> Result<Vec<FileMigration>> {
    // collect any .sql files into a Map<`stamp-tag`, Vec<up&down files>>
    let mut files = HashMap::new();
    for dir in WalkDir::new(mig_root) {
        if dir.is_err() { break; }
        let e = dir.unwrap();
        let path = e.path();
        if let Some(ext) = path.extension() {
            if ext.is_empty() || ext != "sql" { continue; }
            let parent = path.parent().unwrap();
            let key = format!("{}", parent.display());
            let entry = files.entry(key).or_insert_with(Vec::new);
            entry.push(path.to_path_buf());
        }
    }

    // transform up&down files into a Vec<Migration>
    let mut migrations = vec![];
    for (path, migs) in &files {
        let full_name = PathBuf::from(path);
        let mut full_name = full_name.file_name()
            .and_then(OsStr::to_str)
            .ok_or_else(|| format_err!(ErrorKind::PathError, "Error extracting file-name from: {:?}", full_name))?
            .split('_');
        let stamp = full_name.next()
            .ok_or_else(|| format_err!(ErrorKind::TagError, "Invalid tag format: {:?}", full_name))?;
        let stamp = Utc.datetime_from_str(stamp, DT_FORMAT)?;
        let tag = full_name.next()
            .ok_or_else(|| format_err!(ErrorKind::TagError, "Invalid tag format: {:?}", full_name))?;

        let mut up = None;
        let mut down = None;

        for mig in migs.iter() {
            let up_down = mig.file_stem()
                .and_then(OsStr::to_str)
                .ok_or_else(|| format_err!(ErrorKind::PathError, "Error extracting file-stem from: {:?}", full_name))?;
            match up_down {
                "up" => up = Some(mig.clone()),
                "down" => down = Some(mig.clone()),
                _ => unreachable!(),
            };
        }
        if up.is_none() {
            bail_fmt!(ErrorKind::MigrationNotFound, "Up migration not found for tag: {}", tag)
        }
        if down.is_none() {
            bail_fmt!(ErrorKind::MigrationNotFound, "Down migration not found for tag: {}", tag)
        }
        migrations.push(FileMigration {
            up: up,
            down: down,
            tag: tag.to_owned(),
            stamp: Some(stamp),
        });
    }

    // sort by timestamps chronologically
    migrations.sort_by(|a, b| a.stamp.unwrap().cmp(&b.stamp.unwrap()));
    Ok(migrations)
}


/// Return the next available up or down migration
fn next_available<'a>(direction: &Direction, available: &'a [Box<Migratable>], applied: &[String]) -> Result<Option<&'a Box<Migratable>>> {
    Ok(match *direction {
        Direction::Up => {
            for mig in available {
                let tag = mig.tag();
                if !applied.contains(&tag) {
                    return Ok(Some(mig))
                }
            }
            None
        }
        Direction::Down => {
            match applied.last() {
                Some(tag) => {
                    let mig = available.iter().rev().find(|m| &m.tag() == tag);
                    match mig {
                        None => bail_fmt!(ErrorKind::MigrationNotFound, "Tag not found: {}", tag),
                        Some(mig) => Some(mig),
                    }
                }
                None => None,
            }
        }
    })
}


/// Database type being used
pub enum DbKind {
    Sqlite,
    Postgres,
}
impl DbKind {
    fn from(s: &str) -> Result<Self> {
        Ok(match s {
            "sqlite" => DbKind::Sqlite,
            "postgres" => DbKind::Postgres,
            _ => bail_fmt!(ErrorKind::InvalidDbKind, "Invalid Database Kind: {}", s),
        })
    }
}


/// Apply the migration in the specified direction
fn run_migration(config: &Config, direction: &Direction,
                 migration: &Box<Migratable>) -> std::result::Result<(), Box<std::error::Error>> {
    let db_kind = DbKind::from(config.settings.database_type.as_ref())?;
    Ok(match *direction {
        Direction::Up => {
            migration.apply_up(db_kind, config)?;
        }
        Direction::Down => {
            migration.apply_down(db_kind, config)?;
        }
    })
}


/// Try applying the next available migration in the specified `Direction`
fn apply_migration(config: &Config, direction: Direction,
                       force: bool, fake: bool, all: bool) -> Result<()> {
    let mig_dir = config.migration_dir()?;

    let migrations = match config.migrations {
        Some(ref migrations) => migrations.clone(),
        None => {
            search_for_migrations(&mig_dir)?.into_iter()
                .map(|fm| fm.boxed()).collect()
        }
    };
    match next_available(&direction, migrations.as_slice(), config.applied.as_slice())? {
        None => bail_fmt!(ErrorKind::MigrationComplete, "No un-applied `{}` migrations found", direction),
        Some(next) => {
            print_flush!("Applying: {}", next.description(&direction));

            if fake {
                println!("  ✓ (fake)");
            } else {
                // match runner(config, next.to_str().unwrap()) {
                match run_migration(config, &direction, next) {
                    Ok(_) => println!("  ✓"),
                    Err(ref e) => {
                        println!();
                        if force {
                            println!(" ** Error ** (Continuing because `--force` flag was specified)\n ** {}", e);
                        } else {
                            bail_fmt!(ErrorKind::Migration, "Migration was unsucessful...\n{}", e);
                        }
                    }
                };
            }

            let mig_tag = next.tag();
            match direction {
                Direction::Up => {
                    config.insert_migration_tag(&mig_tag)?;
                }
                Direction::Down => {
                    config.delete_migration_tag(&mig_tag)?;
                }
            }
        }
    };

    let config = config.reload()?;

    if all {
        let res = apply_migration(&config, direction, force, fake, all);
        match res {
            Ok(_) => (),
            Err(error) => {
                if !error.is_migration_complete() { return Err(error) }
            }
        }
    }
    Ok(())
}


/// List the currently applied and available migrations under `migration_location`
pub fn list(config: &Config) -> Result<()> {
    let available = match config.migrations {
        None => {
            let mig_dir = config.migration_dir()?;
            let migs = search_for_migrations(&mig_dir)?
                .into_iter()
                .map(|file_mig| file_mig.boxed())
                .collect::<Vec<_>>();
            if migs.is_empty() {
                println!("No migrations found under {:?}", &mig_dir);
            }
            migs
        }
        Some(ref migs) => {
            if migs.is_empty() {
                println!("No migrations specified");
            }
            migs.clone()
        }
    };

    if available.is_empty() {
        return Ok(())
    }
    println!("Current Migration Status:");
    for mig in &available {
        let tagname = mig.tag();
        let x = config.applied.contains(&tagname);
        println!(" -> [{x}] {name}", x=if x { '✓' } else { ' ' }, name=tagname);
    }
    Ok(())
}


fn invalid_tag(tag: &str) -> bool {
    BAD_TAG_RE.is_match(tag)
}


/// Create a new migration with the given tag
///
/// Intended only for use with `FileMigration`s not managed directly in source
/// with `Config::use_migrations`.
pub fn new(config: &Config, tag: &str) -> Result<()> {
    if invalid_tag(tag) {
        bail_fmt!(ErrorKind::Migration, "Invalid tag `{}`. Tags can contain [a-z0-9-]", tag);
    }
    let now = chrono::Utc::now();
    let dt_string = now.format(DT_FORMAT).to_string();
    let folder = format!("{stamp}_{tag}", stamp=dt_string, tag=tag);

    let mig_dir = config.migration_dir()?.join(folder);

    fs::create_dir_all(&mig_dir)?;

    let up = "up.sql";
    let down = "down.sql";
    for mig in &[up, down] {
        let mut p = mig_dir.clone();
        p.push(mig);
        let _ = fs::File::create(&p)?;
    }
    Ok(())
}


/// Open a repl connection to the given `Config` settings
pub fn shell(config: &Config) -> Result<()> {
    Ok(match config.settings.database_type.as_ref() {
        "sqlite" => {
            let db_path = config.database_path()?;
            let _ = Command::new("sqlite3")
                    .arg(db_path.to_str().unwrap())
                    .spawn().expect("Failing running sqlite3").wait()?;
        }
        "postgres" => {
            let conn_str = config.connect_string()?;
            Command::new("psql")
                    .arg(&conn_str)
                    .spawn().unwrap().wait()?;
        }
        _ => unreachable!(),
    })
}


/// Get user's selection of a set of migrations
fn select_from_matches<'a>(tag: &str, matches: &'a [FileMigration]) -> Result<&'a FileMigration> {
    let min = 1;
    let max = matches.len();
    loop {
        println!("* Migrations matching `{}`:", tag);
        for (row, mig) in matches.iter().enumerate() {
            let dt_string = mig.stamp.expect("Timestamp missing").format(DT_FORMAT).to_string();
            let info = format!("{stamp}_{tag}", stamp=dt_string, tag=mig.tag);
            println!("    {}) {}", row + 1, info);
        }
        print!("\n Please select a migration [1-{}] >> ", max);
        io::stdout().flush()?;
        let mut s = String::new();
        io::stdin().read_line(&mut s)?;
        let n = match s.trim().parse::<usize>() {
            Err(e) => {
                println!("\nError: {}", e);
                continue;
            }
            Ok(n) => {
                if min <= n && n <= max { n - 1 }
                else {
                    println!("\nPlease select a number between 1-{}", max);
                    continue;
                }
            }
        };
        return Ok(&matches[n]);
    }
}


/// Open a migration file containing `tag` in its name
///
/// Intended only for use with `FileMigration`s not managed directly in source
/// with `Config::use_migrations`.
pub fn edit(config: &Config, tag: &str, up_down: &Direction) -> Result<()> {
    let mig_dir = config.migration_dir()?;

    let available = search_for_migrations(&mig_dir)?;
    if available.is_empty() {
        println!("No migrations found under {:?}", &mig_dir);
        return Ok(())
    }

    let matches = available.into_iter().filter(|m| m.tag.contains(tag)).collect::<Vec<_>>();
    let n = matches.len();
    let editor = env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());
    let mig = match n {
        0 => bail_fmt!(ErrorKind::Config, "No migrations found with tag: {}", tag),
        1 => &matches[0],
        _ => {
            println!("* Multiple tags found!");
            select_from_matches(tag, matches.as_slice())?
        }
    };
    let file = match *up_down {
        Direction::Up   => mig.up.as_ref().expect("UP migration missing").to_owned(),
        Direction::Down => mig.down.as_ref().expect("DOWN migration missing").to_owned(),
    };
    let file_path = file.to_str().unwrap();
    let command = format!("{} {}", editor, file_path);
    println!("* Running: `{}`", command);
    let _ = prompt(&format!(" -- Press [ENTER] to open now or [CTRL+C] to exit and edit manually"))?;
    open_file_in_fg(&editor, file_path)
        .map_err(|e| format_err!(ErrorKind::Migration, "Error editing migrant file: {}", e))?;
    Ok(())
}

