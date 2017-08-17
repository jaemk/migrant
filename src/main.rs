#[macro_use] extern crate clap;
extern crate migrant_lib;

#[cfg(feature="update")]
extern crate self_update;

use std::env;
use std::path::PathBuf;
use clap::{Arg, ArgMatches, App, SubCommand};
use migrant_lib::{Config, Direction, Migrator};


static APP_VERSION: &'static str = crate_version!();

fn main() {
    let matches = App::new("Migrant")
        .version(APP_VERSION)
        .author("James K. <james.kominick@gmail.com>")
        .about("Postgres/SQLite migration manager")
        .subcommand(SubCommand::with_name("self")
                    .about("Self referential things")
                    .subcommand(SubCommand::with_name("update")
                        .about("Update to the latest binary release, replacing this binary")
                        .arg(Arg::with_name("no_confirm")
                             .help("Skip download/update confirmation")
                             .long("no-confirm")
                             .short("y")
                             .required(false)
                             .takes_value(false))
                        .arg(Arg::with_name("quiet")
                             .help("Suppress unnecessary download output (progress bar)")
                             .long("quiet")
                             .short("q")
                             .required(false)
                             .takes_value(false))))
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
            .arg(Arg::with_name("tag")
                 .required(true)
                 .help("tag to use for new migration")))
        .subcommand(SubCommand::with_name("shell")
            .about("Open a repl connection"))
        .subcommand(SubCommand::with_name("edit")
            .about("Edit a migration file by tag name")
            .arg(Arg::with_name("tag")
                 .help("Tag name"))
            .arg(Arg::with_name("down")
                 .long("down")
                 .short("d")
                 .help("Edit the down.sql file")))
        .subcommand(SubCommand::with_name("which-config")
            .about("Display the path to the configuration file being used"))
        .get_matches();

    let dir = env::current_dir().expect("Unable to retrieve current directory");

    if let Err(ref e) = run(&dir, &matches) {
        match *e {
            migrant_lib::Error::MigrationComplete(ref s) => println!("{}", s),
            _ => {
                println!("[ERROR] {}", e);
                ::std::process::exit(1);
            }
        }
    }
}


fn run(dir: &PathBuf, matches: &clap::ArgMatches) -> Result<(), migrant_lib::Error> {
    if let Some(self_matches) = matches.subcommand_matches("self") {
        if let Some(update_matches) = self_matches.subcommand_matches("update") {
            return update(update_matches)
                .map_err(|e| migrant_lib::Error::Config(format!("{}", e)));
        }
        println!("migrant: see `--help`");
        return Ok(())
    }

    let config_path = migrant_lib::search_for_config(dir);

    if matches.is_present("init") || config_path.is_none() {
        let config = if let Some(init_matches) = matches.subcommand_matches("init") {
            let dir = init_matches.value_of("location").map(PathBuf::from).unwrap_or_else(|| dir.to_owned());
            let interactive = !init_matches.is_present("no-confirm");
            Config::init_in(&dir)
                .interactive(interactive)
                .for_database(init_matches.value_of("type"))?
        } else {
            Config::init_in(&dir)
        };
        config.initialize()?;
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
            let tag = matches.value_of("tag").unwrap();
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
        ("edit", Some(matches)) => {
            let tag = matches.value_of("tag").unwrap();
            let up_down = if matches.is_present("down") { Direction::Down } else { Direction::Up };
            migrant_lib::edit(&config, &tag, &up_down)?;
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

#[cfg(feature="update")]
fn update(matches: &ArgMatches) -> Result<(), Box<std::error::Error>> {
    let mut builder = self_update::backends::github::Update::configure()?;

    builder.repo_owner("jaemk")
        .repo_name("migrant")
        .target(&self_update::get_target()?)
        .bin_name("migrant")
        .show_download_progress(true)
        .no_confirm(matches.is_present("no_confirm"))
        .current_version(APP_VERSION);

    if matches.is_present("quiet") {
        builder.show_output(false)
            .show_download_progress(false);
    }

    let status = builder.build()?.update()?;
    match status {
        self_update::Status::UpToDate(v) => {
            println!("Already up to date [v{}]!", v);
        }
        self_update::Status::Updated(v) => {
            println!("Updated to {}!", v);
        }
    }
    return Ok(());
}


#[cfg(not(feature="update"))]
fn update(_: &ArgMatches) -> Result<(), Box<std::error::Error>> {
    Err(Box::new(migrant_lib::Error::Config("This executable was not compiled with `self_update` features enabled via `--features update`".to_string())))
}

