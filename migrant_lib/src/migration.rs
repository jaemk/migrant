use std;
use std::path::{Path, PathBuf};
use chrono::{DateTime, Utc};

use drivers;
use migratable::Migratable;
use config::Config;
use connection::DbConn;
use {DbKind, invalid_tag, Direction, DT_FORMAT};
use errors::*;


#[derive(Clone, Debug)]
pub struct FileMigration {
    pub tag: String,
    pub up: Option<PathBuf>,
    pub down: Option<PathBuf>,
    pub stamp: Option<DateTime<Utc>>,
}
impl FileMigration {
    pub fn with_tag(tag: &str) -> Result<Self> {
        if invalid_tag(tag) {
            bail_fmt!(ErrorKind::Migration, "Invalid tag `{}`. Tags can contain [a-z0-9-]", tag);
        }
        Ok(Self {
            tag: tag.to_owned(),
            up: None,
            down: None,
            stamp: None,
        })
    }

    fn check_path(path: &Path) -> Result<()> {
        if !path.exists() {
            bail_fmt!(ErrorKind::MigrationNotFound, "Migration file not found: {:?}", path)
        }
        Ok(())
    }

    pub fn up<T: AsRef<Path>>(&mut self, up_file: T) -> Result<&mut Self> {
        let path = up_file.as_ref();
        Self::check_path(path)?;
        self.up = Some(path.to_owned());
        Ok(self)
    }

    pub fn down<T: AsRef<Path>>(&mut self, down_file: T) -> Result<&mut Self> {
        let path = down_file.as_ref();
        Self::check_path(path)?;
        self.down = Some(path.to_owned());
        Ok(self)
    }

    pub fn boxed(&self) -> Box<Migratable> {
        Box::new(self.clone())
    }
}

impl Migratable for FileMigration {
    fn apply_up(&self, db_kind: DbKind, config: &Config) -> std::result::Result<(), Box<std::error::Error>> {
        if let Some(ref up) = self.up {
            match db_kind {
                DbKind::Sqlite => {
                    let db_path = config.database_path()?;
                    drivers::sqlite::run_migration(&db_path, up)?;
                }
                DbKind::Postgres => {
                    let conn_str = config.connect_string()?;
                    drivers::pg::run_migration(&conn_str, up)?;
                }
            }
        } else {
            print_flush!("(empty) ...");
        }
        Ok(())
    }
    fn apply_down(&self, db_kind: DbKind, config: &Config) -> std::result::Result<(), Box<std::error::Error>> {
        if let Some(ref down) = self.down {
            match db_kind {
                DbKind::Sqlite => {
                    let db_path = config.database_path()?;
                    drivers::sqlite::run_migration(&db_path, down)?;
                }
                DbKind::Postgres => {
                    let conn_str = config.connect_string()?;
                    drivers::pg::run_migration(&conn_str, down)?;
                }
            }
        } else {
            print_flush!("(empty) ...");
        }
        Ok(())
    }
    fn tag(&self) -> String {
        match self.stamp.as_ref() {
            Some(dt) => {
                let dt_string = dt.format(DT_FORMAT).to_string();
                format!("{}_{}", dt_string, self.tag)
            }
            None => self.tag.to_owned(),
        }
    }
    fn description(&self, direction: &Direction) -> String {
        match *direction {
            Direction::Up   => self.up.as_ref().map(|p| format!("{:?}", p)).unwrap_or_else(|| self.tag()),
            Direction::Down => self.down.as_ref().map(|p| format!("{:?}", p)).unwrap_or_else(|| self.tag()),
        }
    }
}


#[derive(Clone, Debug)]
pub struct EmbeddedMigration {
    pub tag: String,
    pub up: Option<&'static str>,
    pub down: Option<&'static str>,
}
impl EmbeddedMigration {
    pub fn with_tag(tag: &str) -> Result<Self> {
        if invalid_tag(tag) {
            bail_fmt!(ErrorKind::Migration, "Invalid tag `{}`. Tags can contain [a-z0-9-]", tag);
        }
        Ok(Self {
            tag: tag.to_owned(),
            up: None,
            down: None,
        })
    }

