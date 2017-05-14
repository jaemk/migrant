#[macro_use] extern crate lazy_static;
#[macro_use] extern crate serde_derive;
extern crate serde;
extern crate toml;
extern crate rpassword;
extern crate chrono;
extern crate walkdir;
extern crate regex;

use std::collections::HashMap;
use std::process::Command;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::ffi::OsStr;
use std::fs;
use std::fmt;

use rpassword::read_password;
use walkdir::WalkDir;
use chrono::TimeZone;
use regex::Regex;


#[derive(Debug)]
pub enum Error {
    Config(String),
    Migration(String),
    MigrationNotFound(String),
    PathError(String),
    IoOpen(std::io::Error),
    IoCreate(std::io::Error),
    IoRead(std::io::Error),
    IoWrite(std::io::Error),
    IoProc(std::io::Error),
    Utf8Error(std::string::FromUtf8Error),
    TomlDe(toml::de::Error),
    TomlSe(toml::ser::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use Error::*;
        match *self {
            Config(ref s)             => write!(f, "Config Error: {}", s),
            Migration(ref s)          => write!(f, "Migration Error: {}", s),
            MigrationNotFound(ref s)  => write!(f, "MigrationNotFound: {}", s),
            PathError(ref s)          => write!(f, "PathError: {}", s),
            IoOpen(ref e)             => write!(f, "IoOpen Error: {}", e),
            IoCreate(ref e)           => write!(f, "IoCreate Error: {}", e),
            IoRead(ref e)             => write!(f, "IoRead Error: {}", e),
            IoWrite(ref e)            => write!(f, "IoWrite Error: {}", e),
            IoProc(ref e)             => write!(f, "IoProcess Error: {}", e),
            Utf8Error(ref e)          => write!(f, "Utf8 Error: {}", e),
            TomlDe(ref e)             => write!(f, "Toml Deserialization Error: {}", e),
            TomlSe(ref e)             => write!(f, "Toml Serialization Error: {}", e),
        }
    }
}


type Result<T> = std::result::Result<T, Error>;


macro_rules! bail {
    (Config <- $msg:expr) => {
        return Err(Error::Config($msg))
    };
    (Migration <- $msg:expr) => {
        return Err(Error::Migration($msg))
    };
    (MigrationNotFound <- $msg:expr) => {
        return Err(Error::MigrationNotFound($msg))
    }
}


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
    fn new(db_type: String, db_name: String, db_user: Option<String>, password: Option<String>) -> Settings {
        Settings {
            database_type: db_type,
            database_name: db_name,
            database_host: Some("localhost".to_string()),
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
            bail!(Config <- "No `.migrant.toml` path provided".into())
        }

        let path = PathBuf::from(ans);
        if !path.is_absolute() || path.file_name().unwrap() != ".migrant.toml" {
            bail!(Config <- format!("Invalid absolute path: {}, must end in '.migrant.toml'", path.display()));
        }
        Ok(path)
    }

    /// Initialize project in the given directory
    pub fn init(dir: &PathBuf) -> Result<Config> {
        let config_path = Config::confirm_new_config_location(dir.clone())
            .map_err(|e| Error::Config(format!("unable to create a .migrant.toml config -> {}", e)))?;

        let db_type = Prompt::with_msg(" db-type (sqlite|postgres) >> ").ask()?;
        match db_type.as_ref() {
            "postgres" | "sqlite" => (),
            e => bail!(Config <- format!("unsupported database type: {}", e)),
        }

        let (db_name, db_user, db_pass) = match db_type.as_ref() {
            "postgres" => {
                (
                    Prompt::with_msg(" $ database name >> ").ask()?,
                    Some(Prompt::with_msg(&format!(" $ {} database user >> ", &db_type)).ask()?),
                    Some(Prompt::with_msg(&format!(" $ {} user password >> ", &db_type)).secure().ask()?),
                )
            }
            "sqlite" => {
                (
                    Prompt::with_msg(" $ relative path to database (from .migrant.toml config file) >> ").ask()?,
                    None,
                    None,
                )
            }
            _ => unreachable!(),
        };

        let settings = Settings::new(db_type, db_name, db_user, db_pass);
        let config = Config::new(settings, &config_path);
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
    stamp: chrono::DateTime<chrono::UTC>,
    dir: PathBuf,
    up: PathBuf,
    down: PathBuf,
}


#[derive(Debug, Clone)]
/// Migration applicator
pub struct Migrator {
    project_dir: PathBuf,
    config_path: PathBuf,
    config: Config,
    direction: Direction,
    force: bool,
    fake: bool,
    all: bool,
}

impl Migrator {
    /// Initialize a new `Migrator` with a given config
    pub fn with_config(config: &Config) -> Self {
        let project_dir = config.path.parent()
            .map(Path::to_path_buf)
            .expect(&format!("Error extracting parent file-path from: {:?}", config.path));
        Self {
            config: config.clone(),
            config_path: config.path.clone(),
            project_dir: project_dir,
            direction: Direction::Up,
            force: false,
            fake: false,
            all: false,
        }
    }

    /// Set `project_dir`
    pub fn project_dir(mut self, proj_dir: &PathBuf) -> Self {
        self.project_dir = proj_dir.clone();
        self
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
            &self.project_dir, &self.config_path, &self.config,
            self.direction, self.force, self.fake, self.all,
            )
    }
}


