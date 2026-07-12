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

    /// All migrations in the requested direction have already been applied
    #[error("MigrationComplete: {0}")]
    MigrationComplete(String),

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
    #[cfg(feature = "d-sqlite")]
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),

    /// Postgres driver error
    #[cfg(feature = "d-postgres")]
    #[error(transparent)]
    Postgres(#[from] postgres::Error),

    /// MySQL driver error
    #[cfg(feature = "d-mysql")]
    #[error(transparent)]
    MySql(#[from] mysql::Error),
}

impl Error {
    /// Return `true` if the error is `Error::MigrationComplete`
    pub fn is_migration_complete(&self) -> bool {
        matches!(self, Error::MigrationComplete(_))
    }
}
