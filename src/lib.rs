#![recursion_limit = "1024"]
#[macro_use] extern crate error_chain;
#[macro_use] extern crate serde_derive;
extern crate serde;
extern crate toml;
extern crate rpassword;
extern crate chrono;
extern crate walkdir;

pub mod errors {
    error_chain! { }
}

use errors::*;
use std::collections::HashMap;
use std::process::Command;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::fs;

use rpassword::read_password;
use walkdir::WalkDir;
use chrono::TimeZone;


static CONFIG_FILE: &'static str = ".migrant.toml";
static DT_FORMAT: &'static str = "%Y%m%d%H%M%S";


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
    fn new(db_type: String, db_name: String, db_user: Option<String>, password: Option<String>) -> Config {
        Config {
            database_type: db_type,
            database_name: db_name,
            database_host: Some("localhost".to_string()),
            database_user: db_user,
            database_password: password,
            migration_location: "migrations".to_string(),
            applied: vec!["some_long_migration_tag".into(), "some_other_migration".into()],
        }
    }

    /// Load toml `.migrant.toml` config file
    pub fn load(dir: &PathBuf) -> Result<Config> {
        let mut file = fs::File::open(dir).chain_err(|| "unable to open config file")?;
        let mut content = String::new();
        file.read_to_string(&mut content).chain_err(|| "unable to read config file")?;
        toml::from_str::<Config>(&content).chain_err(|| "unable to decode config file")
    }

    /// Determines whether new .migrant file location should be in
    /// the given directory or a user specified path
    fn confirm_config_location(mut dir: PathBuf) -> Result<PathBuf> {
        dir.push(".migrant.toml");
        println!(" $ --- No `.migrant.toml` config file found in current path ---");
        println!(" $ A new `.migrant.toml` config file will be created at the following location: ");
        println!(" $  {:?}", dir.display());
        let ans = prompt(" $ Is this ok? (y/n) >> ", false);
        if ans.trim().to_lowercase() == "y" {
            return Ok(dir);
        }
        println!(" $ You can specify the absolute location now, or nothing to exit");
        let ans = prompt(" $ >> ", false);
        if ans.trim().is_empty() { bail!("No `.migrant.toml` path provided"); }
        let path = PathBuf::from(ans);
        if !path.is_absolute() || path.file_name().unwrap() != ".migrant.toml" {
            bail!("Invalid absolute path: {:?}, must end in '.migrant.toml'", path.display());
        }
        Ok(path)
    }

    /// Initialize the current directory
    pub fn init(dir: &PathBuf) -> Result<Config> {
        let config_path = Config::confirm_config_location(dir.clone())
                        .chain_err(|| "unable to create a .migrant.toml config")?;
        let db_type = prompt(" db-type (sqlite|pg) >> ", false);

        let mut db_user = None;
        let mut password = None;
        let mut db_type_full = None;
        match db_type.as_ref() {
            "pg" => db_type_full = Some("postgres"),
            "sqlite" => (),
            _ => bail!("unsupported database type"),
        }

        let db_name;
        if let Some(dbtype) = db_type_full {
            db_name = prompt(" $ database name >> ", false);
            db_user = Some(prompt(&format!(" $ {} database user >> ", dbtype), false));
            password = Some(prompt(&format!(" $ {} user password >> ", dbtype), true));
        } else {
            db_name = prompt(" $ relative path to database (from .migrant.toml config file) >> ", false);
        }
        let config = Config::new(db_type, db_name, db_user, password);
        config.write_to_path(&config_path);
        Ok(config)
    }

    fn write_to_path(&self, path: &PathBuf) -> Result<()> {
        let mut file = fs::File::create(path)
                        .chain_err(|| "unable to create updated config file")?;
        let content = toml::to_string(self).chain_err(|| "Error serialize config")?;
        file.write_all(content.as_bytes())
            .chain_err(|| "unable to write file contents")?;
        Ok(())
    }
}


pub enum Direction {
    Up,
    Down,
}


#[derive(Debug)]
/// Migration meta data
struct Migration {
    stamp: chrono::DateTime<chrono::UTC>,
    up: PathBuf,
    down: PathBuf,
}
impl Migration {
    fn new(up: PathBuf, down: PathBuf, stamp: chrono::DateTime<chrono::UTC>) -> Migration {
        Migration {
            stamp: stamp,
            up: up,
            down: down,
        }
    }
}


/// Generate a postgres connection string
fn connect_string(config: &Config) -> Result<String> {
    let pass = match config.database_password {
        Some(ref pass) => format!(":{}", pass),
        None => "".into(),
    };
    let user = match config.database_user {
        Some(ref user) => user.to_string(),
        None => bail!("config-err: 'database_user' not specified"),
    };
    Ok(format!("postgresql://{}{}@{}/{}",
            user,
            pass,
            config.database_host.as_ref().unwrap_or(&"localhost".to_string()),
            config.database_name))
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
                    .chain_err(|| "failed to run migration command")?
        }
        "pg" => {
            let conn_str = connect_string(config).chain_err(|| "Error generating connection string")?;
            Command::new("psql")
                    .arg(&conn_str)
                    .arg("-f").arg(&filename)
                    .output()
                    .chain_err(|| "failed to run migration command")?
        }
        _ => unreachable!(),
    })
}


/// Display a prompt and return the entered response
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


