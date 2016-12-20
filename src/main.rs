extern crate clap;
extern crate migrant;

use std::env;
use std::path::PathBuf;
use clap::{Arg, App};
use migrant::errors::*;


fn main() {
    let matches = App::new("Migrant")
        .version("0.1.0")
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
            .short("f")
            .long("force")
            .help("Applies the migration and treats it as if it were successful"))
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
    let meta: Option<_> = migrant::search_for_meta(&dir, 3);

    let force = matches.occurrences_of("force") > 0;

    if matches.occurrences_of("init") > 0 || meta.is_none() {
        migrant::init(&dir)?;
    }
    else if matches.occurrences_of("list") > 0 {
        migrant::list(&dir)?;
    }
    else if let Some(tag) = matches.value_of("new") {
        migrant::new(&dir, tag)?;
    }
    else if matches.occurrences_of("up") > 0 {
        migrant::up(&dir, force)?;
    }
    else if matches.occurrences_of("down") > 0 {
        migrant::down(&dir, force)?;
    }
    else if matches.occurrences_of("shell") > 0 {
        migrant::shell(&dir)?;
    }
    Ok(())
}
