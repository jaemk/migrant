use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use migrant_lib::config::{MySqlSettingsBuilder, PostgresSettingsBuilder, SqliteSettingsBuilder};
use migrant_lib::{Config, DbKind, Direction, Migrator};

mod cli;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[cfg(feature = "update")]
static APP_VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    dotenvy::dotenv().ok();
    let matches = cli::build_cli().get_matches();
    let result = env::current_dir()
        .map_err(Into::into)
        .and_then(|dir| run(&dir, &matches));
    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn run(dir: &Path, matches: &clap::ArgMatches) -> Result<()> {
    if let Some(self_matches) = matches.subcommand_matches("self") {
        return run_self(self_matches);
    }

    let config_path = migrant_lib::search_for_settings_file(dir);

    if matches.subcommand_matches("init").is_some() || config_path.is_none() {
        let initializer = match matches.subcommand_matches("init") {
            None => Config::init_in(dir),
            Some(init_matches) => {
                let from_env = init_matches.get_flag("default-from-env");
                let dir = init_matches
                    .get_one::<String>("location")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| dir.to_owned());
                let mut initializer = Config::init_in(&dir);
                initializer.interactive(!init_matches.get_flag("no-confirm"));
                initializer.with_env_defaults(from_env);
                if let Some(db_kind) = init_matches.get_one::<String>("type") {
                    match db_kind.parse::<DbKind>()? {
                        DbKind::Sqlite => {
                            initializer.with_sqlite_options(&SqliteSettingsBuilder::empty());
                        }
                        DbKind::Postgres => {
                            initializer.with_postgres_options(&PostgresSettingsBuilder::empty());
                        }
                        DbKind::MySql => {
                            initializer.with_mysql_options(&MySqlSettingsBuilder::empty());
                        }
                    }
                }
                initializer
            }
        };
        initializer.initialize()?;
        return Ok(());
    }

    // Absolute path of `Migrant.toml` file
    // This file must exist at this point, created by the block above
    let config_path = config_path.expect("Settings file must exist");
    let mut config = Config::from_settings_file(&config_path)?;
    config.use_cli_compatible_tags(true);

    match matches.subcommand() {
        Some(("setup", _)) => {
            config.setup()?;
        }
        Some(("connect-string", _)) => match config.database_type() {
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
        },
        Some(("list", _)) => {
            // load applied migrations from the database
            let config = config.reload()?;

            migrant_lib::list(&config)?;
        }
        Some(("new", matches)) => {
            // load applied migrations from the database
            let config = config.reload()?;

            let tag = matches.get_one::<String>("tag").expect("required arg");
            migrant_lib::new(&config, tag)?;
            migrant_lib::list(&config)?;
        }
        Some(("apply", matches)) => {
            // load applied migrations from the database
            let config = config.reload()?;

            let force = matches.get_flag("force");
            let fake = matches.get_flag("fake");
            let all = matches.get_flag("all");
            let direction = if matches.get_flag("down") {
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
        Some(("redo", matches)) => {
            // load applied migrations from the database
            let config = config.reload()?;

            let force = matches.get_flag("force");
            let all = matches.get_flag("all");

            Migrator::with_config(&config)
                .direction(Direction::Down)
                .force(force)
                .all(all)
                .apply()?;
            let config = config.reload()?;
            migrant_lib::list(&config)?;

            Migrator::with_config(&config)
                .direction(Direction::Up)
                .force(force)
                .all(all)
                .apply()?;
            let config = config.reload()?;
            migrant_lib::list(&config)?;
        }
        Some(("shell", _)) => {
            migrant_lib::shell(&config)?;
        }
        Some(("edit", matches)) => {
            let tag = matches.get_one::<String>("tag").expect("required arg");
            let up_down = if matches.get_flag("down") {
                Direction::Down
            } else {
                Direction::Up
            };
            migrant_lib::edit(&config, tag, &up_down)?;
        }
        Some(("which-config", _)) => {
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

fn run_self(self_matches: &clap::ArgMatches) -> Result<()> {
    if let Some(update_matches) = self_matches.subcommand_matches("update") {
        update(update_matches)?;
        return Ok(());
    }

    if let Some(compl_matches) = self_matches.subcommand_matches("bash-completions") {
        let mut out: Box<dyn io::Write> = {
            if let Some(install_matches) = compl_matches.subcommand_matches("install") {
                let install_path = install_matches
                    .get_one::<String>("path")
                    .expect("arg has a default");
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
        clap_complete::generate(
            clap_complete::Shell::Bash,
            &mut cli::build_cli(),
            "migrant",
            &mut out,
        );
        eprintln!("** Success!");
        return Ok(());
    }
    println!("migrant: see `--help`");
    Ok(())
}

/// CLI release tags are prefixed (`cli-v<version>`) to distinguish them from
/// library releases (`lib-v<version>`), so the latest CLI release is resolved
/// manually instead of relying on self_update's plain-semver tag handling.
#[cfg(feature = "update")]
fn update(matches: &clap::ArgMatches) -> Result<()> {
    static TAG_PREFIX: &str = "cli-v";

    let releases = self_update::backends::github::ReleaseList::configure()
        .repo_owner("jaemk")
        .repo_name("migrant")
        .build()?
        .fetch()?;
    // `Release::version` is the git tag with any leading `v` stripped, and
    // releases are ordered newest-first
    let latest = releases
        .iter()
        .find_map(|r| r.version.strip_prefix(TAG_PREFIX));
    let latest = match latest {
        Some(v) => v,
        None => {
            println!("No `{}*` releases available", TAG_PREFIX);
            return Ok(());
        }
    };

    if !self_update::version::bump_is_greater(APP_VERSION, latest)? {
        println!("Already up to date [v{}]!", APP_VERSION);
        return Ok(());
    }

    let mut builder = self_update::backends::github::Update::configure();
    builder
        .repo_owner("jaemk")
        .repo_name("migrant")
        .target(self_update::get_target())
        .bin_name("migrant")
        .target_version_tag(&format!("{}{}", TAG_PREFIX, latest))
        .show_download_progress(true)
        .no_confirm(matches.get_flag("no_confirm"))
        .current_version(APP_VERSION);

    if matches.get_flag("quiet") {
        builder.show_output(false).show_download_progress(false);
    }

    builder.build()?.update()?;
    println!("Updated to {}!", latest);
    Ok(())
}

#[cfg(not(feature = "update"))]
fn update(_: &clap::ArgMatches) -> Result<()> {
    Err("This executable was not compiled with `self_update` features enabled via `--features update`".into())
}

/// Get confirmation on a prompt
/// Returns `Ok` for 'yes' and `Err` for anything else
fn confirm(s: &str) -> Result<()> {
    print!("{}", s);
    io::stdout().flush()?;
    let mut s = String::new();
    io::stdin().read_line(&mut s)?;
    let s = s.trim().to_lowercase();
    if !s.is_empty() && s != "y" {
        return Err("Unable to confirm...".into());
    }
    Ok(())
}
