use std::ffi::OsString;

use camino::Utf8PathBuf;
use clap::Parser;
use clap::Subcommand;
use tracing::info;
use tracing_subscriber::fmt;

pub use use_cases::common::Error;
pub use use_cases::common::Result;

mod use_cases;

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
    /// Copy non-duplicate files from the source directory to the output directory. No source files will be modified.
    Copy {
        /// Dry run, do not copy files
        #[arg(short, long)]
        dry_run: bool,

        /// The source directories or files
        #[arg(required = true)]
        sources: Vec<Utf8PathBuf>,

        /// The output directory
        #[arg(short, long)]
        output: Utf8PathBuf,
    },

    /// Find all duplicate files in the source directory and print a shell script (using batch file syntax for Windows) to delete the duplicate files on standard output. If the output parameter is provided, then deletion operations for files located in the output directory will be commented out.
    Find {
        /// Use fast hash
        #[arg(short, long, default_value = "true", action = clap::ArgAction::SetTrue)]
        fast: bool,

        /// The source directories or files
        #[arg(required = true)]
        sources: Vec<Utf8PathBuf>,

        /// The output directory
        #[arg(short, long)]
        output: Option<Utf8PathBuf>,
    },

    /// Move non-duplicate files from the source directory to the output directory. Duplicate files already present in the output directory will be deleted.
    Move {
        /// Dry run, do not move files
        #[arg(short, long)]
        dry_run: bool,

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
            sources,
            output,
        } => use_cases::copy(sources, output, dry_run, false),
        Commands::Find {
            fast,
            sources,
            output,
        } => use_cases::find_duplicates(fast, sources, output),
        Commands::Move {
            dry_run,
            sources,
            output,
        } => use_cases::copy(sources, output, dry_run, true),
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
    info!("cli: {:?}", cli);
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

    let _ = tracing_subscriber::fmt()
        .with_max_level(cli.log_level)
        .with_writer(std::io::stderr)
        .event_format(format)
        .try_init();
}
