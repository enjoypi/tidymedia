use std::ffi::OsString;

use clap::Parser;
use clap::Subcommand;
use tracing::debug;
use tracing_subscriber::fmt;
use tracing_subscriber::EnvFilter;

use crate::adapters::dispatch::tidy;
use crate::entities::common::Error;
use crate::entities::common::Result;
use crate::entities::uri::Location;

pub(crate) const FEATURE_CLI: &str = "cli";

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
    },
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

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(format!("{}={},nom_exif=error", env!("CARGO_PKG_NAME"), cli.log_level))
    });

    let _ = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(std::io::stderr)
        .event_format(format)
        .try_init();
}
