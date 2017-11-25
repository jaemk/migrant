use std::io::Read;
use std::path::{Path, PathBuf};
use std::env;
use std::fs;

use toml;
use url;
use chrono::{self, TimeZone};

use drivers;
use {
    Migratable, encode, prompt, open_file_in_fg, write_to_path,
    FULL_TAG_RE, DT_FORMAT, CONFIG_FILE,
    PG_CONFIG_TEMPLATE, SQLITE_CONFIG_TEMPLATE,
};
use errors::*;


#[derive(Debug, Clone)]
/// Project configuration/settings builder to initialize a new config file
pub struct ConfigInitializer {
    dir: PathBuf,
    database_type: Option<String>,
    interactive: bool,

}
impl ConfigInitializer {
    /// Start a new `ConfigInitializer`
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_owned(),
            database_type: None,
            interactive: true,
        }
    }

    /// Specify the database_type, checks whether the type is supported
    pub fn for_database(mut self, db_type: Option<&str>) -> Result<Self> {
        match db_type {
            None => self.database_type = None,
            Some(db_type) => {
                match db_type {
                    "postgres" | "sqlite" => (),
                    e => bail_fmt!(ErrorKind::Config, "unsupported database type: {}", e),
                };
                self.database_type = Some(db_type.to_owned());
            }
        }
        Ok(self)
    }

    /// Set interactive prompts, default is `true`
    pub fn interactive(mut self, b: bool) -> Self {
        self.interactive = b;
        self
    }

    /// Determines whether new .migrant file location should be in
    /// the given directory or a user specified path
    fn confirm_new_config_location(dir: &Path) -> Result<PathBuf> {
        println!(" A new `{}` config file will be created at the following location: ", CONFIG_FILE);
        println!("   {:?}", dir.display());
        let ans = prompt(" Is this ok? [Y/n] ")?;
        if ans.is_empty() || ans.to_lowercase() == "y" {
            return Ok(dir.to_owned());
        }

        println!(" You can specify the absolute location now, or nothing to exit");
        let ans = prompt(" >> ")?;
        if ans.is_empty() {
            bail_fmt!(ErrorKind::Config, "No `{}` path provided", CONFIG_FILE)
        }

        let path = PathBuf::from(ans);
        if !path.is_absolute() || path.file_name().unwrap() != CONFIG_FILE {
            bail_fmt!(ErrorKind::Config, "Invalid absolute path: {}, must end in `{}`", path.display(), CONFIG_FILE);
        }
        Ok(path)
    }

    /// Generate a template config file using provided parameters or prompting the user.
    /// If running interactively, the file will be opened for editing and `Config::setup`
    /// will be run automatically.
    pub fn initialize(self) -> Result<()> {
        let config_path = self.dir.join(CONFIG_FILE);
        let config_path = if !self.interactive {
            config_path
        } else {
            ConfigInitializer::confirm_new_config_location(&config_path)
                .map_err(|e| format_err!(ErrorKind::Config, "unable to create a `{}` config -> {}", CONFIG_FILE, e))?
        };

        let db_type = if let Some(db_type) = self.database_type.as_ref() {
            db_type.to_owned()
        } else {
            if !self.interactive {
                bail_fmt!(ErrorKind::Config, "database type must be specified if running non-interactively")
            }
            println!("\n ** Gathering database information...");
            let db_type = prompt(" database type (sqlite|postgres) >> ")?;
            match db_type.as_ref() {
                "postgres" | "sqlite" => (),
                e => bail_fmt!(ErrorKind::Config, "unsupported database type: {}", e),
            };
            db_type
        };

        println!("\n ** Writing {} config template to {:?}", db_type, config_path);
        match db_type.as_ref() {
            "postgres" => {
                write_to_path(&config_path, PG_CONFIG_TEMPLATE.as_bytes())?;
            }
            "sqlite" => {
                let content = SQLITE_CONFIG_TEMPLATE.replace("__CONFIG_DIR__", config_path.parent().unwrap().to_str().unwrap());
                write_to_path(&config_path, content.as_bytes())?;
            }
            _ => unreachable!(),
        };

        println!("\n ** Please update `{}` with your database credentials and run `setup`", CONFIG_FILE);

        if self.interactive {
            let editor = env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());
            let file_path = config_path.to_str().unwrap();
            let command = format!("{} {}", editor, file_path);
            println!(" -- Your config file will be opened with the following command: `{}`", &command);
            println!(" -- After editing, the `setup` command will be run for you");
            let _ = prompt(&format!(" -- Press [ENTER] to open now or [CTRL+C] to exit and edit manually"))?;
            open_file_in_fg(&editor, file_path)
                .map_err(|e| format_err!(ErrorKind::Config, "Error editing config file: {}", e))?;

            println!();
            let config = Config::load_file_only(&config_path)?;
            let _setup = config.setup()?;
        }
        Ok(())
    }
}


