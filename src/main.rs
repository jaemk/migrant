#![recursion_limit = "1024" ]
#[macro_use] extern crate clap;
#[macro_use] extern crate error_chain;
extern crate migrant_lib;

#[cfg(feature="update")]
extern crate self_update;

use std::io;
use std::fs;
use std::env;
use std::path::PathBuf;
use migrant_lib::{Config, Direction, Migrator};

mod cli;
mod errors {
    use super::*;
    error_chain! {
        foreign_links {
            MigrantLib(migrant_lib::Error);
            SelfUpdate(self_update::errors::Error) #[cfg(feature="update")];
            Io(io::Error);
        }
    }
}
use errors::*;


static APP_VERSION: &'static str = crate_version!();
static APP_NAME: &'static str = "Migrant";


quick_main!(my_main);


fn my_main() -> Result<()> {
    let matches = cli::build_cli().get_matches();
    let dir = env::current_dir()?;

    run(&dir, &matches)
}


fn run(dir: &PathBuf, matches: &clap::ArgMatches) -> Result<()> {
    if let Some(self_matches) = matches.subcommand_matches("self") {
        if let Some(update_matches) = self_matches.subcommand_matches("update") {
            update(update_matches)?;
            return Ok(())
        }

        if let Some(compl_matches) = self_matches.subcommand_matches("bash-completions") {
            let mut out: Box<io::Write> = {
                if let Some(install_matches) = compl_matches.subcommand_matches("install") {
                    let install_path = install_matches.value_of("path").unwrap();
                    let prompt = format!("** Completion file will be installed at: `{}`\n** Is this Ok? [Y/n] ", install_path);
                    confirm(&prompt)?;
                    let file = fs::File::create(install_path)?;
                    Box::new(file)
                } else {
                    Box::new(io::stdout())
                }
            };
            cli::build_cli().gen_completions_to(APP_NAME.to_lowercase(), clap::Shell::Bash, &mut out);
            println!("** Success!");
            return Ok(())
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
        ("redo", Some(matches)) => {
            let force = matches.is_present("force");
            let fake = matches.is_present("fake");
            let all = matches.is_present("all");

            Migrator::with_config(&config)
                .direction(Direction::Down)
                .force(force)
                .fake(fake)
                .all(all)
                .apply()?;
            let config = config.reload()?;
            migrant_lib::list(&config)?;

            Migrator::with_config(&config)
                .direction(Direction::Up)
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
fn update(matches: &clap::ArgMatches) -> Result<()> {
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
fn update(_: &clap::ArgMatches) -> Result<()> {
    bail!("This executable was not compiled with `self_update` features enabled via `--features update`")
}


/// Get confirmation on a prompt
/// Returns `Ok` for 'yes' and `Err` for anything else
fn confirm(s: &str) -> Result<()> {
    use io::Write;
    print!("{}", s);
    io::stdout().flush()?;
    let mut s = String::new();
    io::stdin().read_line(&mut s)?;
    let s = s.trim().to_lowercase();
    if !s.is_empty() && s != "y" { bail!("Unable to confirm...") }
    Ok(())
}

