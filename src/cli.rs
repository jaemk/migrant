use clap::{Arg, App, SubCommand};
use super::{APP_VERSION, APP_NAME};


pub fn build_cli() -> App<'static, 'static> {
    App::new(APP_NAME)
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
                             .takes_value(false)))
                    .subcommand(SubCommand::with_name("bash-completions")
                        .about("Generate bash completions & output to stdout or a file if specified")
                        .subcommand(SubCommand::with_name("install")
                            .about("Installs generated bash completions")
                            .arg(Arg::with_name("path")
                                .help("Path to install bash completions at")
                                .long("path")
                                .default_value("/etc/bash_completion.d/migrant")
                                .takes_value(true)))))
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
        .subcommand(SubCommand::with_name("redo")
            .about("Shortcut for running the latest `down` and `up` migration. Can be augmented with `all` and `force`")
            .arg(Arg::with_name("all")
                .long("all")
                .short("a")
                .help("Applies all available migrations"))
            .arg(Arg::with_name("force")
                .long("force")
                .help("Applies the migration and treats it as if it were successful")))
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
}

