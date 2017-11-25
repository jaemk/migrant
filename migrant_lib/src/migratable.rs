use std::fmt;
use errors::*;


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
    fn apply_up(&self) -> Result<bool>;
    fn apply_down(&self) -> Result<bool>;
    fn tag(&self) -> Result<String>;
}
impl Clone for Box<Migratable> {
    fn clone(&self) -> Box<Migratable> {
        self.clone_migratable_box()
    }
}

impl fmt::Debug for Box<Migratable> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let tag = self.tag().unwrap_or_else(|e| format!("<Invalid Tag: {}>", e));
        write!(f, "Migration: {}", tag)
    }
}

