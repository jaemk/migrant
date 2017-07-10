#[macro_use] extern crate clap;
extern crate migrant_lib;

use std::env;
use std::path::PathBuf;
use clap::{Arg, App, SubCommand};
use migrant_lib::{Error, Config, Direction, Migrator};


fn main() {
    let matches = App::new("Migrant")
        .version(crate_version!())
        .author("James K. <james.kominick@gmail.com>")
        .about("Postgres/SQLite migration manager")
        .subcommand(SubCommand::with_name("init")
            .about("Initialize project config")
            .arg(Arg::with_name("type")
                 .long("type")
                 .short("t")
                 .takes_value(true)
                 .help("Specify the database type (sqlite|postgres)"))
            .arg(Arg::with_name("location")
                 .long("location")
                 .short("l")
                 .takes_value(true)
                 .help("Directory to initialize in"))
            .arg(Arg::with_name("no-confirm")
                 .long("no-confirm")
                 .takes_value(false)
                 .help("Disable interactive prompts")))
        .subcommand(SubCommand::with_name("setup")
            .about("Setup migration table"))
        .subcommand(SubCommand::with_name("connect-string")
            .about("Print out the connection string for postgres, or file-path for sqlite"))
        .subcommand(SubCommand::with_name("list")
            .about("List status of applied and available migrations"))
        .subcommand(SubCommand::with_name("apply")
            .about("Moves up or down (applies up/down.sql) one migration. Default direction is up unless specified with `-d/--down`.")
            .arg(Arg::with_name("down")
                .long("down")
                .short("d")
                .help("Applies `down.sql` migrations"))
            .arg(Arg::with_name("all")
                .long("all")
                .short("a")
                .help("Applies all available migrations"))
            .arg(Arg::with_name("force")
                .long("force")
                .help("Applies the migration and treats it as if it were successful"))
            .arg(Arg::with_name("fake")
                .long("fake")
                .help("Updates the `.migrant.toml` file as if the specified migration was applied")))
        .subcommand(SubCommand::with_name("new")
            .about("Create new migration up/down files")
            .arg(Arg::with_name("TAG")
                 .required(true)
                 .help("tag to use for new migration")))
        .subcommand(SubCommand::with_name("shell")
            .about("Open a repl connection"))
        .subcommand(SubCommand::with_name("which-config")
            .about("Display the path to the configuration file being used"))
        .get_matches();

    let dir = env::current_dir().expect("Unable to retrieve current directory");

    if let Err(ref e) = run(&dir, &matches) {
        match *e {
            Error::MigrationComplete(ref s) => println!("{}", s),
            _ => {
                println!("[ERROR] {}", e);
                ::std::process::exit(1);
            }
        }
    }
}


fn run(dir: &PathBuf, matches: &clap::ArgMatches) -> Result<(), Error> {
    let config_path = migrant_lib::search_for_config(dir);

    if matches.is_present("init") || config_path.is_none() {
        let init_matches = matches.subcommand_matches("init").unwrap();
        let dir = init_matches.value_of("location").map(PathBuf::from).unwrap_or_else(|| dir.to_owned());
        let interactive = !init_matches.is_present("no-confirm");
        Config::init_in(&dir)
            .interactive(interactive)
            .for_database(init_matches.value_of("type"))?
            .initialize()?;
        return Ok(())
    }

    let config_path = config_path.unwrap();    // absolute path of `.migrant` file
    let config = Config::load_file_only(&config_path)?;

    if matches.is_present("setup") {
        config.setup()?;
        return Ok(())
    }

    if matches.is_present("connect-string") {
        if config.database_type()? == "sqlite" {
            println!("{}", config.database_path()?.to_str().unwrap());
        } else {
            println!("{}", config.connect_string()?);
        }
        return Ok(())
    }

    // load applied migrations from the database
    let config = config.reload()?;

    match matches.subcommand() {
        ("list", _) => {
            migrant_lib::list(&config)?;
        }
        ("new", Some(matches)) => {
            let tag = matches.value_of("TAG").unwrap();
            migrant_lib::new(&config, tag)?;
            migrant_lib::list(&config)?;
        }
        ("apply", Some(matches)) => {
            let force = matches.is_present("force");
            let fake = matches.is_present("fake");
            let all = matches.is_present("all");
            let direction = if matches.is_present("down") { Direction::Down } else { Direction::Up };

            Migrator::with_config(&config)
                .direction(direction)
                .force(force)
                .fake(fake)
                .all(all)
                .apply()?;

            let config = config.reload()?;
            migrant_lib::list(&config)?;
        }
        ("shell", _) => {
            migrant_lib::shell(&config)?;
        }
        ("which-config", _) => {
            println!("{}", config_path.to_str().unwrap());
        }
        _ => {
            println!("migrant: see `--help`");
        }
    };
    Ok(())
}
