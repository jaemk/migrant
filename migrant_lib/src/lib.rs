#[macro_use] extern crate lazy_static;
#[macro_use] extern crate serde_derive;
extern crate serde;
extern crate toml;
extern crate rpassword;
extern crate chrono;
extern crate walkdir;
extern crate regex;
extern crate percent_encoding;
extern crate hyper;
extern crate libc;

#[cfg(feature="postgresql")]
extern crate postgres;

#[cfg(feature="sqlite")]
extern crate rusqlite;

use std::collections::HashMap;
use std::process::Command;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::ffi::{OsStr, CString};
use std::fs;
use std::fmt;
use std::env;

use percent_encoding::{percent_encode, DEFAULT_ENCODE_SET};
use rpassword::read_password;
use walkdir::WalkDir;
use chrono::TimeZone;
use regex::Regex;

#[macro_use]
mod errors;
pub use errors::*;

mod drivers;


static CONFIG_FILE: &'static str = ".migrant.toml";
static DT_FORMAT: &'static str = "%Y%m%d%H%M%S";


static SQLITE_CONFIG_TEMPLATE: &'static str = r#"
# required, do not edit
database_type = "sqlite"

# required: relative path to your database file from this config file dir: `__CONFIG_DIR__/`
# ex.) database_name = "db/db.db"
database_name = ""

migration_location = "migrations"  # defeault "migrations"
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


#[derive(Serialize, Deserialize, Debug, Clone)]
/// Settings that are serialized and saved in a project `.migrant.toml` file
struct Settings {
    database_type: String,
    database_name: String,
    database_host: Option<String>,
    database_port: Option<String>,
    database_user: Option<String>,
    database_password: Option<String>,
    migration_location: String,
    database_params: Option<toml::value::Table>,
}


/// Write the provided bytes to the specified path
fn write_to_path(path: &Path, content: &[u8]) -> Result<()> {
    let mut file = fs::File::create(path)
                    .map_err(Error::IoCreate)?;
    file.write_all(content)
        .map_err(Error::IoWrite)?;
    Ok(())
}


/// Run the given command in the foreground
/// Note: `std::process::Command` runs everything in a background
///       child process. In order to run things like opening an
///       editor, the command must be run in the current process.
fn run_command_in_fg(command: &str) -> Result<()> {
    let c_comm = CString::new(command).unwrap();
    let ret = unsafe { libc::system(c_comm.as_ptr()) };
    if ret != 0 { bail!(ShellCommand <- "Command `{}` exited with status `{}`", command, ret) }
    Ok(())
}


#[derive(Debug, Clone)]
/// Project configuration/settings builder to initialize a new config file
pub struct ConfigInitializer {
    dir: PathBuf,
    database_type: Option<String>,
    interactive: bool,

}
impl ConfigInitializer {
    /// Start a new `ConfigInitializer`
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_owned(),
            database_type: None,
            interactive: true,
        }
    }

    /// Specify the database_type, checks whether the type is supported
    pub fn for_database(mut self, db_type: Option<&str>) -> Result<Self> {
        match db_type {
            None => self.database_type = None,
            Some(db_type) => {
                match db_type {
                    "postgres" | "sqlite" => (),
                    e => bail!(Config <- "unsupported database type: {}", e),
                };
                self.database_type = Some(db_type.to_owned());
            }
        }
        Ok(self)
    }

    /// Toggle interactive prompts
    pub fn interactive(mut self, b: bool) -> Self {
        self.interactive = b;
        self
    }

    /// Determines whether new .migrant file location should be in
    /// the given directory or a user specified path
    fn confirm_new_config_location(dir: &Path) -> Result<PathBuf> {
        println!(" $ A new `{}` config file will be created at the following location: ", CONFIG_FILE);
        println!(" $  {:?}", dir.display());
        let ans = Prompt::with_msg(" $ Is this ok? (y/n) >> ").ask()?;
        if ans.trim().to_lowercase() == "y" {
            return Ok(dir.to_owned());
        }

        println!(" $ You can specify the absolute location now, or nothing to exit");
        let ans = Prompt::with_msg(" $ >> ").ask()?;
        if ans.trim().is_empty() {
            bail!(Config <- "No `{}` path provided", CONFIG_FILE)
        }

        let path = PathBuf::from(ans);
        if !path.is_absolute() || path.file_name().unwrap() != CONFIG_FILE {
            bail!(Config <- "Invalid absolute path: {}, must end in `{}`", path.display(), CONFIG_FILE);
        }
        Ok(path)
    }

    /// Generate a template config file using provided parameters or prompting the user
    /// If running interactively, the file will be opened for editing and `Config::setup`
    /// will be run.
    pub fn initialize(self) -> Result<()> {
        let config_path = self.dir.join(CONFIG_FILE);
        let config_path = if !self.interactive {
            config_path
        } else {
            ConfigInitializer::confirm_new_config_location(&config_path)
                .map_err(|e| format_err!(Error::Config, "unable to create a `{}` config -> {}", CONFIG_FILE, e))?
        };

        let db_type = if let Some(db_type) = self.database_type.as_ref() {
            db_type.to_owned()
        } else {
            if !self.interactive {
                bail!(Config <- "database type must be specified if running non-interactively")
            }
            println!("\n ** Gathering database information...");
            let db_type = Prompt::with_msg(" database type (sqlite|postgres) >> ").ask()?;
            match db_type.as_ref() {
                "postgres" | "sqlite" => (),
                e => bail!(Config <- "unsupported database type: {}", e),
            };
            db_type
        };

        println!("\n ** Writing {} config template to {:?}", db_type, config_path);
        match db_type.as_ref() {
            "postgres" => {
                write_to_path(&config_path, PG_CONFIG_TEMPLATE.as_bytes())?;
            }
            "sqlite" => {
                let content = SQLITE_CONFIG_TEMPLATE.replace("__CONFIG_DIR__", config_path.parent().unwrap().to_str().unwrap());
                write_to_path(&config_path, content.as_bytes())?;
            }
            _ => unreachable!(),
        };

        println!("\n ** Please update `{}` with your database credentials and run `setup`", CONFIG_FILE);

        if self.interactive {
            let editor = env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());
            let command = format!("{} {}", editor, config_path.to_str().unwrap());
            println!(" -- Your config file will be opened with the following command: `{}`", &command);
            println!(" -- After editing, the `setup` command will be run for you");
            let _ = Prompt::with_msg(&format!(" -- Press [ENTER] to open now or [CTRL+C] to exit and open manually")).ask()?;
            run_command_in_fg(&command)
                .map_err(|e| format_err!(Error::Config, "Error editing config file: {}", e))?;

            let config = Config::load(&config_path)?;
            let _setup = config.setup()?;
        }
        Ok(())
    }
}


