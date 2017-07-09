use std;
use toml;

#[cfg(feature="sqlite")]
use rusqlite;

#[cfg(feature="postgresql")]
use postgres;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Config(String),
    Migration(String),
    MigrationComplete(String),
    ShellCommand(String),
    PathError(String),
    IoOpen(std::io::Error),
    IoCreate(std::io::Error),
    IoRead(std::io::Error),
    IoWrite(std::io::Error),
    IoProc(std::io::Error),
    Utf8Error(std::string::FromUtf8Error),
    TomlDe(toml::de::Error),
    TomlSe(toml::ser::Error),

    #[cfg(feature="sqlite")]
    Sqlite(rusqlite::Error),

    #[cfg(feature="postgresql")]
    Postgres(postgres::error::Error),
    #[cfg(feature="postgresql")]
    PostgresConnect(postgres::error::ConnectError),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        use Error::*;
        match *self {
            Config(ref s)             => write!(f, "Config Error: {}", s),
            Migration(ref s)          => write!(f, "Migration Error: {}", s),
            MigrationComplete(ref s)  => write!(f, "MigrationComplete: {}", s),
            ShellCommand(ref s)       => write!(f, "ShellCommand Error: {}", s),
            PathError(ref s)          => write!(f, "PathError: {}", s),
            IoOpen(ref e)             => write!(f, "IoOpen Error: {}", e),
            IoCreate(ref e)           => write!(f, "IoCreate Error: {}", e),
            IoRead(ref e)             => write!(f, "IoRead Error: {}", e),
            IoWrite(ref e)            => write!(f, "IoWrite Error: {}", e),
            IoProc(ref e)             => write!(f, "IoProcess Error: {}", e),
            Utf8Error(ref e)          => write!(f, "Utf8 Error: {}", e),
            TomlDe(ref e)             => write!(f, "Toml Deserialization Error: {}", e),
            TomlSe(ref e)             => write!(f, "Toml Serialization Error: {}", e),

            #[cfg(feature="sqlite")]
            Sqlite(ref e)   => write!(f, "Sqlite Error: {}", e),

            #[cfg(feature="postgresql")]
            Postgres(ref e) => write!(f, "Postgres Error: {}", e),
            #[cfg(feature="postgresql")]
            PostgresConnect(ref e) => write!(f, "Postgres ConnectError: {}", e),
        }
    }
}

impl std::error::Error for Error {
    fn description(&self) -> &str {
        "Migrant Error"
    }

    fn cause(&self) -> Option<&std::error::Error> {
        use Error::*;
        Some(match *self {
            IoOpen(ref e)     => e,
            IoCreate(ref e)   => e,
            IoRead(ref e)     => e,
            IoWrite(ref e)    => e,
            IoProc(ref e)     => e,
            Utf8Error(ref e)  => e,
            TomlDe(ref e)     => e,
            TomlSe(ref e)     => e,

            #[cfg(feature="sqlite")]
            Sqlite(ref e)     => e,

            #[cfg(feature="postgresql")]
            Postgres(ref e)   => e,
            #[cfg(feature="postgresql")]
            PostgresConnect(ref e)   => e,

            _ => return None
        })
    }
}


#[cfg(feature="sqlite")]
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Error {
        Error::Sqlite(e)
    }
}


#[cfg(feature="postgresql")]
impl From<postgres::error::Error> for Error {
    fn from(e: postgres::error::Error) -> Error {
        Error::Postgres(e)
    }
}

#[cfg(feature="postgresql")]
impl From<postgres::error::ConnectError> for Error {
    fn from(e: postgres::error::ConnectError) -> Error {
        Error::PostgresConnect(e)
    }
}

macro_rules! format_err {
    ($e_type:expr, $literal:expr) => {
        $e_type(format!($literal))
    };
    ($e_type:expr, $literal:expr, $($arg:expr),*) => {
        $e_type(format!($literal, $($arg),*))
    };
}


macro_rules! bail {
    (Config <- $msg:expr) => {
        return Err(format_err!(Error::Config, $msg))
    };
    (Config <- $msg:expr, $($arg:expr),*) => {
        return Err(format_err!(Error::Config, $msg, $($arg),*))
    };
    (Migration <- $msg:expr) => {
        return Err(format_err!(Error::Migration, $msg))
    };
    (Migration <- $msg:expr, $($arg:expr),*) => {
        return Err(format_err!(Error::Migration, $msg, $($arg),*))
    };
    (MigrationComplete <- $msg:expr) => {
        return Err(format_err!(Error::MigrationComplete, $msg))
    };
    (MigrationComplete <- $msg:expr, $($arg:expr),*) => {
        return Err(format_err!(Error::MigrationComplete, $msg, $($arg),*))
    };
    (ShellCommand <- $msg:expr) => {
        return Err(format_err!(Error::ShellCommand, $msg))
    };
    (ShellCommand <- $msg:expr, $($arg:expr),*) => {
        return Err(format_err!(Error::ShellCommand, $msg, $($arg),*))
    };
}

