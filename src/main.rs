extern crate clap;
extern crate migrant;

use std::env;
use std::path::PathBuf;
use clap::{Arg, App};
use migrant::errors::*;
use migrant::Config;


fn main() {
    let matches = App::new("Migrant")
        .version("0.2.3")
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


fn run(dir: PathBuf, matches: clap::ArgMatches) -> Result<()> {
    let config_path = migrant::search_for_config(&dir);

    let force = matches.is_present("force");
    let fake = matches.is_present("fake");

    if matches.is_present("init") || config_path.is_none() {
        let _ = migrant::Config::init(&dir)?;
        return Ok(())
    }

    let config_path = config_path.unwrap();    // absolute path of `.migrant` file
    let mut base_dir = config_path.clone();    //
    base_dir.pop();                            // project base-directory

    let mut config = Config::load(&config_path)?;

    if matches.is_present("list") {
        migrant::list(&config, &base_dir)?;
    } else if let Some(tag) = matches.value_of("new") {
        migrant::new(&base_dir, &config, tag)?;
        migrant::list(&config, &base_dir)?;
    }
    //else if matches.is_present("up") {
    //    migrant::up(&base_dir, &config_path, & settings, force, fake)?;
    //    let new_settings = load_settings(&meta)?;
    //    migrant::list(&base_dir, &new_settings)?;
    //}
    //else if matches.occurrences_of("down") > 0 {
    //    migrant::down(&meta, &mut settings, force, fake)?;
    //    let new_settings = load_settings(&meta)?;
    //    migrant::list(&base_dir, &new_settings)?;
    //}
    //else if matches.occurrences_of("shell") > 0 {
    //    migrant::shell(&meta, settings)?;
    //}
    Ok(())
}