#[derive(Serialize, Deserialize, Debug, Clone)]
/// Settings that are serialized and saved in a project `.migrant.toml` config file
pub(crate) struct Settings {
    pub(crate) database_type: String,
    pub(crate) database_name: String,
    pub(crate) database_host: Option<String>,
    pub(crate) database_port: Option<String>,
    pub(crate) database_user: Option<String>,
    pub(crate) database_password: Option<String>,
    pub(crate) migration_location: String,
    pub(crate) database_params: Option<toml::value::Table>,
}


#[derive(Debug, Clone)]
/// Project configuration/settings
pub struct Config {
    pub path: PathBuf,
    pub(crate) settings: Settings,
    pub(crate) applied: Vec<String>,
    pub(crate) migrations: Option<Vec<Box<Migratable>>>,
}
impl Config {
    pub fn use_migrations(&mut self, migrations: Vec<Box<Migratable>>) -> &mut Self {
        self.migrations = Some(migrations);
        self
    }

    /// Do a full reload of the configuration file
    pub fn reload(&self) -> Result<Config> {
        Self::load(&self.path)
    }

    /// Load config file from the given path without querying the database
    /// to check for applied migrations
    pub fn load_file_only(path: &Path) -> Result<Config> {
        let mut file = fs::File::open(path)?;
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        let settings = toml::from_str::<Settings>(&content)?;
        Ok(Config {
            path: path.to_owned(),
            settings: settings,
            applied: vec![],
            migrations: None,
        })
    }

    /// Load config file from the given path and query the database to load up applied migrations
    pub fn load(path: &Path) -> Result<Config> {
        let mut config = Config::load_file_only(path)?;
        let applied = config.load_applied()?;
        config.applied = applied;
        Ok(config)
    }

    /// Load the applied migrations from the database migration table
    pub(crate) fn load_applied(&self) -> Result<Vec<String>> {
        if !self.migration_table_exists()? {
            bail_fmt!(ErrorKind::Migration, "`__migrant_migrations` table is missing, maybe try re-setting-up? -> `setup`")
        }

        let applied = match self.settings.database_type.as_ref() {
            "sqlite"    => drivers::sqlite::select_migrations(&self.settings.database_name)?,
            "postgres"  => drivers::pg::select_migrations(&self.connect_string()?)?,
            _ => unreachable!(),
        };
        let mut stamped = vec![];
        for tag in applied.into_iter() {
            if !FULL_TAG_RE.is_match(&tag) {
                bail_fmt!(ErrorKind::Migration, "Found a non-conforming tag in the database: `{}`", tag)
            }
            let stamp = chrono::Utc.datetime_from_str(
                tag.split('_').next().unwrap(),
                DT_FORMAT
            ).unwrap();
            stamped.push((stamp, tag));
        }
        stamped.sort_by(|a, b| a.0.cmp(&b.0));
        let applied = stamped.into_iter().map(|tup| tup.1).collect::<Vec<_>>();
        Ok(applied)
    }


    /// Check if a __migrant_migrations table exists
    pub(crate) fn migration_table_exists(&self) -> Result<bool> {
        match self.settings.database_type.as_ref() {
            "sqlite"    => drivers::sqlite::migration_table_exists(self.settings.database_name.as_str()),
            "postgres"  => drivers::pg::migration_table_exists(&self.connect_string()?),
            _ => unreachable!()
        }
    }

    /// Insert given tag into database migration table
    pub(crate) fn insert_migration_tag(&self, tag: &str) -> Result<()> {
        match self.settings.database_type.as_ref() {
            "sqlite"    => drivers::sqlite::insert_migration_tag(&self.settings.database_name, tag)?,
            "postgres"  => drivers::pg::insert_migration_tag(&self.connect_string()?, tag)?,
            _ => unreachable!(),
        };
        Ok(())
    }

    /// Remove a given tag from the database migration table
    pub(crate) fn delete_migration_tag(&self, tag: &str) -> Result<()> {
        match self.settings.database_type.as_ref() {
            "sqlite"    => drivers::sqlite::remove_migration_tag(&self.settings.database_name, tag)?,
            "postgres"  => drivers::pg::remove_migration_tag(&self.connect_string()?, tag)?,
            _ => unreachable!(),
        };
        Ok(())
    }

    /// Start a config initializer in the given directory
    pub fn init_in(dir: &Path) -> ConfigInitializer {
        ConfigInitializer::new(dir)
    }

