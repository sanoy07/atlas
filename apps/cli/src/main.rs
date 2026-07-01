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
    /// Full history of a file: commits + PRs
    Explain {
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
        Commands::Status => commands::status::run(),
        Commands::Ingest { path, github } => commands::ingest::run(&path, github),
        Commands::Query { file } => commands::query::run(&file),
        Commands::Explain { file } => commands::explain::run(&file),
    }
}
