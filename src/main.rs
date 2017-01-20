extern crate clap;
extern crate migrant;
extern crate rustc_serialize;

use std::env;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use clap::{Arg, App};
use rustc_serialize::json;
use migrant::errors::*;
use migrant::Settings;


// number of parent directories to look back through for a .migrant file
const N_PARENTS: u32 = 3;


fn main() {
    let matches = App::new("Migrant")
        .version("0.2.1")
        .author("James K. <james.kominick@gmail.com>")
        .about("Postgres migration manager")
        .arg(Arg::with_name("init")
            .long("init")
            .help("Initialize project"))
        .arg(Arg::with_name("list")
            .short("l")
            .long("list")
            .help("List status of applied and available migrations"))
        .arg(Arg::with_name("up")
            .short("u")
            .long("up")
            .help("Moves up (applies .up.sql) one migration"))
        .arg(Arg::with_name("down")
            .short("d")
            .long("down")
            .help("Moves down (applies .down.sql) one migration"))
        .arg(Arg::with_name("force")
            .long("force")
            .help("Applies the migration and treats it as if it were successful"))
        .arg(Arg::with_name("fake")
            .long("fake")
            .help("Updates the .meta file as if the specified migration was applied"))
        .arg(Arg::with_name("new")
            .short("n")
            .long("new")
            .help("Creates a new migrations folder with up&down templates")
            .takes_value(true)
            .value_name("MIGRATION_TAG"))
        .arg(Arg::with_name("shell")
            .short("s")
            .long("shell")
            .help("Open a repl connection"))
        .get_matches();

    let dir = env::current_dir().expect("Unable to retrieve current directory");

    if let Err(ref e) = run(dir, matches) {
        println!("error: {}", e);
        for e in e.iter().skip(1) {
            println!("caused by: {}", e);
        }
        // if RUST_BACKTRACE=1
        if let Some(backtrace) = e.backtrace() {
            println!("backtrace: {:?}", backtrace);
        }
        ::std::process::exit(1);
    }
}


/// Load json `.migrant` settings file
fn load_settings(dir: &PathBuf) -> Result<Settings> {
    let mut file = fs::File::open(dir).chain_err(|| "unable to open settings file")?;
    let mut json = String::new();
    file.read_to_string(&mut json).chain_err(|| "unable to read settings file")?;
    json::decode::<Settings>(&json).chain_err(|| "unable to load settings")
}


fn run(dir: PathBuf, matches: clap::ArgMatches) -> Result<()> {
    let meta: Option<_> = migrant::search_for_meta(&dir, N_PARENTS);

    let force = matches.occurrences_of("force") > 0;
    let fake = matches.occurrences_of("fake") > 0;

    if matches.occurrences_of("init") > 0 || meta.is_none() {
        migrant::init(&dir)?;
        return Ok(())
    }

    let meta = meta.unwrap();           // absolute path of `.migrant` file
    let mut base_dir = meta.clone();    //
    base_dir.pop();                     // project base-directory

    let mut settings = load_settings(&meta)?;

    if matches.occurrences_of("list") > 0 {
        migrant::list(&base_dir, &settings)?;
    }
    else if let Some(tag) = matches.value_of("new") {
        migrant::new(&mut base_dir, &mut settings, tag)?;
        let new_settings = load_settings(&meta)?;
        migrant::list(&base_dir, &new_settings)?;
    }
    else if matches.occurrences_of("up") > 0 {
        migrant::up(&base_dir, &meta, &mut settings, force, fake)?;
        let new_settings = load_settings(&meta)?;
        migrant::list(&base_dir, &new_settings)?;
    }
    else if matches.occurrences_of("down") > 0 {
        migrant::down(&meta, &mut settings, force, fake)?;
        let new_settings = load_settings(&meta)?;
        migrant::list(&base_dir, &new_settings)?;
    }
    else if matches.occurrences_of("shell") > 0 {
        migrant::shell(&meta, settings)?;
    }
    Ok(())
}
