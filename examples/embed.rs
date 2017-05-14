/*!
Migrant can be used as a library so you can embed the management of migrations
into your binary and don't need to use a secondary tool in production environments.

The majority of `src/main.rs` could be copied, or just select functionality.
*/
extern crate migrant;

use std::env;
use migrant::Config;


pub fn main() {
    let dir = env::current_dir().unwrap();
    let config = match migrant::search_for_config(&dir) {
        None => {
            migrant::Config::init(&dir).expect("failed to initialize project");
            return;
        }
        Some(p) => Config::load(&p).expect("failed to load config"),
    };

    // This will fail if no migration files are present!
    // Run all available `up` migrations
    // migrant::Migrator::with_config(&config)
    //     .direction(migrant::Direction::Up)
    //     .all(true)
    //     .apply()
    //     .expect("failed to apply migrations")

    migrant::list(&config)
        .expect("failed to list migrations");
}