#[derive(Debug, Clone)]
/// Project configuration/settings
pub struct Config {
    pub path: PathBuf,
    settings: Settings,
    applied: Vec<String>,
}
impl Config {
    /// Do a full reload of the configuration file
    pub fn reload(&self) -> Result<Config> {
        Self::load(&self.path)
    }

    /// Load config file from the given path without querying the database
    /// to check for applied migrations
    pub fn load_file_only(path: &Path) -> Result<Config> {
        let mut file = fs::File::open(path).map_err(Error::IoOpen)?;
        let mut content = String::new();
        file.read_to_string(&mut content).map_err(Error::IoRead)?;
        let settings = toml::from_str::<Settings>(&content).map_err(Error::TomlDe)?;
        Ok(Config {
            path: path.to_owned(),
            settings: settings,
            applied: vec![],
        })
    }

    /// Load config file from the given path and query the database to load up applied migrations
    pub fn load(path: &Path) -> Result<Config> {
        let mut config = Config::load_file_only(path)?;
        let applied = config.load_applied()?;
        config.applied = applied;
        Ok(config)
    }

    /// Load the applied migrations from the database migration table
    fn load_applied(&self) -> Result<Vec<String>> {
        if !self.migration_table_exists()? {
            bail!(Migration <- "`__migrant_migrations` table is missing, maybe try re-setting-up? -> `setup`")
        }

        let applied = match self.settings.database_type.as_ref() {
            "sqlite"    => drivers::sqlite::select_migrations(&self.settings.database_name)?,
            "postgres"  => drivers::pg::select_migrations(&self.connect_string()?)?,
            _ => unreachable!(),
        };
        let mut stamped = vec![];
        for tag in applied.into_iter() {
            if !FULL_TAG_RE.is_match(&tag) {
                bail!(Migration <- "Found a non-conforming tag in the database: `{}`", tag)
            }
            let stamp = chrono::Utc.datetime_from_str(
                tag.split('_').next().unwrap(),
                DT_FORMAT
            ).unwrap();
            stamped.push((stamp, tag));
        }
        stamped.sort_by(|a, b| a.0.cmp(&b.0));
        let applied = stamped.into_iter().map(|tup| tup.1).collect::<Vec<_>>();
        Ok(applied)
    }


    /// Check if a __migrant_migrations table exists
    fn migration_table_exists(&self) -> Result<bool> {
        match self.settings.database_type.as_ref() {
            "sqlite"    => drivers::sqlite::migration_table_exists(self.settings.database_name.as_str()),
            "postgres"  => drivers::pg::migration_table_exists(&self.connect_string()?),
            _ => unreachable!()
        }
    }

    /// Insert given tag into database migration table
    fn insert_migration_tag(&self, tag: &str) -> Result<()> {
        match self.settings.database_type.as_ref() {
            "sqlite"    => drivers::sqlite::insert_migration_tag(&self.settings.database_name, tag)?,
            "postgres"  => drivers::pg::insert_migration_tag(&self.connect_string()?, tag)?,
            _ => unreachable!(),
        };
        Ok(())
    }

    /// Remove a given tag from the database migration table
    fn delete_migration_tag(&self, tag: &str) -> Result<()> {
        match self.settings.database_type.as_ref() {
            "sqlite"    => drivers::sqlite::remove_migration_tag(&self.settings.database_name, tag)?,
            "postgres"  => drivers::pg::remove_migration_tag(&self.connect_string()?, tag)?,
            _ => unreachable!(),
        };
        Ok(())
    }

