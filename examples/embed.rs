/*!
Migrant can be used as a library so you can embed the management of migrations
into your binary and don't need to use a secondary tool in production environments.

The majority of `src/main.rs` could be copied, or just select functionality.
*/
extern crate migrant;

use std::env;
use std::path::PathBuf;
use migrant::Config;


pub fn main() {
    let dir = env::current_dir().unwrap();
    let config_path = match migrant::search_for_config(&dir) {
        None => {
            migrant::Config::init(&dir).expect("failed to initialize project");
            return;
        }
        Some(p) => p,
    };
    let base_dir = config_path.parent()
        .map(PathBuf::from)
        .expect("failed to get parent dir");
    let config = Config::load(&config_path)
        .expect("failed to load config");

    // This will fail if no migration files are present
    //migrant::apply_migration(&base_dir, &config_path, &config, migrant::Direction::Up, false, false, true)
    //    .expect("failed to apply migrations")

    migrant::list(&config, &base_dir)
        .expect("failed to list migrations");
}
