use std::fmt::Debug;

use clap::Parser;
use tracing::info;
use tracing_subscriber::fmt;

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(short, long, default_value = "info")]
    log_level: tracing::Level,

    #[arg(long, default_value = "false")]
    log_line_number: bool,

    #[arg(long, default_value = "false")]
    log_target: bool,

    #[arg(long, default_value = "false")]
    log_thread_ids: bool,

    #[clap(subcommand)]
    command: tidymedia::Commands,
}

fn main() -> std::io::Result<()> {
    let cli = Cli::parse();

    // Configure a custom event formatter
    let format = fmt::format()
        .with_ansi(false)
        .with_level(false)
        .with_line_number(cli.log_line_number)
        .with_target(cli.log_target)
        .with_thread_ids(cli.log_thread_ids)
        .compact(); // use the `Compact` formatting style.

    // Create a `fmt` subscriber that uses our custom event format, and set it
    // as the default.
    tracing_subscriber::fmt()
        .with_max_level(cli.log_level)
        .with_writer(std::io::stderr)
        .event_format(format)
        .init();

    info!("cli: {:?}", cli);
    tidymedia::tidy(cli.command)
}
