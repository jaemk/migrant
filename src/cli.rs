use clap::{Arg, ArgAction, Command};

pub fn build_cli() -> Command {
    Command::new("migrant")
        .version(env!("CARGO_PKG_VERSION"))
        .author("James K. <james@kominick.com>")
        .about("Postgres/SQLite/MySQL migration manager")
        .subcommand(
            Command::new("self")
                .about("Self referential things")
                .subcommand(
                    Command::new("update")
                        .about("Update to the latest binary release, replacing this binary")
                        .arg(
                            Arg::new("no_confirm")
                                .help("Skip download/update confirmation")
                                .long("no-confirm")
                                .short('y')
                                .action(ArgAction::SetTrue),
                        )
                        .arg(
                            Arg::new("quiet")
                                .help("Suppress unnecessary download output (progress bar)")
                                .long("quiet")
                                .short('q')
                                .action(ArgAction::SetTrue),
                        ),
                )
                .subcommand(
                    Command::new("bash-completions")
                        .about("Generate bash completions & output to stdout or a file if specified")
                        .subcommand(
                            Command::new("install")
                                .about("Installs generated bash completions")
                                .arg(
                                    Arg::new("path")
                                        .help("Path to install bash completions at")
                                        .long("path")
                                        .default_value("/etc/bash_completion.d/migrant"),
                                ),
                        ),
                ),
        )
        .subcommand(
            Command::new("init")
                .about("Initialize project config")
                .arg(
                    Arg::new("type")
                        .long("type")
                        .short('t')
                        .help("Specify the database type (sqlite|postgres|mysql)"),
                )
                .arg(
                    Arg::new("location")
                        .long("location")
                        .short('l')
                        .help("Directory to initialize in"),
                )
                .arg(
                    Arg::new("default-from-env")
                        .long("default-from-env")
                        .action(ArgAction::SetTrue)
                        .help("Whether to default all settings file values to `env:VAR` form"),
                )
                .arg(
                    Arg::new("no-confirm")
                        .long("no-confirm")
                        .action(ArgAction::SetTrue)
                        .help("Disable interactive prompts"),
                ),
        )
        .subcommand(Command::new("setup").about("Setup migration table"))
        .subcommand(
            Command::new("connect-string")
                .about("Print out the connection string for postgres, or file-path for sqlite"),
        )
        .subcommand(
            Command::new("list").about("List status of applied and available migrations"),
        )
        .subcommand(
            Command::new("apply")
                .about("Moves up or down (applies up/down.sql) one migration. Default direction is up unless specified with `-d/--down`.")
                .arg(
                    Arg::new("down")
                        .long("down")
                        .short('d')
                        .action(ArgAction::SetTrue)
                        .help("Applies `down.sql` migrations"),
                )
                .arg(
                    Arg::new("all")
                        .long("all")
                        .short('a')
                        .action(ArgAction::SetTrue)
                        .help("Applies all remaining migrations in the chosen direction (un-applies all with --down)"),
                )
                .arg(
                    Arg::new("force")
                        .long("force")
                        .action(ArgAction::SetTrue)
                        .help("Applies migrations, ignoring errors"),
                )
                .arg(
                    Arg::new("fake")
                        .long("fake")
                        .action(ArgAction::SetTrue)
                        .help("Updates the migration table without running the migration"),
                ),
        )
        .subcommand(
            Command::new("redo")
                .about("Shortcut for running the latest `down` and `up` migration. Can be augmented with `all` and `force`")
                .arg(
                    Arg::new("all")
                        .long("all")
                        .short('a')
                        .action(ArgAction::SetTrue)
                        .help("Applies all remaining migrations in the chosen direction (un-applies all with --down)"),
                )
                .arg(
                    Arg::new("force")
                        .long("force")
                        .action(ArgAction::SetTrue)
                        .help("Applies migrations, ignoring errors"),
                ),
        )
        .subcommand(
            Command::new("new")
                .about("Create new migration up/down files")
                .arg(
                    Arg::new("tag")
                        .required(true)
                        .help("tag to use for new migration"),
                ),
        )
        .subcommand(Command::new("shell").about("Open a repl connection"))
        .subcommand(
            Command::new("edit")
                .about("Edit a migration file by tag name")
                .arg(Arg::new("tag").required(true).help("Tag name"))
                .arg(
                    Arg::new("down")
                        .long("down")
                        .short('d')
                        .action(ArgAction::SetTrue)
                        .help("Edit the down.sql file"),
                ),
        )
        .subcommand(
            Command::new("which-config")
                .about("Display the path to the configuration file being used"),
        )
        .subcommand(
            Command::new("tui")
                .about("Interactive terminal UI for viewing and applying migrations"),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_is_well_formed() {
        build_cli().debug_assert();
    }
}
