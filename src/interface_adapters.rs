use clap::Subcommand;

mod use_cases;

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// find duplicates
    Find {
        #[arg(short, long, default_value="true", action = clap::ArgAction::SetTrue)]
        fast: bool,

        sources: Vec<String>,

        #[arg(short, long)]
        output: Option<String>,
    },
    /// Move non-duplicate files from the source directory to the output directory.
    Move {
        sources: Vec<String>,
        #[arg(short, long)]
        output: String,
    },
}

pub fn tidy(command: Commands) {
    match command {
        Commands::Find {
            fast,
            sources,
            output,
        } => use_cases::find_duplicates(fast, sources, output),
        Commands::Move {
            sources: _sources,
            output,
        } => println!("{}", output),
    }
}
