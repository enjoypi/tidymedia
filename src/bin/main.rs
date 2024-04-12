use clap::Parser;
use tidymedia::interface_adapters;
use tracing::debug;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt};

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
    debug!("cli: {:?}", cli);
    interface_adapters::tidy(cli.fast, cli.dirs, cli.output)
}
