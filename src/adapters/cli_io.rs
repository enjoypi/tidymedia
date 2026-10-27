use std::ffi::OsString;

use clap::Parser;
use tracing::debug;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt;

use super::commands::Commands;
use super::config_level;
use crate::adapters::dispatch::tidy;
use crate::entities::common::Error;
use crate::entities::common::Result;
use crate::usecases::config::checked_config;

const FEATURE_CLI: &str = "cli";

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[arg(
        short,
        long,
        help = "Log level (trace/debug/info/warn/error); defaults to log.level in config.yaml"
    )]
    pub log_level: Option<tracing::Level>,

    #[arg(long, default_value = "false")]
    pub log_line_number: bool,

    #[arg(long, default_value = "false")]
    pub log_target: bool,

    #[arg(long, default_value = "false")]
    pub log_thread_ids: bool,

    #[clap(subcommand)]
    pub command: Commands,
}

#[expect(
    clippy::missing_errors_doc,
    reason = "错误由统一 Result 与 CLI 集成测试约束"
)]
pub fn run_cli<I, T>(args: I) -> Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(e)
            if matches!(
                e.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            let _ = e.print();
            return Ok(());
        }
        Err(e) => {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                e.to_string(),
            )));
        }
    };
    let cfg = checked_config()?;
    let level = cli
        .log_level
        .unwrap_or_else(|| config_level(&cfg.log.level));
    let log_level = install_logging(&cli, level);
    debug!(
        feature = FEATURE_CLI,
        operation = "parse_args",
        result = "ok",
        log_level = %log_level,
        command = ?cli.command,
        "cli parsed"
    );
    tidy(cli.command)
}

fn install_logging(cli: &Cli, level: tracing::Level) -> tracing::Level {
    let format = fmt::format()
        .with_ansi(false)
        .with_level(false)
        .with_line_number(cli.log_line_number)
        .with_target(cli.log_target)
        .with_thread_ids(cli.log_thread_ids)
        .compact();
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(format!(
            "{}={},nom_exif=error",
            env!("CARGO_PKG_NAME"),
            level
        ))
    });
    let _ = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(std::io::stderr)
        .event_format(format)
        .try_init();
    level
}
