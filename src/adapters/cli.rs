use std::ffi::OsString;

use clap::Parser;
use clap::Subcommand;
use tracing::debug;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt;

use crate::adapters::dispatch::tidy;
use crate::entities::common::Error;
use crate::entities::common::Result;
use crate::entities::uri::Location;
use crate::usecases::config::config;

pub(crate) const FEATURE_CLI: &str = "cli";

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

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Copy non-duplicate media files (images / videos recognized via magic-bytes MIME) from sources to the output directory. Pass --include-non-media to also copy everything else. Duplicate detection uses SHA-512. No source files are modified. Sources / output accept URI form: `smb://[user@]host[:port]/share/path`, `mtp://device/storage/path` or plain local path.
    Copy {
        /// Dry run, do not copy files
        #[arg(short, long)]
        dry_run: bool,

        /// Also copy files that magic-bytes MIME does not classify as image/video (e.g. documents, archives, unknown formats)
        #[arg(long)]
        include_non_media: bool,

        /// The source directories or files (URI or local path)
        #[arg(required = true)]
        sources: Vec<Location>,

        /// The output directory (URI or local path)
        #[arg(short, long)]
        output: Location,

        /// Archive directory template; placeholders: `{year}` `{month}` `{day}` `{make}` `{model}` `{valuable_name}`
        #[arg(long)]
        archive_template: Option<String>,

        /// Write a JSON operation report to this path
        #[arg(long)]
        report: Option<String>,
    },

    /// Find duplicate files under the sources and print a shell script (batch syntax on Windows) that deletes the duplicates. Default uses a fast non-cryptographic hash (xxh3-64); pass --secure to use SHA-512 instead. If --output is given, deletions for files under that directory are commented out.
    Find {
        /// Use the cryptographic hash (SHA-512) instead of the default fast non-cryptographic hash (xxh3-64). Slower but eliminates the (already astronomically small) collision risk.
        #[arg(short, long)]
        secure: bool,

        /// The source directories or files (URI or local path)
        #[arg(required = true)]
        sources: Vec<Location>,

        /// The output directory; deletions for files under it are commented out
        #[arg(short, long)]
        output: Option<Location>,

        /// Write a JSON operation report to this path
        #[arg(long)]
        report: Option<String>,
    },

    /// Move images whose content contains detectable text (OCR text detection) from sources into the output directory, preserving each file's path relative to its source root. Requires a configured `PaddleOCR` `DBNet` `det.onnx` model (`backend.ocr.det_model_path` / `TIDYMEDIA_OCR_DET_MODEL`). Non-image files are skipped.
    MoveTextShot {
        /// Dry run, do not move files
        #[arg(short, long)]
        dry_run: bool,

        /// The source directories or files (URI or local path)
        #[arg(required = true)]
        sources: Vec<Location>,

        /// The output directory (URI or local path)
        #[arg(short, long)]
        output: Location,

        /// Write a JSON operation report to this path
        #[arg(long)]
        report: Option<String>,
    },

    /// Cull similar/burst photos: keep the best one in source and move lower-quality copies to `output/<relative-path>/group-NNN/`, with a `BEST_<basename>` copy of the best photo placed alongside for side-by-side review. Uses perceptual hashing for grouping plus 4 ONNX models (`SCRFD`/`MobileFaceNet`/`FaceMesh`/`EyeState`) configured under `backend.face.*` for face quality scoring.
    Cull {
        /// Dry run, do not move files or create output directories
        #[arg(short, long)]
        dry_run: bool,

        /// The source directories or files (URI or local path)
        #[arg(required = true)]
        sources: Vec<Location>,

        /// The output directory (URI or local path)
        #[arg(short, long)]
        output: Location,

        /// Maximum pHash Hamming distance for grouping similar photos (overrides `backend.face.phash_hamming_max`)
        #[arg(long)]
        phash_max: Option<u8>,

        /// Write a JSON operation report to this path
        #[arg(long)]
        report: Option<String>,
    },

    /// Move non-duplicate media files from sources into the output directory. Sources that duplicate something already in output are physically deleted; duplicate detection uses SHA-512. Pass --include-non-media to also move everything else.
    Move {
        /// Dry run, do not move or delete files
        #[arg(short, long)]
        dry_run: bool,

        /// Also move files that magic-bytes MIME does not classify as image/video
        #[arg(long)]
        include_non_media: bool,

        /// The source directories or files (URI or local path)
        #[arg(required = true)]
        sources: Vec<Location>,

        /// The output directory (URI or local path)
        #[arg(short, long)]
        output: Location,

        /// Archive directory template; placeholders: `{year}` `{month}` `{day}` `{make}` `{model}` `{valuable_name}`
        #[arg(long)]
        archive_template: Option<String>,

        /// Write a JSON operation report to this path
        #[arg(long)]
        report: Option<String>,
    },
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
mod tests {
    use super::config_level;

    #[test]
    fn config_level_parses_valid_level() {
        assert_eq!(config_level("debug"), tracing::Level::DEBUG);
    }

    #[test]
    fn config_level_falls_back_to_info_on_invalid() {
        assert_eq!(config_level("chatty"), tracing::Level::INFO);
    }
}