    /// - Confirm the database can be accessed
    /// - Setup the database migrations table if it doesn't exist yet
    pub fn setup(&self) -> Result<bool> {
        println!(" ** Confirming database credentials...");
        match self.settings.database_type.as_ref() {
            "sqlite" => {
                if self.settings.database_name.is_empty() {
                    bail_fmt!(ErrorKind::Config, "`database_name` cannot be blank!")
                }
                let db_path = self.path.parent().unwrap().join(&self.settings.database_name);
                let created = drivers::sqlite::create_file_if_missing(&db_path)?;
                println!("    - checking if db file already exists...");
                if created {
                    println!("    - db not found... creating now... ✓")
                } else {
                    println!("    - db already exists ✓");
                }
            }
            "postgres" => {
                let conn_str = self.connect_string()?;
                let can_connect = drivers::pg::can_connect(&conn_str)?;
                if !can_connect {
                    println!(" ERROR: Unable to connect to {}", conn_str);
                    println!("        Please initialize your database and user and then run `setup`");
                    println!("\n  ex) sudo -u postgres createdb {}", &self.settings.database_name);
                    println!("      sudo -u postgres createuser {}", self.settings.database_user.as_ref().unwrap());
                    println!("      sudo -u postgres psql -c \"alter user {} with password '****'\"", self.settings.database_user.as_ref().unwrap());
                    println!();
                    bail_fmt!(ErrorKind::Config, "Cannot connect to postgres database");
                } else {
                    println!("    - Connection confirmed ✓");
                }
            }
            _ => unreachable!(),
        }

        println!("\n ** Setting up migrations table");
        let table_created = match self.settings.database_type.as_ref() {
            "sqlite" => {
                let db_path = self.path.parent().unwrap().join(&self.settings.database_name);
                drivers::sqlite::migration_setup(&db_path)?
            }
            "postgres" => {
                let conn_str = self.connect_string()?;
                drivers::pg::migration_setup(&conn_str)?
            }
            _ => unreachable!(),
        };

        if table_created {
            println!("    - migrations table missing");
            println!("    - `__migrant_migrations` table created ✓");
            Ok(true)
        } else {
            println!("    - `__migrant_migrations` table already exists ✓");
            Ok(false)
        }
    }

    /// Return the absolute path to the directory containing migration folders
    pub fn migration_dir(&self) -> Result<PathBuf> {
        Ok(self.path.parent()
            .map(|p| p.join(&self.settings.migration_location))
            .ok_or_else(|| format_err!(ErrorKind::PathError, "Error generating PathBuf to migration_location"))?)
    }

    /// Return the database type
    pub fn database_type(&self) -> Result<String> {
        Ok(self.settings.database_type.to_owned())
    }

    /// Return the absolute path to the database file. This is intended for
    /// sqlite3 databases only
    pub fn database_path(&self) -> Result<PathBuf> {
        match self.settings.database_type.as_ref() {
            "sqlite" => (),
            db_t => bail_fmt!(ErrorKind::Config, "Cannot retrieve database-path for database-type: {}", db_t),
        };

        Ok(self.path.parent()
            .map(|p| p.join(&self.settings.database_name))
            .ok_or_else(|| format_err!(ErrorKind::PathError, "Error generating PathBuf to database-path"))?)
    }

    /// Generate a database connection string.
    /// Not intended for file-based databases (sqlite)
    pub fn connect_string(&self) -> Result<String> {
        match self.settings.database_type.as_ref() {
            "postgres" => (),
            db_t => bail_fmt!(ErrorKind::Config, "Cannot generate connect-string for database-type: {}", db_t),
        };

        let pass = match self.settings.database_password {
            Some(ref pass) => {
                let p = encode(pass);
                format!(":{}", p)
            },
            None => "".into(),
        };
        let user = match self.settings.database_user.as_ref().and_then(|s| if s.is_empty() { None } else { Some(s) }) {
            Some(ref user) => encode(user),
            None => bail_fmt!(ErrorKind::Config, "'database_user' not specified"),
        };

        let db_name = if self.settings.database_name.is_empty() {
            bail_fmt!(ErrorKind::Config, "`database_name` not specified");
        } else {
            encode(&self.settings.database_name)
        };

        let host = self.settings.database_host.clone().unwrap_or_else(|| "localhost".to_string());
        let host = if host.is_empty() { "localhost".to_string() } else { host };
        let host = encode(&host);

        let port = self.settings.database_port.clone().unwrap_or_else(|| "5432".to_string());
        let port = if host.is_empty() { "5432".to_string() } else { port };
        let port = encode(&port);

        let s = format!("{db_type}://{user}{pass}@{host}:{port}/{db_name}",
                db_type=self.settings.database_type,
                user=user,
                pass=pass,
                host=host,
                port=port,
                db_name=db_name);

        let mut url = url::Url::parse(&s)?;

        if let Some(ref params) = self.settings.database_params {
            let mut pairs = vec![];
            for (k, v) in params.iter() {
                let k = encode(k);
                let v = match *v {
                    toml::value::Value::String(ref s) => encode(s),
                    ref v => bail_fmt!(ErrorKind::Config, "Database params can only be strings, found `{}={}`", k, v),
                };
                pairs.push((k, v));
            }
            if !pairs.is_empty() {
                let mut url = url.query_pairs_mut();
                for &(ref k, ref v) in &pairs {
                    url.append_pair(k, v);
                }
            }
        }

        Ok(url.into_string())
    }
}


