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

    /// fast or secure checksum
    #[arg(short, long, default_value = "true")]
    fast: bool,

    dirs: Vec<String>,
}

pub fn run() {
    let mut index = file_index::FileIndex::new();

    let cli = Cli::parse();
    for argument in cli.dirs {
        let path = std::path::Path::new(argument.as_str());
        index.visit_dir(path);
    }

    // let index = index;
    println!(
        "Files: {}, FastChecksums: {}",
        index.files.len(),
        index.fast_checksums.len()
    );

    let same = if cli.fast {
        index.fast_search_same()
    } else {
        index.search_same()
    };

    println!("Same: {}", same.len());

    for paths in same {
        println!("{:?}", paths);
    }
}
