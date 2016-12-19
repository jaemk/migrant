extern crate migrant;
extern crate clap;

use std::env;

use clap::{Arg, App};

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

    let dir = env::current_dir().unwrap();
    let meta = migrant::search_for_meta(&dir, 3);
    let force = matches.occurrences_of("force") > 0;

    if matches.occurrences_of("init") > 0 || meta.is_none() {
        migrant::init(&dir);
        return;
    }

    if matches.occurrences_of("list") > 0 {
        migrant::list(&dir);
        return;
    }

    if let Some(tag) = matches.value_of("new") {
        migrant::new(&dir, tag);
        return;
    }

    if matches.occurrences_of("up") > 0 {
        migrant::up(&dir, force);
        return;
    }

    if matches.occurrences_of("down") > 0 {
        migrant::down(&dir, force);
        return;
    }

    if matches.occurrences_of("shell") > 0 {
        migrant::shell(&dir);
        return;
    }
}