    /// Start a config initializer in the given directory
    pub fn init_in(dir: &Path) -> ConfigInitializer {
        ConfigInitializer::new(dir)
    }

    /// - Confirm the database can be accessed
    /// - Setup the database migrations table if it doesn't exist yet
    pub fn setup(&self) -> Result<bool> {
        println!(" ** Confirming database credentials...");
        match self.settings.database_type.as_ref() {
            "sqlite" => {
                let db_path = self.path.parent().unwrap().join(&self.settings.database_name);
                let created = drivers::sqlite::create_file_if_missing(&db_path)?;
                println!("    - checking if db file already exists...");
                if created {
                    println!("    - db not found... creating now... ✓")
                } else {
                    println!("    - db already exists ✓");
                }
            }
            "postgres" => {
                let conn_str = self.connect_string()?;
                let can_connect = drivers::pg::can_connect(&conn_str)?;
                if !can_connect {
                    println!(" ERROR: Unable to connect to {}", conn_str);
                    println!("        Please initialize your database and user");
                    println!("\n  ex) sudo -u postgres createdb {}", &self.settings.database_name);
                    println!("      sudo -u postgres createuser {}", self.settings.database_user.as_ref().unwrap());
                    println!("      sudo -u postgres psql -c \"alter user {} with password '****'\"", self.settings.database_user.as_ref().unwrap());
                    println!();
                    bail!(Config <- "Cannot connect to postgres database");
                } else {
                    println!("    - Connection confirmed ✓");
                }
            }
            _ => unreachable!(),
        }

        println!("\n ** Setting up migrations table");
        let table_created = match self.settings.database_type.as_ref() {
            "sqlite" => {
                let db_path = self.path.parent().unwrap().join(&self.settings.database_name);
                drivers::sqlite::migration_setup(&db_path)?
            }
            "postgres" => {
                let conn_str = self.connect_string()?;
                drivers::pg::migration_setup(&conn_str)?
            }
            _ => unreachable!(),
        };

        if table_created {
            println!("    - migrations table missing");
            println!("    - `__migrant_migrations` table created ✓");
            Ok(true)
        } else {
            println!("    - `__migrant_migrations` table already exists ✓");
            Ok(false)
        }
    }

    /// Return the absolute path to the directory containing migration folders
    pub fn migration_dir(&self) -> Result<PathBuf> {
        self.path.parent()
            .map(|p| p.join(&self.settings.migration_location))
            .ok_or_else(|| format_err!(Error::PathError, "Error generating PathBuf to migration_location"))
    }

    /// Return the database type
    pub fn database_type(&self) -> Result<String> {
        Ok(self.settings.database_type.to_owned())
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
        let user = match self.settings.database_user.as_ref().and_then(|s| if s.is_empty() { None } else { Some(s) }) {
            Some(ref user) => encode(user),
            None => bail!(Config <- "'database_user' not specified"),
        };

        let db_name = if self.settings.database_name.is_empty() {
            bail!(Config <- "`database_name` not specified");
        } else {
            encode(&self.settings.database_name)
        };

        let host = self.settings.database_host.clone().unwrap_or_else(|| "localhost".to_string());
        let host = if host.is_empty() { "localhost".to_string() } else { host };
        let host = encode(&host);

        let port = self.settings.database_port.clone().unwrap_or_else(|| "5432".to_string());
        let port = if host.is_empty() { "5432".to_string() } else { port };
        let port = encode(&port);

        let s = format!("{db_type}://{user}{pass}@{host}:{port}/{db_name}",
                db_type=self.settings.database_type,
                user=user,
                pass=pass,
                host=host,
                port=port,
                db_name=db_name);
        let mut url = hyper::Url::parse(&s)
            .map_err(|e| format_err!(Error::Config, "{}", e))?;

        if let Some(ref params) = self.settings.database_params {
            let mut pairs: Vec<(String, String)> = vec![];
            for (k, v) in params.iter() {
                let k = k.to_string();
                let v = match *v {
                    toml::value::Value::String(ref s) => s.to_owned(),
                    ref v => bail!(Config <- "Database params can only be strings, found `{}={}`", k, v),
                };
                pairs.push((k, v));
            }
            url.set_query_from_pairs(pairs);
        }

        Ok(url.serialize())
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

    match next_available(&direction, &mig_dir, config.applied.as_slice()) {
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

            //let mut config = config.clone();
            let mig_tag = next.parent()
                .and_then(Path::file_name)
                .and_then(OsStr::to_str)
                .map(str::to_string)
                .expect(&format!("Error extracting parent dir-name from: {:?}", next));
            match direction {
                Direction::Up => {
                    config.insert_migration_tag(&mig_tag)?;
                    //config.settings.applied.push(mig_tag);
                    //config.save()?;
                }
                Direction::Down => {
                    config.delete_migration_tag(&mig_tag)?;
                    //config.settings.applied.pop();
                    //config.save()?;
                }
            }
        }
    };

    let config = config.reload()?;

    if all {
        let res = apply_migration(&config, direction, force, fake, all);
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
        let x = config.applied.contains(&tagname);
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
