///! Error types
use std;
use toml;
use url;
use chrono;

#[cfg(feature="sqlite")]
use rusqlite;

#[cfg(feature="postgresql")]
use postgres;


error_chain! {
    foreign_links {
        Io(std::io::Error);
        StringUtf8Error(std::string::FromUtf8Error);
        StrUtf8Error(std::str::Utf8Error);
        TomlDe(toml::de::Error);
        TomlSe(toml::ser::Error);
        UrlParse(url::ParseError);
        ChronoParse(chrono::ParseError);
        Sqlite(rusqlite::Error) #[cfg(feature="sqlite")];
        Postgres(postgres::Error) #[cfg(feature="postgresql")];
    }
    errors {
        Config(s: String) {
            description("ConfigError")
            display("ConfigError: {}", s)
        }
        Migration(s: String) {
            description("MigrationError")
            display("MigrationError: {}", s)
        }
        MigrationComplete(s: String) {
            description("MigrationComplete")
            display("MigrationComplete: {}", s)
        }
        MigrationNotFound(s: String) {
            description("MigrationNotFound")
            display("MigrationNotFound: {}", s)
        }
        ShellCommand(s: String) {
            description("ShellCommand")
            display("ShellCommandError: {}", s)
        }
        PathError(s: String) {
            description("PathError")
            display("PathError: {}", s)
        }
        TagError(s: String) {
            description("TagError")
            display("TagError: {}", s)
        }
        InvalidDbKind(s: String) {
            description("InvalidDbKind")
            display("InvalidDbKind: {}", s)
        }
    }
}

impl Error {
    pub fn is_migration_complete(&self) -> bool {
        match *self.kind() {
            ErrorKind::MigrationComplete(_) => true,
            _ => false,
        }
    }
}

