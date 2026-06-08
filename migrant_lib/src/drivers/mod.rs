use super::errors::*;

mod sql {
    pub static CREATE_TABLE: &str = "create table __migrant_migrations(tag text unique);";
    pub static MYSQL_CREATE_TABLE: &str =
        "create table __migrant_migrations(tag varchar(512) unique);";

    pub static GET_MIGRATIONS: &str = "select tag from __migrant_migrations;";

    pub static SQLITE_MIGRATION_TABLE_EXISTS: &str = "select exists(select 1 from sqlite_master where type = 'table' and name = '__migrant_migrations');";
    pub static PG_MIGRATION_TABLE_EXISTS: &str =
        "select exists(select 1 from pg_tables where tablename = '__migrant_migrations');";
    pub static MYSQL_MIGRATION_TABLE_EXISTS: &str = "select exists(select 1 from information_schema.tables where table_name='__migrant_migrations') as tag;";
}

pub mod mysql;
pub mod pg;
pub mod sqlite;
