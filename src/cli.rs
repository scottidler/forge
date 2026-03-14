use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::LazyLock;

static HELP_TEXT: LazyLock<String> =
    LazyLock::new(|| "Logs are written to: ~/.local/share/forge/logs/forge.log".to_string());

#[derive(Parser)]
#[command(
    name = "forge",
    about = "MWP Pipeline Runner -- portable briefcase pattern for content pipelines",
    version = env!("GIT_DESCRIBE"),
    after_help = HELP_TEXT.as_str()
)]
pub struct Cli {
    /// Path to forge.yml config
    #[arg(short, long, global = true, help = "Path to forge.yml config")]
    pub config: Option<PathBuf>,

    /// Enable verbose output
    #[arg(short, long, global = true, help = "Enable verbose output")]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Deploy pipeline scaffolding into current directory
    Unpack {
        /// Pipeline name
        pipeline: String,

        /// Initial input (file path, URL, or text)
        #[arg(short, long)]
        input: Option<String>,

        /// Output filename slug
        #[arg(short, long)]
        slug: Option<String>,
    },

    /// Retract .forge/ from current directory
    Pack {
        /// Abandon the pipeline run without writing final output
        #[arg(long)]
        abandon: bool,
    },

    /// Execute the next pending stage of the active pipeline
    Run {
        /// Specific stage name to run
        stage: Option<String>,

        /// Input file for the stage
        #[arg(short, long)]
        input: Option<String>,
    },

    /// List active pipeline runs
    Ls {
        /// Include packed/completed/abandoned runs
        #[arg(long)]
        all: bool,
    },

    /// Show details of current or specified pipeline run
    Show {
        /// Run ID to show
        run_id: Option<String>,
    },

    /// Show history of pipeline runs
    History {
        /// Filter by pipeline type
        pipeline: Option<String>,

        /// Limit number of results
        #[arg(short, long, default_value = "10")]
        limit: usize,
    },

    /// Print pipeline definition
    Describe {
        /// Pipeline name
        pipeline: String,

        /// Show only a specific stage
        #[arg(short, long)]
        stage: Option<usize>,
    },

    /// List reference material for a pipeline
    Refs {
        /// Pipeline name
        pipeline: String,

        /// Show refs for a specific stage
        #[arg(short, long)]
        stage: Option<usize>,
    },

    /// List all available pipeline definitions
    Pipelines,

    /// Initialize forge configuration in ~/.config/forge/
    Init {
        /// Overwrite existing files
        #[arg(long)]
        force: bool,
    },
}
