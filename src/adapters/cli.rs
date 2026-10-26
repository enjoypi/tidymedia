#[path = "cli_io.rs"]
mod cli_io;
#[path = "cli_commands.rs"]
mod commands;

pub use cli_io::{Cli, run_cli};
pub use commands::Commands;

fn config_level(raw: &str) -> tracing::Level {
    raw.parse().unwrap_or(tracing::Level::INFO)
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
