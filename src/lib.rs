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
use std::path::PathBuf;
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
    static ref TAG_RE: Regex = Regex::new(r"[^a-z0-9-]+").unwrap();
    static ref MIG_RE: Regex = Regex::new(r##"(?P<mig>".*")"##).unwrap();
}


#[derive(Serialize, Deserialize, Debug, Clone)]
/// Settings that are serialized and saved in a project `.migrant.toml` file
pub struct Config {
    database_type: String,
    database_name: String,
    database_host: Option<String>,
    database_user: Option<String>,
    database_password: Option<String>,
    migration_location: String,
    applied: Vec<String>,
}
impl Config {
    /// Create a new config
    fn new(db_type: String, db_name: String, db_user: Option<String>, password: Option<String>) -> Config {
        Config {
            database_type: db_type,
            database_name: db_name,
            database_host: Some("localhost".to_string()),
            database_user: db_user,
            database_password: password,
            migration_location: "migrations".to_string(),
            applied: vec![],
        }
    }

    /// Load toml `.migrant.toml` config file
    pub fn load(dir: &PathBuf) -> Result<Config> {
        let mut file = fs::File::open(dir).map_err(Error::IoOpen)?;
        let mut content = String::new();
        file.read_to_string(&mut content).map_err(Error::IoRead)?;
        toml::from_str::<Config>(&content).map_err(Error::TomlDe)
    }

    /// Determines whether new .migrant file location should be in
    /// the given directory or a user specified path
    fn confirm_config_location(mut dir: PathBuf) -> Result<PathBuf> {
        dir.push(".migrant.toml");
        println!(" $ A new `.migrant.toml` config file will be created at the following location: ");
        println!(" $  {:?}", dir.display());
        let ans = prompt(" $ Is this ok? (y/n) >> ", false);
        if ans.trim().to_lowercase() == "y" {
            return Ok(dir);
        }

        println!(" $ You can specify the absolute location now, or nothing to exit");
        let ans = prompt(" $ >> ", false);
        if ans.trim().is_empty() {
            bail!(Config <- "No `.migrant.toml` path provided".into())
        }

        let path = PathBuf::from(ans);
        if !path.is_absolute() || path.file_name().unwrap() != ".migrant.toml" {
            bail!(Config <- format!("Invalid absolute path: {}, must end in '.migrant.toml'", path.display()));
        }
        Ok(path)
    }

    /// Initialize project in the current directory
    pub fn init(dir: &PathBuf) -> Result<Config> {
        let config_path = Config::confirm_config_location(dir.clone())
            .map_err(|e| Error::Config(format!("unable to create a .migrant.toml config -> {}", e)))?;

        let db_type = prompt(" db-type (sqlite|postgres) >> ", false);
        match db_type.as_ref() {
            "postgres" | "sqlite" => (),
            e @ _ => bail!(Config <- format!("unsupported database type: {}", e)),
        }

        let db_name;
        let mut db_user = None;
        let mut password = None;
        if db_type != "sqlite" {
            db_name = prompt(" $ database name >> ", false);
            db_user = Some(prompt(&format!(" $ {} database user >> ", &db_type), false));
            password = Some(prompt(&format!(" $ {} user password >> ", &db_type), true));
        } else {
            db_name = prompt(" $ relative path to database (from .migrant.toml config file) >> ", false);
        }

        let config = Config::new(db_type, db_name, db_user, password);
        config.write_to_path(&config_path)?;
        Ok(config)
    }

    /// Write the current config to the given file path
    fn write_to_path(&self, path: &PathBuf) -> Result<()> {
        let mut file = fs::File::create(path)
                        .map_err(Error::IoCreate)?;
        let content = toml::to_string(self).map_err(Error::TomlSe)?;
        let content = content.lines().map(|line| {
            if !line.starts_with("applied") { line.to_string() }
            else {
                // format the list of applied migrations nicely
                let migs = MIG_RE.captures_iter(line)
                    .map(|cap| format!("    {}", &cap["mig"]))
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("applied = [\n{}\n]", migs)
            }
        }).collect::<Vec<_>>().join("\n");
        file.write_all(content.as_bytes())
            .map_err(Error::IoWrite)?;
        Ok(())
    }
}


