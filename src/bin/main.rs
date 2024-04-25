use std::fmt::Debug;

use clap::Parser;
use tracing::info;
use tracing_subscriber::fmt;

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(short, long, default_value = "info")]
    log: String,

    #[clap(subcommand)]
    command: tidymedia::Commands,
}

fn main() -> std::io::Result<()> {
    let cli = Cli::parse();

    // Configure a custom event formatter
    let format = fmt::format()
        // .with_level(false) // don't include levels in formatted output
        .with_target(false) // don't include targets
        .compact(); // use the `Compact` formatting style.

    // Create a `fmt` subscriber that uses our custom event format, and set it
    // as the default.
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .event_format(format)
        .init();

    info!("cli: {:?}", cli);
    tidymedia::tidy(cli.command)
}
