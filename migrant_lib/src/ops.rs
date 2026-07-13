/*!
File-based migration management operations, compatible with the `migrant` CLI tool
*/
use std::collections::HashMap;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::{NaiveDateTime, Utc};
use log::warn;
use walkdir::WalkDir;

use crate::config::{Config, DbSettings};
use crate::errors::*;
use crate::macros::{bail, err};
use crate::migratable::Migratable;
use crate::migration::FileMigration;
use crate::migrator::Direction;
use crate::util::{open_file_in_fg, prompt};
use crate::{tags, DbKind, CONFIG_FILE, DT_FORMAT};

/// Search for a `Migrant.toml` settings file in the given directory
/// and all of its parent directories
pub fn search_for_settings_file<T: AsRef<Path>>(base: T) -> Option<PathBuf> {
    let mut dir = Some(base.as_ref());
    while let Some(d) = dir {
        let candidate = d.join(CONFIG_FILE);
        if candidate.is_file() {
            return Some(candidate);
        }
        dir = d.parent();
    }
    None
}

/// Search for available migrations in the given migration directory
///
/// Migration directories are expected to be named `<14-digit-timestamp>_<tag>`
/// and contain `up.sql` / `down.sql` files.
///
/// Intended only for use with `FileMigration`s not managed directly in source
/// with `Config::use_migrations`.
pub(crate) fn search_for_migrations(mig_root: &Path) -> Result<Vec<FileMigration>> {
    // collect any .sql files into a Map<parent-dir, Vec<up&down files>>
    let mut files: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    for entry in WalkDir::new(mig_root).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension() != Some(OsStr::new("sql")) {
            continue;
        }
        let Some(parent) = path.parent() else {
            continue;
        };
        files
            .entry(parent.to_path_buf())
            .or_default()
            .push(path.to_path_buf());
    }

    // transform up&down files into a Vec<FileMigration>
    let mut migrations = vec![];
    for (dir, migs) in &files {
        let full_name = dir
            .file_name()
            .and_then(OsStr::to_str)
            .ok_or_else(|| err!(PathError, "Error extracting file-name from: {:?}", dir))?;
        let (stamp, tag) = full_name.split_once('_').ok_or_else(|| {
            err!(
                TagError,
                "Invalid tag format: {:?}, must follow `<timestamp>_<tag>`",
                full_name
            )
        })?;
        let stamp = NaiveDateTime::parse_from_str(stamp, DT_FORMAT)
            .map(|dt| dt.and_utc())
            .map_err(|_| {
                err!(
                    TagError,
                    "Invalid timestamp format {:?}, on tag: {:?}, must follow `{}`",
                    stamp,
                    full_name,
                    DT_FORMAT
                )
            })?;

        let mut up = None;
        let mut down = None;
        for mig in migs {
            match mig.file_stem().and_then(OsStr::to_str) {
                Some("up") => up = Some(mig.clone()),
                Some("down") => down = Some(mig.clone()),
                _ => warn!("Ignoring unexpected sql file: {:?}", mig),
            }
        }
        if up.is_none() {
            bail!(MigrationNotFound, "Up migration not found for tag: {}", tag)
        }
        if down.is_none() {
            bail!(
                MigrationNotFound,
                "Down migration not found for tag: {}",
                tag
            )
        }
        migrations.push(FileMigration {
            up,
            down,
            tag: tag.to_owned(),
            stamp: Some(stamp),
            no_transaction: false,
        });
    }

    // sort by timestamps chronologically
    migrations.sort_by_key(|m| m.stamp);
    Ok(migrations)
}

/// The status of a single migration
#[derive(Debug, Clone)]
pub struct MigrationStatus {
    /// The full migration tag
    pub tag: String,
    /// Whether the migration is currently applied
    pub applied: bool,
}