/// Represents direction to apply migrations.
/// `Up`   -> up.sql
/// `Down` -> down.sql
pub enum Direction {
    Up,
    Down,
}


#[derive(Debug)]
/// Migration meta data
struct Migration {
    stamp: chrono::DateTime<chrono::UTC>,
    dir: PathBuf,
    up: PathBuf,
    down: PathBuf,
}


/// Generate a database connection string
fn connect_string(config: &Config) -> Result<String> {
    let pass = match config.database_password {
        Some(ref pass) => format!(":{}", pass),
        None => "".into(),
    };
    let user = match config.database_user {
        Some(ref user) => user.to_string(),
        None => bail!(Config <- "config-err: 'database_user' not specified".into()),
    };
    Ok(format!("{db_type}://{user}{pass}@{host}/{db_name}",
            db_type=config.database_type,
            user=user,
            pass=pass,
            host=config.database_host.as_ref().unwrap_or(&"localhost".to_string()),
            db_name=config.database_name))
}


/// Run a given migration file through either sqlite or postgres, returning the output
fn runner(config: &Config, config_path: &PathBuf, filename: &str) -> Result<std::process::Output> {
    Ok(match config.database_type.as_ref() {
        "sqlite" => {
            let mut db_path = config_path.clone();
            db_path.pop();
            db_path.push(&config.database_name);
            Command::new("sqlite3")
                    .arg(db_path.to_str().unwrap())
                    .arg(&format!(".read {}", filename))
                    .output()
                    .map_err(Error::IoProc)?
        }
        "postgres" => {
            let conn_str = connect_string(config)?;
            Command::new("psql")
                    .arg(&conn_str)
                    .arg("-f").arg(&filename)
                    .output()
                    .map_err(Error::IoProc)?
        }
        _ => unreachable!(),
    })
}


/// Display a prompt and return the entered response.
/// If `secure`, conceal the input.
fn prompt(msg: &str, secure: bool) -> String {
    print!("{}", msg);
    let _ = io::stdout().flush();

    if secure {
        read_password().unwrap()
    } else {
        let stdin = io::stdin();
        let mut resp = String::new();
        let _ = stdin.read_line(&mut resp);
        resp.trim().to_string()
    }
}