    /// Statement to use for `up` migrations
    pub fn up(&mut self, stmt: &'static str) -> &mut Self {
        self.up = Some(stmt);
        self
    }

    /// Statement to use for `down` migrations
    pub fn down(&mut self, stmt: &'static str) -> &mut Self {
        self.down = Some(stmt);
        self
    }

    /// Box this migration up so it can be stored with other migrations
    pub fn boxed(&self) -> Box<Migratable> {
        Box::new(self.clone())
    }
}

impl Migratable for EmbeddedMigration {
    fn apply_up(&self, db_kind: DbKind, config: &Config) -> std::result::Result<(), Box<std::error::Error>> {
        if let Some(ref up) = self.up {
            #[cfg(any(feature="postgresql", feature="sqlite"))]
            match db_kind {
                DbKind::Sqlite => {
                    let db_path = config.database_path()?;
                    drivers::sqlite::run_migration_str(&db_path, up)?;
                }
                DbKind::Postgres => {
                    let conn_str = config.connect_string()?;
                    drivers::pg::run_migration_str(&conn_str, up)?;
                }
            }
            #[cfg(not(any(feature="postgresql", feature="sqlite")))]
            panic!("** Migrant ERROR: Database specific feature required to run embedded-file migration **");
        } else {
            print_flush!("(empty) ...");
        }
        Ok(())
    }
    fn apply_down(&self, db_kind: DbKind, config: &Config) -> std::result::Result<(), Box<std::error::Error>> {
        if let Some(ref down) = self.down {
            match db_kind {
                DbKind::Sqlite => {
                    let db_path = config.database_path()?;
                    drivers::sqlite::run_migration_str(&db_path, down)?;
                }
                DbKind::Postgres => {
                    let conn_str = config.connect_string()?;
                    drivers::pg::run_migration_str(&conn_str, down)?;
                }
            }
        } else {
            print_flush!("(empty) ...");
        }
        Ok(())
    }
    fn tag(&self) -> String {
        self.tag.to_owned()
    }
    fn description(&self, _: &Direction) -> String {
        self.tag()
    }
}


#[derive(Clone, Debug)]
pub struct FnMigration<T, U> {
    pub tag: String,
    pub up: Option<T>,
    pub down: Option<U>,
}

impl<T, U> FnMigration<T, U>
    where T: 'static + Clone + Fn(DbConn) -> std::result::Result<(), Box<std::error::Error>>,
          U: 'static + Clone + Fn(DbConn) -> std::result::Result<(), Box<std::error::Error>>
{
    pub fn with_tag(tag: &str) -> Result<Self> {
        if invalid_tag(tag) {
            bail_fmt!(ErrorKind::Migration, "Invalid tag `{}`. Tags can contain [a-z0-9-]", tag);
        }
        Ok(Self {
            tag: tag.to_owned(),
            up: None,
            down: None,
        })
    }

    pub fn up(&mut self, f_up: T) -> &mut Self {
        self.up = Some(f_up);
        self
    }

    pub fn down(&mut self, f_down: U) -> &mut Self {
        self.down = Some(f_down);
        self
    }

    pub fn boxed(&self) -> Box<Migratable> {
        Box::new(self.clone())
    }
}

impl<T, U> Migratable for FnMigration<T, U>
    where T: 'static + Clone + Fn(DbConn) -> std::result::Result<(), Box<std::error::Error>>,
          U: 'static + Clone + Fn(DbConn) -> std::result::Result<(), Box<std::error::Error>>
{
    fn apply_up(&self, _: DbKind, config: &Config) -> std::result::Result<(), Box<::std::error::Error>> {
        if let Some(ref up) = self.up {
            up(DbConn::new(config))?;
        } else {
            print_flush!("(empty) ...");
        }
        Ok(())
    }

    fn apply_down(&self, _: DbKind, config: &Config) -> std::result::Result<(), Box<::std::error::Error>> {
        if let Some(ref down) = self.down {
            down(DbConn::new(config))?;
        } else {
            print_flush!("(empty) ...");
        }
        Ok(())
    }

    fn tag(&self) -> String {
        self.tag.to_owned()
    }

    fn description(&self, _: &Direction) -> String {
        self.tag()
    }
}