/// Return the status of all migrations being managed: either those explicitly
/// defined on the config, or file-migrations under `migration_location`.
///
/// Make sure the `Config` has been `reload`ed so its set of applied
/// migrations is current.
pub fn migration_statuses(config: &Config) -> Result<Vec<MigrationStatus>> {
    let available = match config.migrations {
        Some(ref migs) => migs.iter().map(|m| m.tag()).collect::<Vec<_>>(),
        None => {
            let location = config.migration_location()?;
            search_for_migrations(&location)?
                .into_iter()
                .map(|m| m.tag())
                .collect()
        }
    };
    Ok(available
        .into_iter()
        .map(|tag| {
            let applied = config.applied.contains(&tag);
            MigrationStatus { tag, applied }
        })
        .collect())
}

/// List the currently applied and available migrations under `migration_location`
pub fn list(config: &Config) -> Result<()> {
    let statuses = migration_statuses(config)?;
    if statuses.is_empty() {
        if config.is_explicit() {
            println!("No migrations specified");
        } else {
            println!(
                "No migrations found under {:?}",
                config.migration_location()?
            );
        }
        return Ok(());
    }
    println!("Current Migration Status:");
    for mig in &statuses {
        println!(
            " -> [{x}] {name}",
            x = if mig.applied { '✓' } else { ' ' },
            name = mig.tag
        );
    }
    Ok(())
}

/// Create a new migration with the given tag
///
/// Generated tags will follow the format `{DT-STAMP}_{TAG}`
///
/// Intended only for use when running in "migrant CLI compatibility mode"
/// where migrations (`FileMigration`s) are all files with names following
/// the expected timestamp formatted name.
pub fn new(config: &Config, tag: &str) -> Result<()> {
    if !tags::is_valid_simple_tag(tag) {
        bail!(
            Migration,
            "Invalid tag `{}`. Tags can contain [a-z0-9-]",
            tag
        );
    }
    let now = Utc::now();
    let folder = format!("{stamp}_{tag}", stamp = now.format(DT_FORMAT), tag = tag);

    let mig_dir = config.migration_location()?.join(folder);
    fs::create_dir_all(&mig_dir)?;

    for name in ["up.sql", "down.sql"] {
        fs::File::create(mig_dir.join(name))?;
    }
    Ok(())
}

/// Open a repl connection to the given `Config` settings
///
/// Note, the respective database shell utility is expected to be available in `$PATH`.
///
/// | Database    |    Utility                  |
/// |-------------|-----------------------------|
/// | `postgres`  | `psql`                      |
/// | `sqlite`    | `sqlite3`                   |
/// | `mysql`     | `mysqlsh` (`mysql-shell`)   |
///
pub fn shell(config: &Config) -> Result<()> {
    let spec = build_shell_command(config)?;
    let mut command = Command::new(&spec.program);
    command.args(&spec.args);
    for (key, value) in &spec.envs {
        command.env(key, value);
    }
    command
        .spawn()
        .map_err(|e| {
            err!(
                ShellCommand,
                "Error running command `{}`. Is it available on your PATH? -> {}",
                spec.program,
                e
            )
        })?
        .wait()?;
    Ok(())
}

/// A fully-resolved database shell invocation: the program to run, its
/// arguments, and any extra environment variables it should be spawned with.
///
/// Secrets (the database password) are carried out-of-band in `envs` rather
/// than in `args`, so they never appear in the process' `argv`
/// (`/proc/<pid>/cmdline`), which is world-readable to other local users.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ShellCommandSpec {
    program: String,
    args: Vec<String>,
    envs: Vec<(String, String)>,
}

/// Build the shell invocation for the given `Config` without spawning anything.
///
/// This is factored out of `shell()` so the command construction (in
/// particular, that the password is kept out of `argv`) is unit-testable
/// without a live database or an installed shell utility.
fn build_shell_command(config: &Config) -> Result<ShellCommandSpec> {
    match config.database_type() {
        DbKind::Sqlite => {
            if config.settings.inner.is_memory_sqlite() {
                bail!(
                    ShellCommand,
                    "shell is not supported for in-memory sqlite databases"
                )
            }
            let db_path = config.database_path_string()?;
            Ok(ShellCommandSpec {
                program: "sqlite3".to_string(),
                args: vec![db_path],
                envs: vec![],
            })
        }
        DbKind::Postgres => {
            let (uri, password) = connect_uri_without_password(config)?;
            // psql reads the password from `PGPASSWORD` when present.
            Ok(ShellCommandSpec {
                program: "psql".to_string(),
                args: vec![uri],
                envs: vec![("PGPASSWORD".to_string(), password)],
            })
        }
        DbKind::MySql => {
            let (uri, password) = connect_uri_without_password(config)?;
            // MySQL Shell honors `MYSQL_PWD` for classic-protocol password auth.
            Ok(ShellCommandSpec {
                program: "mysqlsh".to_string(),
                args: vec!["--sql".to_string(), "--uri".to_string(), uri],
                envs: vec![("MYSQL_PWD".to_string(), password)],
            })
        }
    }
}

