#![recursion_limit = "1024"]
#[macro_use]
extern crate clap;
#[macro_use]
extern crate error_chain;
extern crate migrant_lib;

#[cfg(feature = "update")]
extern crate self_update;

extern crate dotenv;

use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;

use dotenv::dotenv;

use migrant_lib::config::{MySqlSettingsBuilder, PostgresSettingsBuilder, SqliteSettingsBuilder};
use migrant_lib::{Config, DbKind, Direction, Migrator};

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
    dotenv().ok();
    let matches = cli::build_cli().get_matches();
    let dir = env::current_dir()?;

    run(&dir, &matches)
}

fn run(dir: &PathBuf, matches: &clap::ArgMatches) -> Result<()> {
    if let Some(self_matches) = matches.subcommand_matches("self") {
        if let Some(update_matches) = self_matches.subcommand_matches("update") {
            update(update_matches)?;
            return Ok(());
        }

        if let Some(compl_matches) = self_matches.subcommand_matches("bash-completions") {
            let mut out: Box<dyn io::Write> = {
                if let Some(install_matches) = compl_matches.subcommand_matches("install") {
                    let install_path = install_matches.value_of("path").unwrap();
                    let prompt = format!(
                        "** Completion file will be installed at: `{}`\n** Is this Ok? [Y/n] ",
                        install_path
                    );
                    confirm(&prompt)?;
                    let file = fs::File::create(install_path)?;
                    Box::new(file)
                } else {
                    Box::new(io::stdout())
                }
            };
            cli::build_cli().gen_completions_to(
                APP_NAME.to_lowercase(),
                clap::Shell::Bash,
                &mut out,
            );
            eprintln!("** Success!");
            return Ok(());
        }
        println!("migrant: see `--help`");
        return Ok(());
    }

    let config_path = migrant_lib::search_for_settings_file(dir);

    if matches.is_present("init") || config_path.is_none() {
        let config = match matches.subcommand_matches("init") {
            None => Config::init_in(&dir),
            Some(init_matches) => {
                let from_env = init_matches.is_present("default-from-env");
                let dir = init_matches
                    .value_of("location")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| dir.to_owned());
                let mut config = Config::init_in(&dir);
                let interactive = !init_matches.is_present("no-confirm");
                config.interactive(interactive);
                config.with_env_defaults(from_env);
                if let Some(db_kind) = init_matches.value_of("type") {
                    match db_kind.parse::<DbKind>()? {
                        DbKind::Sqlite => {
                            config.with_sqlite_options(&SqliteSettingsBuilder::empty());
                        }
                        DbKind::Postgres => {
                            config.with_postgres_options(&PostgresSettingsBuilder::empty());
                        }
                        DbKind::MySql => {
                            config.with_mysql_options(&MySqlSettingsBuilder::empty());
                        }
                    }
                }
                config
            }
        };
        config.initialize()?;
        return Ok(());
    }

    // Absolute path of `Migrant.toml` file
    // This file must exist at this point, created by the block above
    let config_path = config_path.expect("Settings file must exist");
    let mut config = Config::from_settings_file(&config_path)?;
    config.use_cli_compatible_tags(true);

    if matches.is_present("setup") {
        config.setup()?;
        return Ok(());
    }

    if matches.is_present("connect-string") {
        match config.database_type() {
            DbKind::Sqlite => {
                let path = config.database_path()?;
                let path = path
                    .to_str()
                    .ok_or_else(|| format!("PathError: Invalid utf8: {:?}", path))?;
                println!("{}", path);
            }
            DbKind::Postgres | DbKind::MySql => {
                println!("{}", config.connect_string()?);
            }
        }
        return Ok(());
    }

    match matches.subcommand() {
        ("list", _) => {
            // load applied migrations from the database
            let config = config.reload()?;

            migrant_lib::list(&config)?;
        }
        ("new", Some(matches)) => {
            // load applied migrations from the database
            let config = config.reload()?;

            let tag = matches.value_of("tag").unwrap();
            migrant_lib::new(&config, tag)?;
            migrant_lib::list(&config)?;
        }
        ("apply", Some(matches)) => {
            // load applied migrations from the database
            let config = config.reload()?;

            let force = matches.is_present("force");
            let fake = matches.is_present("fake");
            let all = matches.is_present("all");
            let direction = if matches.is_present("down") {
                Direction::Down
            } else {
                Direction::Up
            };

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
            // load applied migrations from the database
            let config = config.reload()?;

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
            let up_down = if matches.is_present("down") {
                Direction::Down
            } else {
                Direction::Up
            };
            migrant_lib::edit(&config, &tag, &up_down)?;
        }
        ("which-config", _) => {
            let path = config_path
                .to_str()
                .ok_or_else(|| format!("PathError: Invalid utf8: {:?}", config_path))?;
            println!("{}", path);
        }
        _ => {
            println!("migrant: see `--help`");
        }
    };
    Ok(())
}

#[cfg(feature = "update")]
fn update(matches: &clap::ArgMatches) -> Result<()> {
    let mut builder = self_update::backends::github::Update::configure()?;

    builder
        .repo_owner("jaemk")
        .repo_name("migrant")
        .target(&self_update::get_target()?)
        .bin_name("migrant")
        .show_download_progress(true)
        .no_confirm(matches.is_present("no_confirm"))
        .current_version(APP_VERSION);

    if matches.is_present("quiet") {
        builder.show_output(false).show_download_progress(false);
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

#[cfg(not(feature = "update"))]
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
    if !s.is_empty() && s != "y" {
        bail!("Unable to confirm...")
    }
    Ok(())
}
