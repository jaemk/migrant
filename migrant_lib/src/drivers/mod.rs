use super::errors::*;
use connection;

mod sql {
    pub static CREATE_TABLE: &'static str = "create table __migrant_migrations(tag text unique);";
    pub static GET_MIGRATIONS: &'static str = "select tag from __migrant_migrations;";

    pub static SQLITE_MIGRATION_TABLE_EXISTS: &'static str = "select exists(select 1 from sqlite_master where type = 'table' and name = '__migrant_migrations');";
    pub static PG_MIGRATION_TABLE_EXISTS: &'static str = "select exists(select 1 from pg_tables where tablename = '__migrant_migrations');";

    // Some of these queries need to do unsafe search/replace of `__VAL__` -> tag
    // All tags are validated when created and again when loaded from the database migration table,
    // limiting chars to `[a-z0-9-]` and the full pattern to `[0-9]{14}_[a-z0-9-]+` so even if malicious
    // tags find their way into the database, tag validators should raise errors and point them out
    #[cfg(not(feature="sqlite"))]
    pub use self::q_sqlite::*;
    #[cfg(not(feature="sqlite"))]
    mod q_sqlite {
        pub static SQLITE_ADD_MIGRATION: &'static str = "insert into __migrant_migrations (tag) values ('__VAL__');";
        pub static SQLITE_DELETE_MIGRATION: &'static str = "delete from __migrant_migrations where tag = '__VAL__';";
    }

    #[cfg(not(feature="postgresql"))]
    pub use self::q_postgres::*;
    #[cfg(not(feature="postgresql"))]
    mod q_postgres {
        pub static PG_ADD_MIGRATION: &'static str = "prepare stmt as insert into __migrant_migrations (tag) values ($1); execute stmt('__VAL__'); deallocate stmt;";
        pub static PG_DELETE_MIGRATION: &'static str = "prepare stmt as delete from __migrant_migrations where tag = $1; execute stmt('__VAL__'); deallocate stmt;";
    }
}

pub mod pg;
pub mod sqlite;

