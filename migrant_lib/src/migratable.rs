use std::fmt;
use {DbKind, Config, Direction};


pub trait MigratableClone {
    fn clone_migratable_box(&self) -> Box<Migratable>;
}
impl<T> MigratableClone for T
    where T: 'static + Migratable + Clone
{
    fn clone_migratable_box(&self) -> Box<Migratable> {
        Box::new(self.clone())
    }
}


pub trait Migratable: MigratableClone {
    fn apply_up(&self, DbKind, &Config) -> Result<(), Box<::std::error::Error>> {
        print_flush!("(empty)");
        Ok(())
    }

    fn apply_down(&self, DbKind, &Config) -> Result<(), Box<::std::error::Error>> {
        print_flush!("(empty)");
        Ok(())
    }

    fn tag(&self) -> String;

    fn description(&self, &Direction) -> String {
        self.tag()
    }
}
impl Clone for Box<Migratable> {
    fn clone(&self) -> Box<Migratable> {
        self.clone_migratable_box()
    }
}

impl fmt::Debug for Box<Migratable> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Migration: {}", self.tag())
    }
}

