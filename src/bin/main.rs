use clap::Parser;
use tracing::debug;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(short, long, default_value = "info")]
    log: String,

    #[clap(subcommand)]
    command: tidymedia::Commands,
}

fn main() {
    let cli = Cli::parse();

    tracing_subscriber::registry()
        .with(fmt::layer().with_writer(std::io::stderr))
        .init();
    debug!("cli: {:?}", cli);
    tidymedia::tidy(cli.command)
}
