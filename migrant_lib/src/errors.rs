/*!
Error types
*/

/// Convenience alias for `Result` with [`Error`]
pub type Result<T> = std::result::Result<T, Error>;

/// The error type returned by all fallible `migrant_lib` operations
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Invalid or incomplete configuration
    #[error("ConfigError: {0}")]
    Config(String),

    /// Failure while applying or un-applying a migration
    #[error("MigrationError: {0}")]
    Migration(String),

    /// A referenced migration could not be found
    #[error("MigrationNotFound: {0}")]
    MigrationNotFound(String),

    /// Failure while running an external command (editor, database shell)
    #[error("ShellCommandError: {0}")]
    ShellCommand(String),

    /// A path could not be interpreted or constructed
    #[error("PathError: {0}")]
    PathError(String),

    /// A migration tag does not conform to naming requirements
    #[error("TagError: {0}")]
    TagError(String),

    /// An unknown database type was specified
    #[error("InvalidDbKind: {0}")]
    InvalidDbKind(String),

    /// The operation requires a database feature that was not enabled at compile time
    #[error("FeatureRequired: this operation requires the `{0}` cargo feature")]
    FeatureRequired(&'static str),

    /// IO error
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// Settings file deserialization error
    #[error(transparent)]
    TomlDe(#[from] toml::de::Error),

    /// Connection string construction error
    #[error(transparent)]
    UrlParse(#[from] url::ParseError),

    /// Timestamp parsing error
    #[error(transparent)]
    ChronoParse(#[from] chrono::ParseError),

    /// Sqlite driver error
    #[cfg(feature = "sqlite")]
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),

    /// Postgres driver error
    #[cfg(feature = "postgres")]
    #[error(transparent)]
    Postgres(#[from] postgres::Error),

    /// MySQL driver error
    #[cfg(feature = "mysql")]
    #[error(transparent)]
    MySql(#[from] mysql::Error),
}

impl Error {
    /// `true` for [`Error::Config`]
    pub fn is_config(&self) -> bool {
        matches!(self, Error::Config(_))
    }

    /// `true` for [`Error::Migration`]
    pub fn is_migration(&self) -> bool {
        matches!(self, Error::Migration(_))
    }

    /// `true` for [`Error::MigrationNotFound`]
    pub fn is_migration_not_found(&self) -> bool {
        matches!(self, Error::MigrationNotFound(_))
    }

    /// `true` for [`Error::ShellCommand`]
    pub fn is_shell_command(&self) -> bool {
        matches!(self, Error::ShellCommand(_))
    }

    /// `true` for [`Error::TagError`]
    pub fn is_tag_error(&self) -> bool {
        matches!(self, Error::TagError(_))
    }

    /// `true` for [`Error::InvalidDbKind`]
    pub fn is_invalid_db_kind(&self) -> bool {
        matches!(self, Error::InvalidDbKind(_))
    }

    /// `true` for [`Error::FeatureRequired`]
    pub fn is_feature_required(&self) -> bool {
        matches!(self, Error::FeatureRequired(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predicates_match_their_variant() {
        assert!(Error::TagError("dup".to_string()).is_tag_error());
        assert!(Error::MigrationNotFound("x".to_string()).is_migration_not_found());
        assert!(Error::FeatureRequired("sqlite").is_feature_required());
    }

    #[test]
    fn predicates_reject_other_variants() {
        let err = Error::Migration("boom".to_string());
        assert!(err.is_migration());
        assert!(!err.is_config());
        assert!(!err.is_tag_error());
    }
}
