extern crate core;

pub mod file_checksum;
mod file_index;

use crate::file_checksum::FileChecksum;
use std::env;

pub struct Config {
    dirs: Vec<String>,
}

impl Config {
    pub fn new(args: env::Args) -> Result<Config, &'static str> {
        let dirs = args.collect();
        Ok(Config { dirs })
    }
}

pub fn run(config: Config) {
    let mut index = file_index::FileIndex::new();
    for argument in config.dirs {
        if let Ok(file) = index.insert(argument.as_str()) {
            println!("{} is inserted", file.path);
        } else {
            println!("{} is not inserted", argument);
        }
    }
}