/// Generate a database connection string
fn connect_string(settings: &Settings) -> Result<String> {
    let pass = match settings.database_password {
        Some(ref pass) => format!(":{}", pass),
        None => "".into(),
    };
    let user = match settings.database_user {
        Some(ref user) => user.to_string(),
        None => bail!(Config <- "config-err: 'database_user' not specified".into()),
    };
    Ok(format!("{db_type}://{user}{pass}@{host}/{db_name}",
            db_type=settings.database_type,
            user=user,
            pass=pass,
            host=settings.database_host.as_ref().unwrap_or(&"localhost".to_string()),
            db_name=settings.database_name))
}


/// Run a given migration file through either sqlite or postgres, returning the output
fn runner(config: &Config, filename: &str) -> Result<std::process::Output> {
    let settings = &config.settings;
    Ok(match settings.database_type.as_ref() {
        "sqlite" => {
            let mut db_path = config.path.clone();
            db_path.pop();
            db_path.push(&settings.database_name);
            Command::new("sqlite3")
                    .arg(db_path.to_str().unwrap())
                    .arg(&format!(".read {}", filename))
                    .output()
                    .map_err(Error::IoProc)?
        }
        "postgres" => {
            let conn_str = connect_string(settings)?;
            Command::new("psql")
                    .arg(&conn_str)
                    .arg("-f").arg(&filename)
                    .output()
                    .map_err(Error::IoProc)?
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
            stamp: chrono::UTC.datetime_from_str(stamp, DT_FORMAT).unwrap()
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
fn apply_migration(project_dir: &PathBuf, config_path: &PathBuf, config: &Config, direction: Direction,
                       force: bool, fake: bool, all: bool) -> Result<()> {
    let mut mig_dir = project_dir.clone();
    mig_dir.push(PathBuf::from(&config.settings.migration_location));

    let new_config = match next_available(&direction, &mig_dir, config.settings.applied.as_slice()) {
        None => bail!(MigrationNotFound <- format!("No un-applied `{}` migration found in `{}/`", direction, config.settings.migration_location)),
        Some(next) => {
            println!("Applying: {:?}", next);

            let stdout = if fake {
                String::new()
            } else {
                let out = runner(config, next.to_str().unwrap())
                    .map_err(|e| Error::Migration(format!("Error occurred while running migration -> {}", e)))?;

                let success = out.status.success();
                if !success {
                    let info = String::from_utf8(out.stderr).map_err(Error::Utf8Error)?;
                    let info = format!(" ** Error **\n{}", info);
                    if force {
                        println!("{}", info);
                    } else {
                        bail!(Migration <- format!("Migration was unsuccessful...\n{}", info));
                    }
                }
                String::from_utf8(out.stdout).map_err(Error::Utf8Error)?
            };

            if !stdout.is_empty() {
                println!("{}", stdout);
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
        let res = apply_migration(project_dir, config_path, &new_config, direction, force, fake, all);
        match res {
            Ok(_) => (),
            Err(error) => match error {
                Error::MigrationNotFound(_) => (),
                e => return Err(e),
            }
        }
    }
    Ok(())
}


/// Try extracting the parent path
fn parent_path(path: &PathBuf) -> Result<PathBuf> {
    path.parent()
        .map(PathBuf::from)
        .ok_or_else(|| format!("Error extracting parent path from: {:?}", path))
        .map_err(Error::PathError)
}


/// List the currently applied and available migrations under `migration_location`
pub fn list(config: &Config) -> Result<()> {
    let mut mig_dir = parent_path(&config.path)?;
    mig_dir.push(PathBuf::from(&config.settings.migration_location));
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
        println!(" -> [{x}] {name}", x=if x { 'x' } else { ' ' }, name=tagname);
    }
    Ok(())
}


/// Create a new migration with the given tag
pub fn new(config: &Config, tag: &str) -> Result<()> {
    if TAG_RE.is_match(tag) {
        bail!(Migration <- format!("Invalid tag `{}`. Tags can contain [a-z0-9-]", tag));
    }
    let now = chrono::UTC::now();
    let dt_string = now.format(DT_FORMAT).to_string();
    let folder = format!("{stamp}_{tag}", stamp=dt_string, tag=tag);
    let mut mig_dir = parent_path(&config.path)?;
    mig_dir.push(&config.settings.migration_location);
    mig_dir.push(folder);
    let _ = fs::create_dir_all(&mig_dir);

    let up = "up.sql";
    let down = "down.sql";
    for mig in &[up, down] {
        let mut p = mig_dir.clone();
        p.push(mig);
        let _ = fs::File::create(&p).map_err(Error::IoCreate)?;
        println!("Created: {:?}", p);
    }
    Ok(())
}


/// Open a repl connection to the given `Config` settings
pub fn shell(config: &Config) -> Result<()> {
    Ok(match config.settings.database_type.as_ref() {
        "sqlite" => {
            let mut db_path = parent_path(&config.path)?;
            db_path.push(&config.settings.database_name);
            let _ = Command::new("sqlite3")
                    .arg(db_path.to_str().unwrap())
                    .spawn().unwrap().wait()
                    .map_err(Error::IoProc)?;
        }
        "postgres" => {
            let conn_str = connect_string(&config.settings)?;
            Command::new("psql")
                    .arg(&conn_str)
                    .spawn().unwrap().wait()
                    .map_err(Error::IoProc)?;
        }
        _ => unreachable!(),
    })
}
