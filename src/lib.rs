// 严格覆盖率模式：跑 `RUSTFLAGS="--cfg=coverage_nightly" cargo +nightly llvm-cov nextest`
// 时启用，让带有 `#[cfg_attr(coverage_nightly, coverage(off))]` 的函数被 LLVM 跳过统计。
// 不影响默认 stable 构建。
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

use std::ffi::OsString;

use camino::Utf8PathBuf;
use clap::Parser;
use clap::Subcommand;
use tracing::debug;
use tracing_subscriber::fmt;
use tracing_subscriber::EnvFilter;

pub use entities::backend::local::LocalBackend;
pub use entities::backend::mtp::{MtpBackend, MtpClient, MtpMatch, MtpTarget};
pub use entities::backend::smb::{SmbBackend, SmbClient, SmbTarget};
pub use entities::backend::{Backend, Entry, EntryKind, MediaReader, MediaWriter, Metadata};
pub use entities::common::Error;
pub use entities::common::Result;
pub use entities::media_time;
pub use entities::uri::{Location, ParseError as LocationParseError};

mod entities;
mod usecases;

const FEATURE_CLI: &str = "cli";

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[arg(short, long, default_value = "info")]
    pub log_level: tracing::Level,

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
    /// Copy non-duplicate media files (images / videos recognized via magic-bytes MIME) from sources to the output directory. Pass --include-non-media to also copy everything else. Duplicate detection uses SHA-512. No source files are modified.
    Copy {
        /// Dry run, do not copy files
        #[arg(short, long)]
        dry_run: bool,

        /// Also copy files that magic-bytes MIME does not classify as image/video (e.g. documents, archives, unknown formats)
        #[arg(long)]
        include_non_media: bool,

        /// The source directories or files
        #[arg(required = true)]
        sources: Vec<Utf8PathBuf>,

        /// The output directory
        #[arg(short, long)]
        output: Utf8PathBuf,
    },

    /// Find duplicate files under the sources and print a shell script (batch syntax on Windows) that deletes the duplicates. Default uses a fast non-cryptographic hash (xxh3-64); pass --secure to use SHA-512 instead. If --output is given, deletions for files under that directory are commented out.
    Find {
        /// Use the cryptographic hash (SHA-512) instead of the default fast non-cryptographic hash (xxh3-64). Slower but eliminates the (already astronomically small) collision risk.
        #[arg(short, long)]
        secure: bool,

        /// The source directories or files
        #[arg(required = true)]
        sources: Vec<Utf8PathBuf>,

        /// The output directory; deletions for files under it are commented out
        #[arg(short, long)]
        output: Option<Utf8PathBuf>,
    },

    /// Move non-duplicate media files from sources into the output directory. Sources that duplicate something already in output are physically deleted; duplicate detection uses SHA-512. Pass --include-non-media to also move everything else.
    Move {
        /// Dry run, do not move or delete files
        #[arg(short, long)]
        dry_run: bool,

        /// Also move files that magic-bytes MIME does not classify as image/video
        #[arg(long)]
        include_non_media: bool,

        /// The source directories or files
        #[arg(required = true)]
        sources: Vec<Utf8PathBuf>,

        /// The output directory
        #[arg(short, long)]
        output: Utf8PathBuf,
    },
}

pub fn tidy(command: Commands) -> Result<()> {
    match command {
        Commands::Copy {
            dry_run,
            include_non_media,
            sources,
            output,
        } => usecases::copy(sources, output, dry_run, false, include_non_media),
        Commands::Find {
            secure,
            sources,
            output,
        } => usecases::find_duplicates(secure, sources, output),
        Commands::Move {
            dry_run,
            include_non_media,
            sources,
            output,
        } => usecases::copy(sources, output, dry_run, true, include_non_media),
    }
}

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
                clap::error::ErrorKind::DisplayHelp
                    | clap::error::ErrorKind::DisplayVersion
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
    install_logging(&cli);
    debug!(
        feature = FEATURE_CLI,
        operation = "parse_args",
        result = "ok",
        log_level = %cli.log_level,
        command = ?cli.command,
        "cli parsed"
    );
    tidy(cli.command)
}

fn install_logging(cli: &Cli) {
    let format = fmt::format()
        .with_ansi(false)
        .with_level(false)
        .with_line_number(cli.log_line_number)
        .with_target(cli.log_target)
        .with_thread_ids(cli.log_thread_ids)
        .compact();

    // 默认让 tidymedia 走 --log-level（默认 info），同时把 nom_exif 内部噪声
    // （parse_gps "find" info、"GPSInfo not found" warn 等）压到 error。
    // 用户可通过 RUST_LOG 覆盖（如 RUST_LOG=nom_exif=debug）。
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(format!("{}={},nom_exif=error", env!("CARGO_PKG_NAME"), cli.log_level))
    });

    let _ = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(std::io::stderr)
        .event_format(format)
        .try_init();
}