/// Return `(connect_uri, raw_password)` for a server database `Config`.
///
/// The returned URI has its password component stripped so it is safe to pass
/// as a command-line argument. The password is returned separately as its raw
/// (non-percent-encoded) value, taken straight from the settings so it never
/// round-trips through URL encoding.
fn connect_uri_without_password(config: &Config) -> Result<(String, String)> {
    let raw_password = match &config.settings.inner {
        DbSettings::Postgres(s) | DbSettings::MySql(s) => s.database_password.clone(),
        DbSettings::Sqlite(_) => String::new(),
    };
    let mut url = url::Url::parse(&config.connect_string()?)?;
    url.set_password(None).map_err(|_| {
        err!(
            ShellCommand,
            "Unable to strip password from database connection string"
        )
    })?;
    Ok((url.to_string(), raw_password))
}

/// Get user's selection of a set of migrations
fn select_from_matches<'a>(tag: &str, matches: &'a [FileMigration]) -> Result<&'a FileMigration> {
    let min = 1;
    let max = matches.len();
    loop {
        println!("* Migrations matching `{}`:", tag);
        for (row, mig) in matches.iter().enumerate() {
            println!("    {}) {}", row + 1, mig.tag());
        }
        print!("\n Please select a migration [1-{}] >> ", max);
        io::stdout().flush()?;
        let mut s = String::new();
        io::stdin().read_line(&mut s)?;
        match s.trim().parse::<usize>() {
            Err(e) => {
                println!("\nError: {}", e);
                continue;
            }
            Ok(n) if min <= n && n <= max => return Ok(&matches[n - 1]),
            Ok(_) => {
                println!("\nPlease select a number between 1-{}", max);
                continue;
            }
        }
    }
}

