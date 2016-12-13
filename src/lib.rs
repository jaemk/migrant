extern crate rpassword;
extern crate rustc_serialize;
extern crate chrono;
extern crate walkdir;

use std::collections::HashMap;
use std::io::{self, Write, Read};
use std::path::PathBuf;
use std::fs;

use rpassword::read_password;
use rustc_serialize::json;
use walkdir::WalkDir;


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
                println!("No `.migrant` file found. Try migrant --init");
                return;
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
            _ => return,
        }
    }
}

#[derive(RustcEncodable, RustcDecodable, Debug)]
struct Settings {
    username: String,
    password: String,
    migration_folder: String,
    applied: Vec<String>,
}
impl Settings {
    fn new(username: String, password: String) -> Settings {
        Settings {
            username: username,
            password: password,
            migration_folder: "resources/migrations".to_string(),
            applied: vec![],
        }
    }
}


struct Migration {
    stamp: chrono::DateTime<chrono::UTC>,
    up: PathBuf,
    down: PathBuf,
}
impl Migration {
    fn new(up: &str, down: &str, stamp: chrono::DateTime<chrono::UTC>) -> Migration {
        Migration {
            stamp: stamp,
            up: PathBuf::from(up),
            down: PathBuf::from(down),
        }
    }
}


pub fn enter() {
    let stdin = io::stdin();
    let mut resp = String::new();
    let _ = stdin.read_line(&mut resp);
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
fn search_for_meta(dir: &PathBuf, parents: u32) -> Option<PathBuf> {
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
    // collect .ups & .downs
    let root = PathBuf::from(dir);
    let mut migs = HashMap::new();
    for dir in WalkDir::new(root).into_iter() {
        let e = dir.unwrap();
        let path = e.path();
        if let Some(ext) = path.extension() {
            if ext.is_empty() || ext != "sql" { continue; }
            let parent = path.parent().unwrap();
            let key = format!("{}", parent.display());
            let entry = migs.entry(key).or_insert(vec![]);
            entry.push(path.to_path_buf());
        }
    }
    println!("{:#?}", migs);
    vec![Migration::new("", "", chrono::UTC::now())]
}


/// Creates a new .migrant file in the given directory or
/// asks for a specific location to put it
fn create_meta(mut dir: PathBuf) -> Result<PathBuf, String> {
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
    if ans.trim().is_empty() { return Err("No `.migrant` path provided".to_string()); }
    let path = PathBuf::from(ans);
    if !path.is_absolute() || path.file_name().unwrap() != ".migrant" {
        return Err(format!("Invalid absolute path: {:?}", path.display()));
    }
    Ok(path)
}


/// Initialize the current directory
pub fn init(dir: &PathBuf) {
    // check if meta-file already exists
    let meta = create_meta(dir.clone());
    if meta.is_err() {
        println!(" >> Err: {}", meta.err().unwrap());
        return;
    }
    let username = prompt(" $ pg-login/project-name >> ", false);
    let password = prompt(" $ pg-password >> ", true);
    let settings = Settings::new(username, password);
    let json = format!("{}", json::as_pretty_json(&settings));
    let mut file = fs::File::create(meta.unwrap()).unwrap();
    file.write_all(json.as_bytes()).unwrap();
}


/// List the currently applied and available migrations under settings.migration_folder
pub fn list(dir: &PathBuf) {
    let settings = try_get_settings!(dir, 2);
    let _available = search_for_migrations(&settings.migration_folder);
}


pub fn new(dir: &PathBuf, tag: &str) {
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
        let _ = fs::File::create(&p).unwrap();
        println!("Created: {:?}", p);
    }
}
