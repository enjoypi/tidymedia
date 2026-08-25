use std::ffi::OsString;

use clap::Parser;
use tracing::debug;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt;

use crate::adapters::dispatch::tidy;
use crate::entities::common::Error;
use crate::entities::common::Result;
use crate::usecases::config::config;

pub(crate) const FEATURE_CLI: &str = "cli";

// Commands 枚举与各子命令字段拆到平级文件，保持本文件 ≤300 行；
// `Commands` 原样 re-export，公开 API（`Cli` / `Commands` / `run_cli`）不变。
#[path = "cli_commands.rs"]
mod commands;

pub use commands::Commands;

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Log level (trace/debug/info/warn/error); defaults to `log.level` in config.yaml
    #[arg(short, long)]
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

/// 解析命令行参数并执行对应子命令。
///
/// # Errors
///
/// 当参数解析失败（无效输入）或子命令执行过程中发生 IO 错误时返回 `Err`。
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
    let log_level = install_logging(&cli);
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

fn install_logging(cli: &Cli) -> tracing::Level {
    // CLI flag 优先；未传时取 config.yaml `log.level`（此路径在 subscriber
    // 安装前触发 config 首次加载，加载期日志不可见——sanitize 已保证非法
    // 值安全回退，仅损失加载日志可观测性，不损失行为）。
    let level = cli
        .log_level
        .unwrap_or_else(|| config_level(&config().log.level));

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

// sanitize 已兜底非法 level；此处独立纯函数再防一手，便于直测两分支。
fn config_level(raw: &str) -> tracing::Level {
    raw.parse().unwrap_or(tracing::Level::INFO)
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
