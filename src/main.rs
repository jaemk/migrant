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
        .get_matches();

    let dir = env::current_dir().unwrap();
    if matches.occurrences_of("init") > 0 {
        migrant::init(&dir);
        return;
    }

    if matches.occurrences_of("list") > 0 {
        migrant::list(&dir);
    }

    if let Some(new_tag) = matches.value_of("new") {
        println!("new tag! {}", new_tag);
    }
}


