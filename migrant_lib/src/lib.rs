#[macro_use] extern crate lazy_static;
#[macro_use] extern crate serde_derive;
extern crate serde;
extern crate toml;
extern crate rpassword;
extern crate chrono;
extern crate walkdir;
extern crate regex;
extern crate percent_encoding;

#[cfg(feature="postgresql")]
extern crate postgres;

#[cfg(feature="sqlite")]
extern crate rusqlite;

use std::collections::HashMap;
use std::process::Command;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::ffi::OsStr;
use std::fs;
use std::fmt;

use percent_encoding::{percent_encode, DEFAULT_ENCODE_SET};
use rpassword::read_password;
use walkdir::WalkDir;
use chrono::TimeZone;
use regex::Regex;

#[macro_use]
mod errors;
pub use errors::*;


static CONFIG_FILE: &'static str = ".migrant.toml";
static DT_FORMAT: &'static str = "%Y%m%d%H%M%S";


lazy_static! {
    // For verifying new `tag` names
    static ref TAG_RE: Regex = Regex::new(r"[^a-z0-9-]+").unwrap();

    // For pulling out `tag` names from `toml::to_string`
    static ref MIG_RE: Regex = Regex::new(r##"(?P<mig>"[\d]+_[a-z0-9-]+")"##).unwrap();
}


#[derive(Serialize, Deserialize, Debug, Clone)]
/// Settings that are serialized and saved in a project `.migrant.toml` file
struct Settings {
    database_type: String,
    database_name: String,
    database_host: Option<String>,
    database_user: Option<String>,
    database_password: Option<String>,
    migration_location: String,
    applied: Vec<String>,
}
impl Settings {
    fn new(db_type: String, db_name: String, db_host: Option<String>, db_user: Option<String>, password: Option<String>) -> Settings {
        Settings {
            database_type: db_type,
            database_name: db_name,
            database_host: db_host.map(|host| if host.is_empty() { "localhost".to_string() } else { host }),
            database_user: db_user,
            database_password: password,
            migration_location: "migrations".to_string(),
            applied: vec![],
        }
    }
}


#[derive(Debug, Clone)]
/// Project configuration/settings
pub struct Config {
    pub path: PathBuf,
    settings: Settings,
}
impl Config {
    /// Create a new config
    fn new(settings: Settings, path: &PathBuf) -> Config {
        Config {
            path: path.clone(),
            settings: settings,
        }
    }

    /// Load `.migrant.toml` config file from the given path
    pub fn load(path: &PathBuf) -> Result<Config> {
        let mut file = fs::File::open(path).map_err(Error::IoOpen)?;
        let mut content = String::new();
        file.read_to_string(&mut content).map_err(Error::IoRead)?;
        let settings = toml::from_str::<Settings>(&content).map_err(Error::TomlDe)?;
        Ok(Config {
            path: path.clone(),
            settings: settings,
        })
    }

    /// Reload configuration file
    pub fn reload(&self) -> Result<Config> {
        Self::load(&self.path)
    }

    /// Determines whether new .migrant file location should be in
    /// the given directory or a user specified path
    fn confirm_new_config_location(mut dir: PathBuf) -> Result<PathBuf> {
        dir.push(".migrant.toml");
        println!(" $ A new `.migrant.toml` config file will be created at the following location: ");
        println!(" $  {:?}", dir.display());
        let ans = Prompt::with_msg(" $ Is this ok? (y/n) >> ").ask()?;
        if ans.trim().to_lowercase() == "y" {
            return Ok(dir);
        }

        println!(" $ You can specify the absolute location now, or nothing to exit");
        let ans = Prompt::with_msg(" $ >> ").ask()?;
        if ans.trim().is_empty() {
            bail!(Config <- "No `.migrant.toml` path provided")
        }

        let path = PathBuf::from(ans);
        if !path.is_absolute() || path.file_name().unwrap() != ".migrant.toml" {
            bail!(Config <- "Invalid absolute path: {}, must end in '.migrant.toml'", path.display());
        }
        Ok(path)
    }

    /// Initialize project in the given directory
    pub fn init(dir: &PathBuf) -> Result<Config> {
        let config_path = Config::confirm_new_config_location(dir.clone())
            .map_err(|e| format_err!(Error::Config, "unable to create a .migrant.toml config -> {}", e))?;

        let db_type = Prompt::with_msg(" db-type (sqlite|postgres) >> ").ask()?;
        match db_type.as_ref() {
            "postgres" | "sqlite" => (),
            e => bail!(Config <- "unsupported database type: {}", e),
        }

        let (db_name, db_host, db_user, db_pass) = match db_type.as_ref() {
            "postgres" => {
                (
                    Prompt::with_msg(" $ database name >> ").ask()?,
                    Some(Prompt::with_msg(&format!(" $ {} database host (leave blank to default to `localhost`) >> ", &db_type)).ask()?),
                    Some(Prompt::with_msg(&format!(" $ {} database user >> ", &db_type)).ask()?),
                    Some(Prompt::with_msg(&format!(" $ {} user password >> ", &db_type)).secure().ask()?),
                )
            }
            "sqlite" => {
                (
                    Prompt::with_msg(" $ relative path to database (from .migrant.toml config file) >> ").ask()?,
                    None,
                    None,
                    None,
                )
            }
            _ => unreachable!(),
        };

        let settings = Settings::new(db_type, db_name, db_host, db_user, db_pass);
        let config = Config::new(settings.clone(), &config_path);

        println!("\n ** Confirming database credentials");
        match settings.database_type.as_ref() {
            "sqlite" => {
                let db_path = config_path.parent().unwrap().join(&settings.database_name);
                let created = create_file_if_missing(&db_path)?;
                println!("created: {}", created);
                println!("    - checking if db file already exists...");
                if created {
                    println!("    - db not found... creating now... ✓")
                } else {
                    println!("    - db already exists ✓");
                }
            }
            "postgres" => {
                let conn_str = config.connect_string()?;
                let can_connect = pg_can_connect(&conn_str)?;
                if !can_connect {
                    println!(" ERROR: Unable to connect to {}", conn_str);
                    println!("        Please initialize your database and user");
                    println!("\n  ex) sudo -u postgres createdb {}", &settings.database_name);
                    println!("      sudo -u postgres createuser {}", settings.database_user.as_ref().unwrap());
                    println!("      sudo -u postgres psql -c \"alter user {} with password '****'\"", settings.database_user.as_ref().unwrap());
                    println!();
                    bail!(Config <- "Cannot connect to postgres database");
                } else {
                    println!("    - Connection confirmed ✓");
                }
            }
            _ => unreachable!(),
        }

        println!("\n ** Setting up migrations table");
        let table_created = match settings.database_type.as_ref() {
            "sqlite" => {
                let db_path = config_path.parent().unwrap().join(&settings.database_name);
                sqlite_migration_setup(&db_path)?
            }
            "postgres" => {
                let conn_str = config.connect_string()?;
                pg_migration_setup(&conn_str)?
            }
            _ => unreachable!(),
        };

        if table_created {
            println!("    - migrations table missing");
            println!("    - `__migrant_migrations` table created ✓");
        } else {
            println!("    - `__migrant_migrations` table already exists ✓");
        }

        config.save()?;
        Ok(config)
    }

    /// Write the current config to its file path
    fn save(&self) -> Result<()> {
        let mut file = fs::File::create(self.path.clone())
                        .map_err(Error::IoCreate)?;
        let content = toml::to_string(&self.settings).map_err(Error::TomlSe)?;
        let content = content.lines().map(|line| {
            if !line.starts_with("applied") { line.to_string() }
            else {
                // format the list of applied migrations nicely
                let migs = MIG_RE.captures_iter(line)
                    .map(|cap| format!("    {}", &cap["mig"]))
                    .collect::<Vec<_>>()
                    .join(",\n");
                format!("applied = [\n{}\n]", migs)
            }
        }).collect::<Vec<_>>().join("\n");
        file.write_all(content.as_bytes())
            .map_err(Error::IoWrite)?;
        Ok(())
    }

    /// Return the absolute path to the directory containing migration folders
    pub fn migration_dir(&self) -> Result<PathBuf> {
        self.path.parent()
            .map(|p| p.join(&self.settings.migration_location))
            .ok_or_else(|| format_err!(Error::PathError, "Error generating PathBuf to migration_location"))
    }

    /// Return the absolute path to the database file. This is intended for
    /// sqlite3 databases only
    pub fn database_path(&self) -> Result<PathBuf> {
        match self.settings.database_type.as_ref() {
            "sqlite" => (),
            db_t => bail!(Config <- "Cannot retrieve database-path for database-type: {}", db_t),
        };

        self.path.parent()
            .map(|p| p.join(&self.settings.database_name))
            .ok_or_else(|| format_err!(Error::PathError, "Error generating PathBuf to database-path"))
    }

    /// Generate a database connection string.
    /// Not intended for file-based databases (sqlite)
    pub fn connect_string(&self) -> Result<String> {
        match self.settings.database_type.as_ref() {
            "postgres" => (),
            db_t => bail!(Config <- "Cannot generate connect-string for database-type: {}", db_t),
        };

        let pass = match self.settings.database_password {
            Some(ref pass) => {
                let p = encode(pass);
                format!(":{}", p)
            },
            None => "".into(),
        };
        let user = match self.settings.database_user {
            Some(ref user) => encode(user),
            None => bail!(Config <- "config-err: 'database_user' not specified"),
        };
        let host = self.settings.database_host.as_ref()
            .map(|s| s.to_string())
            .unwrap_or("localhost".to_string());
        let host = if host.is_empty() { "localhost".to_string() } else { encode(&host) };
        let db_name = encode(&self.settings.database_name);
        Ok(format!("{db_type}://{user}{pass}@{host}/{db_name}",
                db_type=self.settings.database_type,
                user=user,
                pass=pass,
                host=host,
                db_name=db_name))
    }
}


/// Percent encode a string
fn encode(s: &str) -> String {
    percent_encode(s.as_bytes(), DEFAULT_ENCODE_SET).to_string()
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


#[cfg(not(feature="postgresql"))]
fn pg_can_connect(connect_string: &str) -> Result<bool> {
    let out = Command::new("psql")
                    .arg(connect_string)
                    .arg("-c")
                    .arg("")
                    .output()
                    .map_err(Error::IoProc)?;
    Ok(out.status.success())
}

#[cfg(feature="postgresql")]
fn pg_can_connect(conn_str: &str) -> Result<bool> {
    use postgres::{Connection, TlsMode};

    match Connection::connect(conn_str, TlsMode::None) {
        Ok(_)   => Ok(true),
        Err(_)  => Ok(false)
    }
}


/// Create a file if it doesn't exist, returning true if the file was created
fn create_file_if_missing(path: &PathBuf) -> Result<bool> {
    if path.exists() {
        Ok(false)
    } else {
        let db_dir = path.parent().unwrap();
        fs::create_dir(db_dir).map_err(Error::IoCreate)?;
        fs::File::create(path).map_err(Error::IoCreate)?;
        Ok(true)
    }
}


mod sql {
    pub static CREATE_TABLE: &'static str = "create table __migrant_migrations(tag text unique);";
    //pub static GET_MIGRATIONS: &'static str = "select tag from __migrant_migrations;";

    pub static SQLITE_MIGRATION_TABLE_EXISTS: &'static str = "select exists(select 1 from sqlite_master where type = 'table' and name = '__migrant_migrations');";
    //pub static SQLITE_ADD_MIGRATION: &'static str = "into __migrant_migrations (tag) values ('{}');";
    //pub static SQLITE_DELETE_MIGRATION: &'static str = "delete from __migrant_migrations where tag = '{}';";

    pub static PG_MIGRATION_TABLE_EXISTS: &'static str = "select exists(select 1 from pg_tables where tablename = '__migrant_migrations');";
    //pub static PG_ADD_MIGRATION: &'static str = "prepare stmt as insert into __migrant_migrations (tag) values ($1); execute stmt({}); deallocate stmt;";
    //pub static PG_DELETE_MIGRATION: &'static str = "prepare stmt as delete from __migrant_migrations where tag = $1; execute stmt({}); deallocate stmt;";
}

#[cfg(not(feature="postgres"))]
fn pg_migration_setup(conn_str: &str) -> Result<bool> {
    let exists = Command::new("psql")
                    .arg(conn_str)
                    .arg("-t")      // no headers or footer
                    .arg("-A")      // un-aligned output
                    .arg("-F,")     // comma separator
                    .arg("-c")
                    .arg(sql::PG_MIGRATION_TABLE_EXISTS)
                    .output()
                    .map_err(Error::IoProc)?;
    if !exists.status.success() {
        let stderr = std::str::from_utf8(&exists.stderr).unwrap();
        bail!(Migration <- "Error executing statement: {}", stderr);
    }
    let stdout = std::str::from_utf8(&exists.stdout).unwrap();
    if stdout.trim() == "t" {
        // exists
        Ok(false)
    } else {
        let out = Command::new("psql")
                        .arg(conn_str)
                        .arg("-t")
                        .arg("-A")
                        .arg("-F,")
                        .arg("-c")
                        .arg(sql::CREATE_TABLE)
                        .output()
                        .map_err(Error::IoProc)?;
        if !out.status.success() {
            let stderr = std::str::from_utf8(&out.stderr).unwrap();
            bail!(Migration <- "Error executing statement: {}", stderr);
        }
        Ok(true)
    }
}

#[cfg(feature="postgres")]
fn pg_migration_setup(conn_str: &str) -> Result<bool> {
    use postgres::{Connection, TlsMode};

    let conn = Connection::connect(conn_str, TlsMode::None)
        .map_err(|e| format_err!(Error::Migration, "{}", e))?;
    let rows = conn.query(sql::PG_MIGRATION_TABLE_EXISTS, &[])
        .map_err(|e| format_err!(Error::Migration, "{}", e))?;
    let exists: bool = rows.iter().next().unwrap().get(0);
    if exists {
        Ok(false)
    } else {
        conn.execute(sql::CREATE_TABLE, &[])
            .map_err(|e| format_err!(Error::Migration, "{}", e))?;
        Ok(true)
    }
}


#[cfg(not(feature="sqlite"))]
fn sqlite_migration_setup(db_path: &PathBuf) -> Result<bool> {
    let db_path = db_path.as_os_str().to_str().unwrap();
    let exists = Command::new("sqlite3")
                    .arg(&db_path)
                    .arg("-csv")
                    .arg(sql::SQLITE_MIGRATION_TABLE_EXISTS)
                    .output()
                    .map_err(Error::IoProc)?;
    if !exists.status.success() {
        let stderr = std::str::from_utf8(&exists.stderr).unwrap();
        bail!(Migration <- "Error executing statement: {}", stderr);
    }
    let stdout = std::str::from_utf8(&exists.stdout).unwrap();
    if stdout.trim() == "1" {
        // exists
        Ok(false)
    } else {
        let out = Command::new("sqlite3")
                        .arg(&db_path)
                        .arg("-csv")
                        .arg(sql::CREATE_TABLE)
                        .output()
                        .map_err(Error::IoProc)?;
        if !out.status.success() {
            let stderr = std::str::from_utf8(&out.stderr).unwrap();
            bail!(Migration <- "Error executing statement: {}", stderr);
        }
        Ok(true)
    }
}

#[cfg(feature="sqlite")]
fn sqlite_migration_setup(db_path: &PathBuf) -> Result<bool> {
    use rusqlite::Connection;

    let conn = Connection::open(db_path)
        .map_err(|e| format_err!(Error::Migration, "{}", e))?;
    let exists: bool = conn.query_row(sql::SQLITE_MIGRATION_TABLE_EXISTS, &[], |row| row.get(0))
        .map_err(|e| format_err!(Error::Migration, "{}", e))?;

    if exists {
        Ok(false)
    } else {
        conn.execute(sql::CREATE_TABLE, &[])
            .map_err(|e| format_err!(Error::Migration, "{}", e))?;
        Ok(true)
    }
}


/// Fall back to running the migration using the sqlite cli
#[cfg(not(feature="sqlite"))]
fn run_sqlite(db_path: &PathBuf, filename: &str) -> Result<()> {
    Command::new("sqlite3")
            .arg(db_path.to_str().unwrap())
            .arg(&format!(".read {}", filename))
            .output()
            .map_err(Error::IoProc)?;
    Ok(())
}

#[cfg(feature="sqlite")]
fn run_sqlite(db_path: &PathBuf, filename: &str) -> Result<()> {
    use rusqlite::Connection;

    let mut file = fs::File::open(filename)
        .map_err(Error::IoOpen)?;
    let mut buf = String::new();
    file.read_to_string(&mut buf)
        .map_err(Error::IoRead)?;
    if buf.is_empty() { return Ok(()); }

    let conn = Connection::open(db_path)
        .map_err(|e| format_err!(Error::Migration, "{}", e))?;
    conn.execute(&buf, &[])
        .map_err(|e| format_err!(Error::Migration, "{}", e))?;
    Ok(())
}


#[cfg(feature="postgresql")]
fn run_postgres(conn_str: &str, filename: &str) -> Result<()> {
    use postgres::{Connection, TlsMode};

    let mut file = fs::File::open(filename)
        .map_err(Error::IoOpen)?;
    let mut buf = String::new();
    file.read_to_string(&mut buf)
        .map_err(Error::IoRead)?;

    let conn = Connection::connect(conn_str, TlsMode::None)
        .map_err(|e| format_err!(Error::Migration, "{}", e))?;
    conn.execute(&buf, &[])
        .map_err(|e| format_err!(Error::Migration, "{}", e))?;
    Ok(())
}

/// Fall back to running the migration using the postgres cli
#[cfg(not(feature="postgresql"))]
fn run_postgres(conn_str: &str, filename: &str) -> Result<()> {
    Command::new("psql")
            .arg(&conn_str)
            .arg("-f").arg(filename)
            .output()
            .map_err(Error::IoProc)?;
    Ok(())
}


/// Run a given migration file through either sqlite or postgres, returning the output
fn runner(config: &Config, filename: &str) -> Result<()> {
    let settings = &config.settings;
    Ok(match settings.database_type.as_ref() {
        "sqlite" => {
            let db_path = config.database_path()?;
            run_sqlite(&db_path, filename)?;
        }
        "postgres" => {
            let conn_str = config.connect_string()?;
            run_postgres(&conn_str, filename)?;
        }
        _ => unreachable!(),
    })
}


/// CLI Prompter
pub struct Prompt {
    msg: String,
    secure: bool,
}
impl Prompt {
    /// Construct a new `Prompt` with the given message
    pub fn with_msg(msg: &str) -> Self {
        Self {
            msg: msg.into(),
            secure: false,
        }
    }

    /// Ask securely. Don't show output when typing
    pub fn secure(mut self) -> Self {
        self.secure = true;
        self
    }

    /// Prompt the user and return their input
    pub fn ask(self) -> Result<String> {
        print!("{}", self.msg);
        io::stdout().flush().map_err(Error::IoWrite)?;
        let resp = if self.secure {
            read_password().map_err(Error::IoRead)?
        } else {
            let mut resp = String::new();
            io::stdin().read_line(&mut resp)
                .map_err(Error::IoRead)?;
            resp.trim().to_string()
        };
        Ok(resp)
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
        let stamp = PathBuf::from(path);
        let mut stamp = stamp.file_name()
            .and_then(OsStr::to_str)
            .expect(&format!("Error extracting file-name from: {:?}", stamp))
            .split('_');
        let stamp = stamp.next().unwrap();

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
            stamp: chrono::Utc.datetime_from_str(stamp, DT_FORMAT).unwrap()
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


/// Try applying the next available migration in the specified `Direction`
fn apply_migration(config: &Config, direction: Direction,
                       force: bool, fake: bool, all: bool) -> Result<()> {
    let mig_dir = config.migration_dir()?;

    let new_config = match next_available(&direction, &mig_dir, config.settings.applied.as_slice()) {
        None => bail!(MigrationComplete <- "No un-applied `{}` migration found in `{}/`", direction, config.settings.migration_location),
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
                            bail!(Migration <- "Migration was unsucessful...\n{}", e);
                        }
                    }
                };
            }

            let mut config = config.clone();
            match direction {
                Direction::Up => {
                    let mig_tag = next.parent()
                        .and_then(Path::file_name)
                        .and_then(OsStr::to_str)
                        .map(str::to_string)
                        .expect(&format!("Error extracting parent dir-name from: {:?}", next));
                    config.settings.applied.push(mig_tag);
                    config.save()?;
                }
                Direction::Down => {
                    config.settings.applied.pop();
                    config.save()?;
                }
            }
            config
        }
    };

    if all {
        let res = apply_migration(&new_config, direction, force, fake, all);
        match res {
            Ok(_) => (),
            Err(error) => match error {
                // No more migrations in this direction
                Error::MigrationComplete(_) => (),
                // Some actual error
                e => return Err(e),
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
        let x = config.settings.applied.contains(&tagname);
        println!(" -> [{x}] {name}", x=if x { '✓' } else { ' ' }, name=tagname);
    }
    Ok(())
}


/// Create a new migration with the given tag
pub fn new(config: &Config, tag: &str) -> Result<()> {
    if TAG_RE.is_match(tag) {
        bail!(Migration <- "Invalid tag `{}`. Tags can contain [a-z0-9-]", tag);
    }
    let now = chrono::Utc::now();
    let dt_string = now.format(DT_FORMAT).to_string();
    let folder = format!("{stamp}_{tag}", stamp=dt_string, tag=tag);

    let mig_dir = config.migration_dir()?.join(folder);

    fs::create_dir_all(&mig_dir)
        .map_err(Error::IoCreate)?;

    let up = "up.sql";
    let down = "down.sql";
    for mig in &[up, down] {
        let mut p = mig_dir.clone();
        p.push(mig);
        let _ = fs::File::create(&p).map_err(Error::IoCreate)?;
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
                    .spawn().unwrap().wait()
                    .map_err(Error::IoProc)?;
        }
        "postgres" => {
            let conn_str = config.connect_string()?;
            Command::new("psql")
                    .arg(&conn_str)
                    .spawn().unwrap().wait()
                    .map_err(Error::IoProc)?;
        }
        _ => unreachable!(),
    })
}
