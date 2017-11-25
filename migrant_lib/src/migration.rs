use std::path::{Path, PathBuf};
use std::ffi::OsStr;
use chrono::{DateTime, Utc, TimeZone};

use {Migratable, DT_FORMAT};
use errors::*;


#[derive(Clone, Debug)]
pub struct FileMigration {
    up: PathBuf,
    down: Option<PathBuf>,
}
impl FileMigration {
    pub fn up<T: AsRef<Path>>(up_file: T) -> Result<Self> {
        let path = up_file.as_ref();
        if !path.exists() {
            bail_fmt!(ErrorKind::MigrationNotFound, "Migration file not found: {:?}", path)
        }
        Ok(Self {
            up: up_file.as_ref().to_owned(),
            down: None,
        })
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

    fn parts(&self) -> Result<(DateTime<Utc>, String)> {
        let mut full_name = self.up.file_name()
            .and_then(OsStr::to_str)
            .ok_or_else(|| format_err!(ErrorKind::PathError, "Error extracting file-name from: {:?}", self.up))?
            .split('_');
        let stamp = full_name.next()
            .ok_or_else(|| format_err!(ErrorKind::TagError, "Invalid tag format: {:?}", full_name))?;
        let stamp = Utc.datetime_from_str(stamp, DT_FORMAT)?;
        let tag = full_name.next()
            .ok_or_else(|| format_err!(ErrorKind::TagError, "Invalid tag format: {:?}", full_name))?
            .to_owned();
        Ok((stamp, tag))
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
        let (_, tag) = self.parts()?;
        Ok(tag)
    }
}


