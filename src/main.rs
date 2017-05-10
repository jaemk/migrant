#[macro_use] extern crate clap;
extern crate migrant;

use std::env;
use std::path::{Path, PathBuf};
use clap::{Arg, App, SubCommand};
use migrant::Error;
use migrant::Config;
use migrant::Direction;


fn main() {
    let matches = App::new("Migrant")
        .version(crate_version!())
        .author("James K. <james.kominick@gmail.com>")
        .about("Postgres migration manager")
        .subcommand(SubCommand::with_name("init")
            .about("Initialize project"))
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

    if let Err(ref e) = run(dir, matches) {
        println!("error: {}", e);
        ::std::process::exit(1);
    }
}


fn run(dir: PathBuf, matches: clap::ArgMatches) -> Result<(), Error> {
    let config_path = migrant::search_for_config(&dir);

    if matches.is_present("init") || config_path.is_none() {
        let _ = migrant::Config::init(&dir)?;
        return Ok(())
    }

    let config_path = config_path.unwrap();    // absolute path of `.migrant` file
    let base_dir = config_path.parent()        // project base directory
        .map(Path::to_path_buf)
        .expect(&format!("failed to get parent path from: {:?}", config_path));

    let config = Config::load(&config_path)?;

    match matches.subcommand() {
        ("list", _) => {
            migrant::list(&config, &base_dir)?;
        }
        ("new", Some(matches)) => {
            let tag = matches.value_of("TAG").unwrap();
            migrant::new(&base_dir, &config, tag)?;
            migrant::list(&config, &base_dir)?;
        }
        ("apply", Some(matches)) => {
            let force = matches.is_present("force");
            let fake = matches.is_present("fake");
            let all = matches.is_present("all");
            let direction = if matches.is_present("down") { Direction::Down } else { Direction::Up };

            migrant::Migrator::with_config(&config, &config_path)
                .direction(direction)
                .force(force)
                .fake(fake)
                .all(all)
                .apply()?;

            let config = Config::load(&config_path)?;
            migrant::list(&config, &base_dir)?;
        }
        ("shell", _) => {
            migrant::shell(&base_dir, &config)?;
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
