/*!
Migrant can be used as a library so you can embed the management of migrations
into your binary and don't need to use a secondary tool in production environments.

The majority of `migrant/src/main.rs` could be copied, or just select functionality.
*/
extern crate migrant_lib;

use std::env;
use migrant_lib::Config;


fn run() -> Result<(), migrant_lib::Error> {
    let dir = env::current_dir().unwrap();
    let config = match migrant_lib::search_for_settings_file(&dir) {
        None => {
            Config::init_in(&dir)
                .initialize()?;
            return Ok(())
        }
        Some(p) => Config::from_settings_file(&p)?
    };
    config.reload()?;

    // This will fail if no migration files are present!
    // Run all available `up` migrations
    // migrant_lib::Migrator::with_config(&config)
    //     .direction(migrant_lib::Direction::Up)
    //     .all(true)
    //     .apply()?;

    migrant_lib::list(&config)?;
    Ok(())
}

pub fn main() {
    if let Err(e) = run() {
        println!("[ERROR] {}", e);
    }
}
