use clap::Parser;
use tracing::debug;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

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

    tracing_subscriber::registry()
        .with(fmt::layer().with_writer(std::io::stderr))
        .init();
    debug!("cli: {:?}", cli);
    tidymedia::tidy(cli.command)
}
