use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

mod commands;

#[derive(Parser)]
#[command(name = "atlas", version, about = "Developer knowledge engine")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Check system health
    Status,
    /// Ingest a Git repository
    Ingest {
        #[arg(default_value = ".")]
        path: String,
        #[arg(long, help = "Also fetch GitHub PRs and issues")]
        github: bool,
    },
    /// Which commits touched this file?
    Query {
        file: String,
    },
    /// Full history of a file: commits, PRs, and linked issues
    Explain {
        file: String,
    },
    /// Which other files co-change with this file?
    #[command(name = "co-changes")]
    CoChanges {
        file: String,
        /// Only show files that co-changed at least this many times
        #[arg(long, default_value_t = 1)]
        min_count: i64,
    },
    /// Chronological story for a file: issues → commits → PRs
    Timeline {
        file: String,
    },
    /// Most frequently modified files in this repository
    #[command(name = "hot-files")]
    HotFiles {
        /// Maximum number of files to show
        #[arg(long, default_value_t = 20)]
        limit: i64,
    },
    /// Which commit first introduced a file?
    #[command(name = "when-introduced")]
    WhenIntroduced {
        file: String,
    },
    /// Full context reconstruction for a file: identity, activity, coupling, coverage
    Context {
        file: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .without_time()
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Status                => commands::status::run(),
        Commands::Ingest { path, github } => commands::ingest::run(&path, github),
        Commands::Query { file }        => commands::query::run(&file),
        Commands::Explain { file }      => commands::explain::run(&file),
        Commands::CoChanges { file, min_count } => commands::cochanges::run(&file, min_count),
        Commands::Timeline { file }             => commands::timeline::run(&file),
        Commands::HotFiles { limit }            => commands::hotfiles::run(limit),
        Commands::WhenIntroduced { file }       => commands::whenintroduced::run(&file),
        Commands::Context { file }              => commands::context::run(&file),
    }
}
