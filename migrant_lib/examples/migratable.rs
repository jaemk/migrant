/*!
When using migrant as a library, migrations can be defined in the source code
instead of being discovered from the file system.

*/
extern crate migrant_lib;

use std::env;
use migrant_lib::{Config, FileMigration};


fn run() -> Result<(), Box<std::error::Error>> {
    let dir = env::current_dir().unwrap();
    let mut config = match migrant_lib::search_for_config(&dir) {
        None => {
            Config::init_in(&dir)
                .initialize()?;
            return Ok(())
        }
        Some(p) => Config::load(&p)?
    };

    config.use_migrations(vec![
        FileMigration::new("initial")
            .up("migrations/20171124032056_initial/up.sql")?
            .down("migrations/20171124032056_initial/down.sql")?
            .wrap(),
        FileMigration::new("second")
            .up("migrations/20171124032102_second/up.sql")?
            .down("migrations/20171124032102_second/down.sql")?
            .wrap(),
    ]);

    migrant_lib::list(&config)?;
    Ok(())
}

pub fn main() {
    if let Err(e) = run() {
        println!("[ERROR] {}", e);
    }
}
