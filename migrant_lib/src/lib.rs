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
use chrono::TimeZone;
use walkdir::WalkDir;
use regex::Regex;

#[macro_use] mod macros;
mod errors;
mod drivers;
mod migratable;
mod config;
mod migration;

pub use errors::*;
pub use migratable::Migratable;
pub use config::{ConfigInitializer, Config};
pub use migration::{FileMigration};



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
    static ref TAG_RE: Regex = Regex::new(r"[^a-z0-9-]+").expect("failed to compile regex");

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


#[derive(Debug)]
/// Migration meta data
struct Migration {
    stamp: chrono::DateTime<chrono::Utc>,
    tag: String,
    dir: PathBuf,
    up: PathBuf,
    down: PathBuf,
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
fn search_for_migrations(mig_root: &PathBuf) -> Vec<Migration> {
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
            .expect(&format!("Error extracting file-name from: {:?}", full_name))
            .split('_');
        let stamp = full_name.next().unwrap();
        let tag = full_name.next().unwrap();

        let mut up = PathBuf::from(path);
        let mut down = PathBuf::from(path);

        for mig in migs.iter() {
            let up_down = mig.file_stem()
                .and_then(OsStr::to_str)
                .expect(&format!("Error extracting file-stem from {:?}", mig));
            match up_down {
                "up" => up = mig.clone(),
                "down" => down = mig.clone(),
                _ => unreachable!(),
            };
        }
        let migration = Migration {
            dir: up.parent().map(PathBuf::from).unwrap(),
            up: up,
            down: down,
            stamp: chrono::Utc.datetime_from_str(stamp, DT_FORMAT).unwrap(),
            tag: tag.to_owned(),
        };
        migrations.push(migration);
    }

    // sort by timestamps chronologically
    migrations.sort_by(|a, b| a.stamp.cmp(&b.stamp));
    migrations
}


/// Return the next available up or down migration
fn next_available(direction: &Direction, mig_dir: &PathBuf, applied: &[String]) -> Option<PathBuf> {
    match *direction {
        Direction::Up => {
            let available = search_for_migrations(mig_dir);
            for mig in &available {
                let tag = mig.dir.file_name()
                    .and_then(OsStr::to_str)
                    .map(str::to_string)
                    .expect(&format!("Error extracting dir-name from: {:?}", mig.dir));
                if !applied.contains(&tag) {
                    return Some(mig.up.clone())
                }
            }
            None
        }
        Direction::Down => {
            applied.last()
                .map(PathBuf::from)
                .map(|mut tag| {
                    tag.push("down.sql");
                    let mut pb = mig_dir.clone();
                    pb.push(tag);
                    pb
                })
        }
    }
}


/// Run a given migration file through either sqlite or postgres, returning the output
fn runner(config: &Config, filename: &str) -> Result<()> {
    let settings = &config.settings;
    Ok(match settings.database_type.as_ref() {
        "sqlite" => {
            let db_path = config.database_path()?;
            drivers::sqlite::run_migration(&db_path, filename)?;
        }
        "postgres" => {
            let conn_str = config.connect_string()?;
            drivers::pg::run_migration(&conn_str, filename)?;
        }
        _ => unreachable!(),
    })
}


/// Try applying the next available migration in the specified `Direction`
fn apply_migration(config: &Config, direction: Direction,
                       force: bool, fake: bool, all: bool) -> Result<()> {
    let mig_dir = config.migration_dir()?;

    match next_available(&direction, &mig_dir, config.applied.as_slice()) {
        None => bail_fmt!(ErrorKind::MigrationComplete, "No un-applied `{}` migration found in `{}/`",
                      direction, config.settings.migration_location),
        Some(next) => {
            print!("Applying: {:?}", next);

            if fake {
                println!("  ✓ (fake)");
            } else {
                match runner(config, next.to_str().unwrap()) {
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

            let mig_tag = next.parent()
                .and_then(Path::file_name)
                .and_then(OsStr::to_str)
                .map(str::to_string)
                .expect(&format!("Error extracting parent dir-name from: {:?}", next));
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
    let mig_dir = config.migration_dir()?;

    let available = search_for_migrations(&mig_dir);
    if available.is_empty() {
        println!("No migrations found under {:?}", &mig_dir);
        return Ok(())
    }
    println!("Current Migration Status:");
    for mig in &available {
        let tagname = mig.up.parent()
            .and_then(Path::file_name)
            .and_then(OsStr::to_str)
            .map(str::to_string)
            .expect(&format!("Error extracting parent dir-name from: {:?}", mig.up));
        let x = config.applied.contains(&tagname);
        println!(" -> [{x}] {name}", x=if x { '✓' } else { ' ' }, name=tagname);
    }
    Ok(())
}


fn valid_tag(tag: &str) -> bool {
    TAG_RE.is_match(tag)
}


/// Create a new migration with the given tag
pub fn new(config: &Config, tag: &str) -> Result<()> {
    if valid_tag(tag) {
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
fn select_from_matches<'a>(tag: &str, matches: &'a [Migration]) -> Result<&'a Migration> {
    let min = 1;
    let max = matches.len();
    loop {
        println!("* Migrations matching `{}`:", tag);
        for (row, mig) in matches.iter().enumerate() {
            let dt_string = mig.stamp.format(DT_FORMAT).to_string();
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
pub fn edit(config: &Config, tag: &str, up_down: &Direction) -> Result<()> {
    let mig_dir = config.migration_dir()?;

    let available = search_for_migrations(&mig_dir);
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
            select_from_matches(tag, &matches)?
        }
    };
    let file = match *up_down {
        Direction::Up   => mig.up.to_owned(),
        Direction::Down => mig.down.to_owned(),
    };
    let file_path = file.to_str().unwrap();
    let command = format!("{} {}", editor, file_path);
    println!("* Running: `{}`", command);
    let _ = prompt(&format!(" -- Press [ENTER] to open now or [CTRL+C] to exit and edit manually"))?;
    open_file_in_fg(&editor, file_path)
        .map_err(|e| format_err!(ErrorKind::Migration, "Error editing migrant file: {}", e))?;
    Ok(())
}
