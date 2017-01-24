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
use std::io::{self, Write};
use std::path::PathBuf;
use std::fs;

use rpassword::read_password;
use rustc_serialize::json;
use walkdir::WalkDir;
use chrono::TimeZone;


static META_FILE: &'static str = ".migrant";
static DT_FORMAT: &'static str = "%Y%m%d-%H%M%S";


#[derive(RustcEncodable, RustcDecodable, Debug, Clone)]
/// Settings that are serialized and saved in a project `.migrant` file
pub struct Settings {
    database_type: String,
    database_name: String,
    db_user: String,
    password: String,
    migration_folder: String,
    applied: Vec<String>,
    down: Vec<String>,
}
impl Settings {
    fn new(db_type: String, db_name: String, db_user: String, password: String) -> Settings {
        Settings {
            database_type: db_type,
            database_name: db_name,
            db_user: db_user,
            password: password,
            migration_folder: "resources/migrations".to_string(),
            applied: vec![],
            down: vec![],
        }
    }
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


fn runner(settings: &Settings, meta_path: &PathBuf, filename: &str) -> Result<std::process::Output> {
    Ok(match settings.database_type.as_ref() {
        "sqlite" => {
            let mut db_path = meta_path.clone();
            db_path.pop();
            db_path.push(&settings.database_name);
            Command::new("sqlite3")
                    .arg(db_path.to_str().unwrap())
                    .arg(&format!(".read {}", filename))
                    .output()
                    .chain_err(|| "failed to run migration command")?
        }
        "pg" => {
            Command::new("psql")
                    .arg("-U").arg(&settings.db_user)
                    .arg("-d").arg(&settings.database_name)
                    .arg("-h").arg("localhost")
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
fn search_for_migrations(mig_root: &PathBuf) -> Vec<Migration> {
    // collect any .sql files into a Map<`stamp-tag`, Vec<up&down files>>
    let mut files = HashMap::new();
    for dir in WalkDir::new(mig_root).into_iter() {
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
        let mut up = PathBuf::from(path);
        let mut down = PathBuf::from(path);
        let stamp_string = chrono::UTC.ymd(2000, 1, 1).and_hms(0, 0, 0).format(DT_FORMAT).to_string();
        let mut stamp = stamp_string.as_str();
        for mig in migs.iter() {
            let mut file_name = mig.file_name().and_then(|p| p.to_str()).unwrap().split('.');
            stamp = file_name.next().unwrap();
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
    let db_type = prompt(" db-type (sqlite|pg) >> ", false);
    let db_name;
    let mut db_user = "".into();
    let mut password = "".into();
    let mut db_pref = None;
    match db_type.as_ref() {
        "pg" => db_pref = Some("postgres"),
        "sqlite" => (),
        _ => bail!("unsupported database type"),
    }
    if let Some(pref) = db_pref {
        db_name = prompt(" $ database name >> ", false);
        db_user = prompt(&format!(" $ {} database user >> ", pref), false);
        password = prompt(&format!(" $ {} user password >> ", pref), true);
    } else {
        db_name = prompt(" $ relative path to database (from .migrant file) >> ", false);
    }
    let settings = Settings::new(db_type, db_name, db_user, password);
    let json = format!("{}", json::as_pretty_json(&settings));
    let mut file = fs::File::create(meta)
                    .chain_err(|| "unable to create new file")?;
    file.write_all(json.as_bytes())
        .chain_err(|| "unable to write file contents")?;
    Ok(())
}


/// List the currently applied and available migrations under settings.migration_folder
pub fn list(mig_dir: &PathBuf, settings: &Settings) -> Result<()> {
    let available = search_for_migrations(mig_dir);
    println!("Current Migration Status:");
    for mig in available.iter() {
        let file = mig.up.file_name().unwrap();
        let mig_path = mig.up.to_str().map(String::from).unwrap();
        let x = settings.applied.contains(&mig_path);
        println!(" -> [{x}] {name}", x=if x { 'x' } else { ' ' }, name=file.to_str().unwrap());
    }
    Ok(())
}


/// Return the next available and unapplied migration
fn next_available(mig_dir: &PathBuf, applied: &[String]) -> Option<(PathBuf, PathBuf)> {
    let available = search_for_migrations(mig_dir);
    for mig in available.iter() {
        if !applied.contains(&mig.up.to_str().map(String::from).unwrap()) {
            return Some(
                (mig.up.clone(), mig.down.clone())
                );
        }
    }
    None
}


/// Move up one migration.
/// If `force`, record the migration as a success regardless of the outcome.
/// If `fake`, only update the settings file as if the migration was successful.
pub fn up(mig_dir: &PathBuf, meta_path: &PathBuf, settings: &mut Settings, force: bool, fake: bool) -> Result<()> {
    if let Some((next_available, down)) = next_available(mig_dir, settings.applied.as_slice()) {
        println!("Applying: {:?}", next_available);

        let mut stdout = String::new();
        if !fake {
            let out = runner(&settings, &meta_path, next_available.to_str().unwrap()).chain_err(|| "failed 'up'")?;
            let success = out.stderr.is_empty();
            if !success {
                let info = format!("migrant --up stderr: {}",
                      String::from_utf8(out.stderr)
                             .chain_err(|| "Error getting stderr string")?);
                if force {
                    println!("{}", info);
                } else {
                    bail!("{}", info);
                }
            }
            stdout = String::from_utf8(out.stdout).chain_err(|| "Error getting stdout string")?;
        }

        println!("migrant --up stdout: {}", stdout);
        settings.applied.push(next_available.to_str().unwrap().to_string());
        settings.down.push(down.to_str().unwrap().to_string());
        let json = format!("{}", json::as_pretty_json(settings));
        let mut file = fs::File::create(meta_path)
                        .chain_err(|| "unable to create file")?;
        file.write_all(json.as_bytes())
            .chain_err(|| "unable to write settings file")?;
    }
    Ok(())
}


/// Move down one migration.
/// If `force`, record the migration as a success regardless of the outcome.
/// If `fake`, only update the settings file as if the migration was successful.
pub fn down(meta_path: &PathBuf, settings: &mut Settings, force: bool, fake: bool) -> Result<()> {
    if let Some(last) = settings.down.pop() {
        println!("Onto database: {}", settings.database_name);
        println!("Applying: {}", last);

        let mut stdout = String::new();
        if !fake {
            let out = runner(&settings, &meta_path, &last).chain_err(|| "failed 'down'")?;
            let success = out.stderr.is_empty();
            if !success {
                let info = format!("migrant --down stderr: {}",
                      String::from_utf8(out.stderr)
                             .chain_err(|| "Error getting stderr string")?);
                if force {
                    println!("{}", info);
                } else {
                    bail!("{}", info);
                }
            }
            stdout = String::from_utf8(out.stdout).chain_err(|| "Error getting stdout string")?;
        }

        println!("migrant --down stdout: {}", stdout);
        settings.applied.pop();
        let json = format!("{}", json::as_pretty_json(settings));
        let mut file = fs::File::create(meta_path)
                         .chain_err(|| "unable to create file")?;
        file.write_all(json.as_bytes())
            .chain_err(|| "unable to write settings file")?;
    }
    Ok(())
}


/// Create a new migration with the given tag
pub fn new(mig_dir: &mut PathBuf, settings: &mut Settings, tag: &str) -> Result<()> {
    let now = chrono::UTC::now();
    let dt_string = now.format(DT_FORMAT).to_string();
    let folder = format!("{}.{}", dt_string, tag);
    mig_dir.push(&settings.migration_folder);
    mig_dir.push(folder);
    let _ = fs::create_dir_all(&mig_dir);

    let up = format!("{}.{}.{}.sql", dt_string, tag, "up");
    let down = format!("{}.{}.{}.sql", dt_string, tag, "down");
    let migs = vec![up, down];
    for mig in migs.iter() {
        let mut p = mig_dir.clone();
        p.push(mig);
        let _ = fs::File::create(&p).chain_err(|| "Failed to create file")?;
        println!("Created: {:?}", p);
    }
    Ok(())
}


/// Open a repl connection to the specified database connection
pub fn shell(meta_path: &PathBuf, settings: Settings) -> Result<()> {
    Ok(match settings.database_type.as_ref() {
        "sqlite" => {
            let mut db_path = meta_path.clone();
            db_path.pop();
            db_path.push(&settings.database_name);
            let _ = Command::new("sqlite3")
                    .arg(db_path.to_str().unwrap())
                    .spawn().unwrap().wait()
                    .chain_err(|| "failed to run migration command")?;
        }
        "pg" => {
            Command::new("psql")
                    .arg("-U").arg(&settings.db_user)
                    .arg("-d").arg(&settings.database_name)
                    .arg("-h").arg("localhost")
                    .spawn().unwrap().wait()
                    .chain_err(|| "failed to run shell")?;
        }
        _ => unreachable!(),
    })
}
