mod file_checksum;
mod file_index;

extern crate core;

use clap::Parser;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Turn debugging information on
    #[arg(short, long, action = clap::ArgAction::Count)]
    debug: u8,

    dirs: Vec<String>,
}

pub fn run() {
    let mut index = file_index::FileIndex::new();

    let cli = Cli::parse();
    for argument in cli.dirs {
        if let Ok(file) = index.insert(argument.as_str()) {
            println!("{} is inserted", file.path);
        } else {
            println!("{} is not inserted", argument);
        }
    }
}
