pub mod media;
pub mod crc32;

use std::env;
use crate::media::Media;


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
    for argument in config.dirs {
        // let file = argument.as_str();
        // let attr = fs::metadata(file).expect("what");
        // let dir = fs::read_dir(argument).expect_err("what");
        if let Ok(mut m) = Media::new(argument.as_str()) {
            let crc32 = m.crc32().unwrap();
            println!("{}\t{}", m.path(), crc32);
        }


        // println!("{}", argument);
    }
}



