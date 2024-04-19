use std::io;

use clap::Subcommand;

mod use_cases;

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Copy non-duplicate files from the source directory to the output directory. No source files will be modified.
    Copy {
        /// The source directories or files
        #[arg(required = true)]
        sources: Vec<String>,

        /// The output directory
        #[arg(short, long)]
        output: String,
    },

    /// Find all duplicate files in the source directory and print a shell script (using batch file syntax for Windows) to delete the duplicate files on standard output. If the output parameter is provided, then deletion operations for files located in the output directory will be commented out.
    Find {
        /// Use fast hash
        #[arg(short, long, default_value = "true", action = clap::ArgAction::SetTrue)]
        fast: bool,

        /// The source directories or files
        #[arg(required = true)]
        sources: Vec<String>,

        /// The output directory
        #[arg(short, long)]
        output: Option<String>,
    },

    /// Move non-duplicate files from the source directory to the output directory. Duplicate files already present in the output directory will be deleted.
    Move {
        /// The source directories or files
        #[arg(required = true)]
        sources: Vec<String>,

        /// The output directory
        #[arg(short, long)]
        output: String,
    },
}

pub fn tidy(command: Commands) -> io::Result<()> {
    match command {
        Commands::Copy { sources, output } => use_cases::copy(sources, output),
        Commands::Find {
            fast,
            sources,
            output,
        } => use_cases::find_duplicates(fast, sources, output),
        Commands::Move {
            sources: _sources,
            output,
        } => Ok(println!("{}", output)),
    }
}
