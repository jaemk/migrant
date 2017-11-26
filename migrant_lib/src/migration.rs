use std::path::{Path, PathBuf};
use chrono::{DateTime, Utc};

use {Migratable};
use errors::*;


#[derive(Clone, Debug)]
pub struct FileMigration {
    pub tag: String,
    pub up: Option<PathBuf>,
    pub down: Option<PathBuf>,
    pub stamp: Option<DateTime<Utc>>,
}
impl FileMigration {
    pub fn new(name: &str) -> Self {
        Self {
            tag: name.to_owned(),
            up: None,
            down: None,
            stamp: None,
        }
    }

    pub fn up<T: AsRef<Path>>(&mut self, up_file: T) -> Result<&mut Self> {
        let path = up_file.as_ref();
        if !path.exists() {
            bail_fmt!(ErrorKind::MigrationNotFound, "Migration file not found: {:?}", path)
        }
        self.up = Some(up_file.as_ref().to_owned());
        Ok(self)
    }

    pub fn down<T: AsRef<Path>>(&mut self, down_file: T) -> Result<&mut Self> {
        let path = down_file.as_ref();
        if !path.exists() {
            bail_fmt!(ErrorKind::MigrationNotFound, "Migration file not found: {:?}", path)
        }
        self.down = Some(down_file.as_ref().to_owned());
        Ok(self)
    }

    pub fn wrap(&self) -> Box<Migratable> {
        Box::new(self.clone())
    }
}

impl Migratable for FileMigration {
    fn apply_up(&self) -> Result<bool> {
        println!("Applying {:?}", self.up);
        Ok(true)
    }
    fn apply_down(&self) -> Result<bool> {
        println!("Applying down {:?}", self.down);
        Ok(true)
    }
    fn tag(&self) -> Result<String> {
        Ok(self.tag.to_owned())
    }
}


