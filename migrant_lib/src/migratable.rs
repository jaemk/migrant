/*!
The `Migratable` trait
*/
use std::fmt;

use crate::migrator::Direction;
use crate::Config;

/// Helper trait so boxed `Migratable` trait objects can be cloned
pub trait MigratableClone {
    /// Clone into a new boxed trait object
    fn clone_migratable_box(&self) -> Box<dyn Migratable>;
}
impl<T> MigratableClone for T
where
    T: 'static + Migratable + Clone,
{
    fn clone_migratable_box(&self) -> Box<dyn Migratable> {
        Box::new(self.clone())
    }
}

/// A type that can be used to define database migrations
pub trait Migratable: MigratableClone {
    /// Define functionality that runs for `up` migrations
    fn apply_up(&self, _: &Config) -> std::result::Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    /// Define functionality that runs for `down` migrations
    fn apply_down(&self, _: &Config) -> std::result::Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    /// A unique identifying tag
    fn tag(&self) -> String;

    /// Optional migration description. Defaults to `Migratable::tag`
    fn description(&self, _: &Direction) -> String {
        self.tag()
    }
}

impl Clone for Box<dyn Migratable> {
    fn clone(&self) -> Box<dyn Migratable> {
        self.clone_migratable_box()
    }
}

impl fmt::Debug for Box<dyn Migratable> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Migration: {}", self.tag())
    }
}