/// Open a migration file containing `tag` in its name
///
/// In the case of ambiguous names, the user will be prompted for a selection.
///
/// Intended only for use with `FileMigration`s that were created by
/// `migrant_lib::new` or `migrant` CLI (migration files with names that
/// follow the expected timestamp format), NOT those managed directly in source
/// with `Config::use_migrations`.
pub fn edit(config: &Config, tag: &str, up_down: &Direction) -> Result<()> {
    let mig_dir = config.migration_location()?;

    let available = search_for_migrations(&mig_dir)?;
    if available.is_empty() {
        println!("No migrations found under {:?}", mig_dir);
        return Ok(());
    }

    let matches = available
        .into_iter()
        .filter(|m| m.tag.contains(tag))
        .collect::<Vec<_>>();
    let mig = match matches.len() {
        0 => bail!(Config, "No migrations found with tag: {}", tag),
        1 => &matches[0],
        _ => {
            println!("* Multiple tags found!");
            select_from_matches(tag, matches.as_slice())?
        }
    };
    let file = match up_down {
        Direction::Up => &mig.up,
        Direction::Down => &mig.down,
    };
    let file = file
        .as_ref()
        .ok_or_else(|| err!(MigrationNotFound, "{} migration missing", up_down))?;
    let file_path = file
        .to_str()
        .ok_or_else(|| err!(PathError, "Invalid utf8 path: {:?}", file))?;

    let editor = env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());
    println!("* Running: `{} {}`", editor, file_path);
    let _ = prompt(" -- Press [ENTER] to open now or [CTRL+C] to exit and edit manually")?;
    open_file_in_fg(&editor, file_path)
        .map_err(|e| err!(Migration, "Error editing migrant file: {}", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_file_search_walks_up() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let nested = root.join("a/b/c");
        fs::create_dir_all(&nested).unwrap();
        assert!(search_for_settings_file(&nested).is_none());

        fs::write(root.join(CONFIG_FILE), "database_type = \"sqlite\"").unwrap();
        let found = search_for_settings_file(&nested).unwrap();
        assert_eq!(found, root.join(CONFIG_FILE));
    }

    #[test]
    fn migration_search_finds_and_sorts() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for (folder, up, down) in [
            ("20200101000000_second", "select 2;", "select -2;"),
            ("20190101000000_first", "select 1;", "select -1;"),
        ] {
            let d = root.join(folder);
            fs::create_dir_all(&d).unwrap();
            fs::write(d.join("up.sql"), up).unwrap();
            fs::write(d.join("down.sql"), down).unwrap();
        }
        let migs = search_for_migrations(root).unwrap();
        assert_eq!(2, migs.len());
        assert_eq!("20190101000000_first", migs[0].tag());
        assert_eq!("20200101000000_second", migs[1].tag());
    }

    #[test]
    fn migration_search_requires_up_and_down() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let d = root.join("20190101000000_first");
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("up.sql"), "select 1;").unwrap();
        assert!(search_for_migrations(root).is_err());
    }

    // A password with characters that must be percent-encoded in a URL: `@`
    // becomes `%40` and the space becomes `%20`. If the raw or encoded form
    // ever leaks into `argv`, these tests catch it.
    const RAW_PASSWORD: &str = "p@ss w";
    const ENCODED_PASSWORD: &str = "p%40ss%20w";

    fn postgres_config() -> Config {
        let settings = crate::config::Settings::configure_postgres()
            .database_name("mydb")
            .database_user("myuser")
            .database_password(RAW_PASSWORD)
            .database_host("localhost")
            .database_port(5432)
            .build()
            .unwrap();
        Config::with_settings(&settings)
    }

    fn mysql_config() -> Config {
        let settings = crate::config::Settings::configure_mysql()
            .database_name("mydb")
            .database_user("myuser")
            .database_password(RAW_PASSWORD)
            .database_host("localhost")
            .database_port(3306)
            .build()
            .unwrap();
        Config::with_settings(&settings)
    }

    fn assert_no_password_in_args(args: &[String]) {
        for arg in args {
            assert!(
                !arg.contains(RAW_PASSWORD),
                "raw password leaked into argv: {:?}",
                arg
            );
            assert!(
                !arg.contains(ENCODED_PASSWORD),
                "encoded password leaked into argv: {:?}",
                arg
            );
        }
    }

    #[test]
    fn postgres_shell_keeps_password_out_of_argv() {
        let spec = build_shell_command(&postgres_config()).unwrap();
        assert_eq!(spec.program, "psql");
        assert_no_password_in_args(&spec.args);
        // The connect uri is still present as an argument (sans password).
        assert!(spec.args.iter().any(|a| a.starts_with("postgres://")));
        // The raw password is delivered out-of-band via PGPASSWORD.
        assert!(
            spec.envs
                .iter()
                .any(|(k, v)| k == "PGPASSWORD" && v == RAW_PASSWORD),
            "PGPASSWORD with raw password not found in {:?}",
            spec.envs
        );
    }

    #[test]
    fn mysql_shell_keeps_password_out_of_argv() {
        let spec = build_shell_command(&mysql_config()).unwrap();
        assert_eq!(spec.program, "mysqlsh");
        assert_no_password_in_args(&spec.args);
        assert!(spec.args.iter().any(|a| a.starts_with("mysql://")));
        // The raw password is delivered out-of-band via MYSQL_PWD.
        assert!(
            spec.envs
                .iter()
                .any(|(k, v)| k == "MYSQL_PWD" && v == RAW_PASSWORD),
            "MYSQL_PWD with raw password not found in {:?}",
            spec.envs
        );
    }

    #[test]
    fn in_memory_sqlite_shell_errors() {
        let settings = crate::config::Settings::configure_sqlite()
            .memory()
            .build()
            .unwrap();
        let config = Config::with_settings(&settings);
        let res = build_shell_command(&config);
        assert!(
            res.is_err(),
            "expected an error for in-memory sqlite, got {:?}",
            res
        );
    }
}
