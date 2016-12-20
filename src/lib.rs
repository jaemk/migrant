#![recursion_limit = "1024"]
#[macro_use]
extern crate error_chain;
extern crate rpassword;
extern crate rustc_serialize;
extern crate chrono;
extern crate walkdir;

pub mod errors {
    error_chain! { }
}

use errors::*;
use std::collections::HashMap;
use std::process::Command;
use std::io::{self, Write, Read};
use std::path::PathBuf;
use std::fs;

use rpassword::read_password;
use rustc_serialize::json;
use walkdir::WalkDir;
use chrono::TimeZone;


static META_FILE: &'static str = ".migrant";
static DT_FORMAT: &'static str = "%Y%m%d-%H%M%S";


macro_rules! try_get_settings {
     ($dir:expr, $n:expr) => {
        match search_for_meta($dir, $n) {
            Some(m) => {
                let mut file = fs::File::open(m).unwrap();
                let mut json = String::new();
                file.read_to_string(&mut json).unwrap();
                json::decode::<Settings>(&json).unwrap()
            }
            _ => {
                bail!("No `.migrant` file found. Try migrant --init");
            }
        }
     }
}

macro_rules! try_base_dir {
    ($dir: expr, $n:expr) => {
        match search_for_meta($dir, $n) {
            Some(mut m) => {
                m.pop();
                m
            }
            _ => bail!("unable to determine project base directory"),
        }
    }
}


#[derive(RustcEncodable, RustcDecodable, Debug, Clone)]
struct Settings {
    username: String,
    password: String,
    migration_folder: String,
    applied: Vec<String>,
    down: Vec<String>,
}
impl Settings {
    fn new(username: String, password: String) -> Settings {
        Settings {
            username: username,
            password: password,
            migration_folder: "resources/migrations".to_string(),
            applied: vec![],
            down: vec![],
        }
    }
}


#[derive(Debug)]
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
pub fn search_for_meta(dir: &PathBuf, parents: u32) -> Option<PathBuf> {
    let mut dir = dir.clone();
    let mut count = 0;
    loop {
        for path in fs::read_dir(&dir).unwrap() {
            let path = path.unwrap().path();
            if let Some(file) = path.file_name() {
                if file == META_FILE { return Some(path.clone()); }
            }
        }
        if dir.parent().is_some() {
            dir.pop();
        } else {
            return None;
        }
        if count == parents { break; }
        count += 1;
    }
    None
}


