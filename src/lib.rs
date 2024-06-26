use camino::Utf8PathBuf;
use clap::Subcommand;

pub use use_cases::common::Result;

mod use_cases;

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
