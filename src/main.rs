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

fn main() {
    let cli = Cli::parse();

    // let subscriber = fmt::Subscriber::builder()
    //     .with_env_filter(EnvFilter::try_new(cli.log).unwarp_or("info"))
    //     .with_writer(std::io::stderr)
    //     .finish();
    //
    // tracing::subscriber::set_global_default(subscriber)
    //     .expect("setting default subscriber failed");

    tracing_subscriber::registry()
        .with(fmt::layer().with_writer(std::io::stderr))
        .init();

    let mut index = file_index::FileIndex::new();

    debug!("cli: {:?}", cli);

    for path in cli.dirs {
        let path = std::path::Path::new(path.as_str());
        index.visit_dir(path);
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

    for paths in same {
        let mut paths: Vec<_> = paths.into_iter().collect();
        paths.sort();
        for path in paths.iter() {
            println!(":DEL \"{}\"\r", path);
        }
        println!()
    }

    info!("BytesRead: {}", index.bytes_read());
}
