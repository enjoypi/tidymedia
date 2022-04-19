pub mod media;
pub mod crc32;
mod media_index;

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
            let _ = m.crc32();
            let _ = m.sha256();
            println!("{:?}", m);
        }


        // println!("{}", argument);
    }
}



