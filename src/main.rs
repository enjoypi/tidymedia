use clap::Parser;
use tracing::{debug, error, info};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::interface_adapters::use_cases::entities::*;

mod interface_adapters;

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[arg(short, long, default_value = "info")]
    log: String,

    #[arg(short, long, default_value="true", action = clap::ArgAction::SetTrue)]
    fast: bool,

    dirs: Vec<String>,

    #[arg(short, long)]
    output: Option<String>,
}

fn main() {
    let cli = Cli::parse();

    tracing_subscriber::registry()
        .with(fmt::layer().with_writer(std::io::stderr))
        .init();

    let mut index = FileIndex::new();

    debug!("cli: {:?}", cli);

    if let Some(output) = cli.output.clone() {
        // check if output is directory
        // if not, create directory
        // the code is
        let output = std::path::Path::new(&output);
        if !output.is_dir() {
            error!("output is not a directory");
            return;
        }
    }

    for path in cli.dirs {
        index.visit_dir(path.as_str());
    }

    info!(
        "Files: {}, FastChecksums: {}, BytesRead: {}",
        index.files.len(),
        index.fast_checksums.len(),
        index.bytes_read(),
    );

    let same = if cli.fast {
        index.fast_search_same()
    } else {
        index.search_same()
    };
    info!("Same: {}", same.len());

    match cli.output {
        Some(output) => {
            let output = FileChecksum::get_full_path(std::path::Path::new(&output)).unwrap();
            let output = output.as_str();
            for (size, paths) in same.iter().rev() {
                println!(":SIZE {}\r", size);
                for path in paths.iter() {
                    if path.starts_with(output) {
                        println!(":DEL \"{}\"\r", path);
                    } else {
                        println!("DEL \"{}\"\r", path);
                    }
                }
                println!()
            }
        }
        _ => {
            for (size, paths) in same.iter().rev() {
                println!(":SIZE {}\r", size);
                for path in paths.iter() {
                    println!(":DEL \"{}\"\r", path);
                }
                println!()
            }
        }
    }

    info!("BytesRead: {}", index.bytes_read());
}