/// Search for available migrations
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
            let up_down = file_name.skip(1).next().unwrap();
            match up_down {
                "up" => up = mig.clone(),
                "down" => down = mig.clone(),
                _ => (),
            };
        }
        let migration = Migration::new(up, down,
                                       chrono::UTC.datetime_from_str(stamp, DT_FORMAT).unwrap());
        migrations.push(migration);
    }
    // sort, earliest migrations first
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
        let file = mig.up.file_name().unwrap();
        let mig_path = mig.up.to_str().map(String::from).unwrap();
        let x = config.applied.contains(&mig_path);
        println!(" -> [{x}] {name}", x=if x { 'x' } else { ' ' }, name=file.to_str().unwrap());
    }
    Ok(())
}


/// Create a new migration with the given tag
pub fn new(base_dir: &PathBuf, config: &Config, tag: &str) -> Result<()> {
    let now = chrono::UTC::now();
    let dt_string = now.format(DT_FORMAT).to_string();
    let folder = format!("{}_{}", dt_string, tag);
    let mut mig_dir = base_dir.clone();
    mig_dir.push(&config.migration_location);
    mig_dir.push(folder);
    let _ = fs::create_dir_all(&mig_dir);

    let up = format!("up.sql");
    let down = format!("down.sql");
    for mig in [up, down].iter() {
        let mut p = mig_dir.clone();
        p.push(mig);
        let _ = fs::File::create(&p).chain_err(|| "Failed to create file")?;
        println!("Created: {:?}", p);
    }
    Ok(())
}


/// Want something like
pub fn apply_migration(base_dir: &PathBuf, config_path: &PathBuf, config: &Config,
                       direction: Direction, force: bool, fake: bool, all: bool) -> Result<()> {
    unimplemented!()
}

///// Return the next available and unapplied migration
//fn next_available(mig_dir: &PathBuf, applied: &[String]) -> Option<(PathBuf, PathBuf)> {
//    let available = search_for_migrations(mig_dir);
//    for mig in available.iter() {
//        if !applied.contains(&mig.up.to_str().map(String::from).unwrap()) {
//            return Some(
//                (mig.up.clone(), mig.down.clone())
//                );
//        }
//    }
//    None
//}
//
//
//
///// Move up one migration.
///// If `force`, record the migration as a success regardless of the outcome.
///// If `fake`, only update the settings file as if the migration was successful.
//pub fn up(base_dir: &PathBuf, meta_path: &PathBuf, settings: &mut Settings, force: bool, fake: bool, all: bool) -> Result<()> {
//    if let Some((next_available, down)) = next_available(mig_dir, settings.applied.as_slice()) {
//        println!("Applying: {:?}", next_available);
//
//        let mut stdout = String::new();
//        if !fake {
//            let out = runner(&settings, &meta_path, next_available.to_str().unwrap()).chain_err(|| "failed 'up'")?;
//            let success = out.stderr.is_empty();
//            if !success {
//                let info = format!("migrant --up stderr: \n{}",
//                      String::from_utf8(out.stderr)
//                             .chain_err(|| "Error getting stderr string")?);
//                if force {
//                    println!("{}", info);
//                } else {
//                    bail!(info);
//                }
//            }
//            stdout = String::from_utf8(out.stdout).chain_err(|| "Error getting stdout string")?;
//        }
//
//        println!("migrant --up stdout: \n{}", stdout);
//        settings.applied.push(next_available.to_str().unwrap().to_string());
//        settings.down.push(down.to_str().unwrap().to_string());
//        let json = format!("{}", json::as_pretty_json(settings));
//        let mut file = fs::File::create(meta_path)
//                        .chain_err(|| "unable to create file")?;
//        file.write_all(json.as_bytes())
//            .chain_err(|| "unable to write settings file")?;
//    }
//    Ok(())
//}


///// Move down one migration.
///// If `force`, record the migration as a success regardless of the outcome.
///// If `fake`, only update the settings file as if the migration was successful.
//pub fn down(meta_path: &PathBuf, settings: &mut Settings, force: bool, fake: bool) -> Result<()> {
//    if let Some(last) = settings.down.pop() {
//        println!("Onto database: {}", settings.database_name);
//        println!("Applying: {}", last);
//
//        let mut stdout = String::new();
//        if !fake {
//            let out = runner(&settings, &meta_path, &last).chain_err(|| "failed 'down'")?;
//            let success = out.stderr.is_empty();
//            if !success {
//                let info = format!("migrant --down stderr: \n{}",
//                      String::from_utf8(out.stderr)
//                             .chain_err(|| "Error getting stderr string")?);
//                if force {
//                    println!("{}", info);
//                } else {
//                    bail!(info);
//                }
//            }
//            stdout = String::from_utf8(out.stdout).chain_err(|| "Error getting stdout string")?;
//        }
//
//        println!("migrant --down stdout: \n{}", stdout);
//        settings.applied.pop();
//        let json = format!("{}", json::as_pretty_json(settings));
//        let mut file = fs::File::create(meta_path)
//                         .chain_err(|| "unable to create file")?;
//        file.write_all(json.as_bytes())
//            .chain_err(|| "unable to write settings file")?;
//    }
//    Ok(())
//}


///// Open a repl connection to the specified database connection
//pub fn shell(meta_path: &PathBuf, settings: Settings) -> Result<()> {
//    Ok(match settings.database_type.as_ref() {
//        "sqlite" => {
//            let mut db_path = meta_path.clone();
//            db_path.pop();
//            db_path.push(&settings.database_name);
//            let _ = Command::new("sqlite3")
//                    .arg(db_path.to_str().unwrap())
//                    .spawn().unwrap().wait()
//                    .chain_err(|| "failed to run migration command")?;
//        }
//        "pg" => {
//            let conn_str = connect_string(&settings);
//            Command::new("psql")
//                    .arg(&conn_str)
//                    .spawn().unwrap().wait()
//                    .chain_err(|| "failed to run shell")?;
//        }
//        _ => unreachable!(),
//    })
//}