/// Search for available migrations
fn search_for_migrations(dir: &str) -> Vec<Migration> {
    let root = PathBuf::from(dir);
    let mut files = HashMap::new();
    for dir in WalkDir::new(root).into_iter() {
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
    let mut migrations = vec![];
    for (path, migs) in files.iter() {
        let mut up = PathBuf::from(path);
        let mut down = PathBuf::from(path);
        let stamp_string = chrono::UTC.ymd(2000, 1, 1).and_hms(0, 0, 0).format(DT_FORMAT).to_string();
        let mut stamp = stamp_string.as_str();
        for mig in migs.iter() {
            stamp = mig.file_name().unwrap().to_str().unwrap().split('.').nth(0).unwrap();
            let up_down = mig.file_name().unwrap().to_str().unwrap().split('.').nth(2).unwrap();
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
    migrations.sort_by(|a, b| a.stamp.cmp(&b.stamp));
    migrations
}


/// Determines whether new .migrant file location should be in
/// the given directory or a user specified path
fn new_meta_location(mut dir: PathBuf) -> Result<PathBuf> {
    dir.push(".migrant");
    println!(" $ --- No `.migrant` file found in parent path ---");
    println!(" $ A new `.migrant` file will be created at the following location: ");
    println!(" $  {:?}", dir.display());
    let ans = prompt(" $ Is this ok? (y/n) >> ", false);
    if ans.trim().to_lowercase() == "y" {
        return Ok(dir);
    }
    println!(" $ You can specify the absolute location now, or nothing to exit");
    let ans = prompt(" $ >> ", false);
    if ans.trim().is_empty() { bail!("No `.migrant` path provided"); }
    let path = PathBuf::from(ans);
    if !path.is_absolute() || path.file_name().unwrap() != ".migrant" {
        return bail!(format!("Invalid absolute path: {:?}", path.display()));
    }
    Ok(path)
}


/// Initialize the current directory
pub fn init(dir: &PathBuf) -> Result<()> {
    let meta = new_meta_location(dir.clone())
                    .chain_err(|| "unable to create a .migrant file")?;
    let username = prompt(" $ pg-login/project-name >> ", false);
    let password = prompt(" $ pg-password >> ", true);

    let settings = Settings::new(username, password);
    let json = format!("{}", json::as_pretty_json(&settings));
    let mut file = fs::File::create(meta)
                    .chain_err(|| "unable to create new file")?;
    file.write_all(json.as_bytes())
        .chain_err(|| "unable to write file contents")?;
    Ok(())
}


/// List the currently applied and available migrations under settings.migration_folder
pub fn list(dir: &PathBuf) -> Result<()> {
    let settings = try_get_settings!(dir, 2);
    let available = search_for_migrations(&settings.migration_folder);
    println!("Current Migration Status:");
    for mig in available.iter() {
        let file = mig.up.file_name().unwrap();
        let x = settings.applied.contains(&mig.up.to_str().unwrap().to_string());
        println!(" -> [{x}] {name}", x=if x { 'x' } else { ' ' }, name=file.to_str().unwrap());
    }
    Ok(())
}


/// Return the next available and unapplied migration
fn next_available(settings: &Settings) -> Option<(PathBuf, PathBuf)> {
    let available = search_for_migrations(settings.migration_folder.as_str());
    for mig in available.iter() {
        if !settings.applied.contains(&mig.up.to_str().unwrap().to_string()) {
            return Some(
                (mig.up.clone(), mig.down.clone())
                );
        }
    }
    None
}


/// Move up one migration. If `force`, record the migration
/// as a success regardless of the outcome.
pub fn up(dir: &PathBuf, force: bool) -> Result<()> {
    let settings = try_get_settings!(dir, 2);
    if let Some((next_available, down)) = next_available(&settings) {
        println!("Applying: {:?}", next_available);
        let out = Command::new("psql")
                          .arg("-U").arg(&settings.username)
                          .arg("-d").arg(&settings.username)
                          .arg("-h").arg("localhost")
                          .arg("-f").arg(next_available.to_str().unwrap())
                          .output()
                          .chain_err(|| "failed to run 'up' migration command")?;
        let success = out.stderr.is_empty();
        if success || force {
            println!("psql --up stdout: {}",
                     String::from_utf8(out.stdout)
                            .chain_err(|| "Error getting stdout string")?);
            let mut settings = settings.clone();
            settings.applied.push(next_available.to_str().unwrap().to_string());
            settings.down.push(down.to_str().unwrap().to_string());
            let meta_path = search_for_meta(dir, 2).unwrap();
            let json = format!("{}", json::as_pretty_json(&settings));
            let mut file = fs::File::create(meta_path)
                            .chain_err(|| "unable to create file")?;
            file.write_all(json.as_bytes())
                .chain_err(|| "unable to write settings file")?;
        } else if !success && !force {
            println!("psql --up stderr: {}",
                     String::from_utf8(out.stderr)
                            .chain_err(|| "Error getting stderr string")?);
        }
    }
    Ok(())
}


/// Move down one migration. If `force`, record the migration
/// as a success regardless of the outcome
pub fn down(dir: &PathBuf, force: bool) -> Result<()> {
    let mut settings = try_get_settings!(dir, 2);
    if let Some(last) = settings.down.pop() {
        println!("Applying: {}", last);
        let out = Command::new("psql")
                          .arg("-U").arg(&settings.username)
                          .arg("-d").arg(&settings.username)
                          .arg("-h").arg("localhost")
                          .arg("-f").arg(last)
                          .output()
                          .chain_err(|| "failed to run 'down' migration command")?;
        let success = out.stderr.is_empty();
        if success || force {
            println!("psql --down stdout: {}",
                     String::from_utf8(out.stdout)
                            .chain_err(|| "Error getting stdout string")?);
            let meta_path = search_for_meta(dir, 2).unwrap();
            settings.applied.pop();
            let json = format!("{}", json::as_pretty_json(&settings));
            let mut file = fs::File::create(meta_path)
                             .chain_err(|| "unable to create file")?;
            file.write_all(json.as_bytes())
                .chain_err(|| "unable to write settings file")?;
        } else if !success && !force {
            println!("psql --down stderr: {}",
                     String::from_utf8(out.stderr)
                            .chain_err(|| "Error getting stderr string")?);
        }
    }
    Ok(())
}


/// Create a new migration with the given tag
pub fn new(dir: &PathBuf, tag: &str) -> Result<()> {
    let settings = try_get_settings!(dir, 2);
    let mut migration_dir = try_base_dir!(dir, 2);
    migration_dir.push(settings.migration_folder);

    let now = chrono::UTC::now();
    let dt_string = now.format(DT_FORMAT).to_string();
    let folder = format!("{}.{}", dt_string, tag);
    migration_dir.push(folder);
    let _ = fs::create_dir_all(&migration_dir);

    let up = format!("{}.{}.{}.sql", dt_string, tag, "up");
    let down = format!("{}.{}.{}.sql", dt_string, tag, "down");
    let migs = vec![up, down];
    for mig in migs.iter() {
        let mut p = migration_dir.clone();
        p.push(mig);
        let _ = fs::File::create(&p).chain_err(|| "Failed to create file")?;
        println!("Created: {:?}", p);
    }
    Ok(())
}


/// Open a repl connection to the specified database connection
pub fn shell(dir: &PathBuf) -> Result<()> {
    let settings = try_get_settings!(dir, 2);
    let out = Command::new("psql")
                      .arg("-U").arg(&settings.username)
                      .arg("-d").arg(&settings.username)
                      .arg("-h").arg("localhost")
                      .spawn();
    out.unwrap().wait().chain_err(|| "Failed to execute shell")?;
    Ok(())
}