/// Search for a .migrant file in the current directory,
/// looking up the parent path until it finds one.
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
            let entry = files.entry(key).or_insert(vec![]);
            entry.push(path.to_path_buf());
        }
    }

    // transform up&down files into a Vec<Migration>
    let mut migrations = vec![];
    for (path, migs) in files.iter() {
        let stamp = PathBuf::from(path);
        let mut stamp = stamp.file_name().and_then(|p| p.to_str()).unwrap().split('_');
        let stamp = stamp.next().unwrap();

        let mut up = PathBuf::from(path);
        let mut down = PathBuf::from(path);

        for mig in migs.iter() {
            let mut file_name = mig.file_name().and_then(|p| p.to_str()).unwrap().split('.');
            let up_down = file_name.next().unwrap();
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



/// List the currently applied and available migrations under `config.migration_location`
pub fn list(config: &Config, base_dir: &PathBuf) -> Result<()> {
    let mut mig_dir = base_dir.clone();
    mig_dir.push(PathBuf::from(&config.migration_location));
    let available = search_for_migrations(&mig_dir);
    if available.is_empty() {
        println!("No migrations found under {:?}", &mig_dir);
        return Ok(())
    }
    println!("Current Migration Status:");
    for mig in available.iter() {
        let file = mig.up.parent().unwrap().iter().rev().next().unwrap();
        let mig_path = mig.up.parent().and_then(|p| p.to_str()).map(String::from).unwrap();
        let x = config.applied.contains(&mig_path);
        println!(" -> [{x}] {name}", x=if x { 'x' } else { ' ' }, name=file.to_str().unwrap());
    }
    Ok(())
}


/// Create a new migration with the given tag
pub fn new(base_dir: &PathBuf, config: &Config, tag: &str) -> Result<()> {
    if TAG_RE.is_match(&tag) {
        bail!(Migration <- format!("Invalid tag format. Tags can contain [a-z0-9-]"));
    }
    let now = chrono::UTC::now();
    let dt_string = now.format(DT_FORMAT).to_string();
    let folder = format!("{stamp}_{tag}", stamp=dt_string, tag=tag);
    let mut mig_dir = base_dir.clone();
    mig_dir.push(&config.migration_location);
    mig_dir.push(folder);
    let _ = fs::create_dir_all(&mig_dir);

    let up = format!("up.sql");
    let down = format!("down.sql");
    for mig in [up, down].iter() {
        let mut p = mig_dir.clone();
        p.push(mig);
        let _ = fs::File::create(&p).map_err(Error::IoCreate)?;
        println!("Created: {:?}", p);
    }
    Ok(())
}


/// Return the next available up or down migration
fn next_available(direction: &Direction, mig_dir: &PathBuf, applied: &[String]) -> Option<PathBuf> {
    match direction {
        &Direction::Up => {
            let available = search_for_migrations(mig_dir);
            for mig in available.iter() {
                if !applied.contains(&mig.dir.to_str().map(String::from).unwrap()) {
                    return Some(mig.up.clone())
                }
            }
            None
        }
        &Direction::Down => {
            applied.last()
                .map(PathBuf::from)
                .map(|mut pb| {
                    pb.push("down.sql");
                    pb
                })
        }
    }
}


/// Try applying the next available migration in the specified `Direction`
pub fn apply_migration(base_dir: &PathBuf, config_path: &PathBuf, config: &Config, direction: Direction,
                       force: bool, fake: bool, all: bool) -> Result<()> {
    let mut mig_dir = base_dir.clone();
    mig_dir.push(PathBuf::from(&config.migration_location));

    let new_config = match next_available(&direction, &mig_dir, config.applied.as_slice()) {
        None => bail!(MigrationNotFound <- format!("No un-applied migration found in {}", config.migration_location)),
        Some(next) => {
            println!("Applying: {:?}", next);

            let mut stdout = String::new();
            if !fake {
                let out = runner(config, config_path, next.to_str().unwrap())
                    .map_err(|e| Error::Migration(format!("Error occurred while running migration -> {}", e)))?;

                let success = out.status.success();
                if !success {
                    let info = format!(" ** Error **\n{}",
                          String::from_utf8(out.stderr)
                                 .map_err(Error::Utf8Error)?);
                    if force {
                        println!("{}", info);
                    } else {
                        bail!(Migration <- format!("Migration was unsuccessful...\n{}", info));
                    }
                }
                stdout = String::from_utf8(out.stdout).map_err(Error::Utf8Error)?;
            }

            if !stdout.is_empty() {
                println!("{}", stdout);
            }

            let mut config = config.clone();
            match direction {
                Direction::Up => {
                    config.applied.push(next.parent().unwrap().to_str().unwrap().to_string());
                    config.write_to_path(&config_path)?;
                }
                Direction::Down => {
                    config.applied.pop();
                    config.write_to_path(&config_path)?;
                }
            }
            config
        }
    };

    if all {
        let res = apply_migration(base_dir, config_path, &new_config, direction, force, fake, all);
        match res {
            Ok(_) => (),
            Err(error) => match error {
                Error::MigrationNotFound(_) => (),
                e @ _ => return Err(e),
            }
        }
    }
    Ok(())
}


/// Open a repl connection to the specified database connection
pub fn shell(base_dir: &PathBuf, config: &Config) -> Result<()> {
    Ok(match config.database_type.as_ref() {
        "sqlite" => {
            let mut db_path = base_dir.clone();
            db_path.push(&config.database_name);
            let _ = Command::new("sqlite3")
                    .arg(db_path.to_str().unwrap())
                    .spawn().unwrap().wait()
                    .map_err(Error::IoProc)?;
        }
        "postgres" => {
            let conn_str = connect_string(&config)?;
            Command::new("psql")
                    .arg(&conn_str)
                    .spawn().unwrap().wait()
                    .map_err(Error::IoProc)?;
        }
        _ => unreachable!(),
    })
}
