#![feature(io_error_more)]

use clap::Parser;
use tracing::{debug, info};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt};

use tidymedia::file_index;

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[arg(short, long, default_value = "info")]
    log: String,

    #[arg(short, long, action = clap::ArgAction::SetTrue)]
    fast: bool,

    dirs: Vec<String>,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    tracing_subscriber::registry()
        .with(fmt::layer().with_writer(std::io::stderr))
        .init();

    let mut index = file_index::FileIndex::new();

    debug!("cli: {:?}", cli);

    for path in cli.dirs {
        let path = std::path::Path::new(path.as_str());
        index.visit_dir(path).await;
    }

    info!(
        "Files: {}, FastChecksums: {}, BytesRead: {}",
        index.files.len(),
        index.fast_checksums.len(),
        index.bytes_read(),
    );

    let same = if cli.fast {
        index.fast_search_same().await
    } else {
        index.search_same()
    };

    info!("Same: {}", same.len());

    let mut sorted: Vec<_> = vec![];
    for paths in same {
        let mut paths: Vec<_> = paths.into_iter().collect();
        paths.sort();

        sorted.push(paths);
    }

    sorted.sort();
    for paths in sorted.iter() {
        for path in paths.iter() {
            println!(":DEL \"{}\"\r", path);
        }
        println!()
    }

    info!("BytesRead: {}", index.bytes_read());
}
