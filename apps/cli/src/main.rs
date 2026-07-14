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
        #[arg(long, help = "Also extract TypeScript structural edges (IMPORTS, CALLS_STATIC, REFERENCES_MODEL)")]
        typescript: bool,
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
        /// Emit the ContextDocument as JSON instead of formatted text
        #[arg(long)]
        json: bool,
    },
    /// Record a friction log entry for a file after using atlas context
    Feedback {
        file: String,
    },
    /// Compose anchor retrieval, structural observation, and history into an investigation
    Investigate {
        #[arg(required = true)]
        anchors: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    /// Show observed structural relationships for a file (IMPORTS, CALLS_STATIC, CALLS_INSTANCE, REFERENCES_MODEL)
    Structural {
        file: String,
        /// Also show which files import this file (reverse edges)
        #[arg(long)]
        reverse: bool,
    },
    /// Search corpus by anchor terms (file paths, commits, PRs, issues)
    Search {
        /// One or more anchor terms to search for
        #[arg(required = true)]
        anchors: Vec<String>,
        /// Emit the SearchDocument as JSON instead of formatted text
        #[arg(long)]
        json: bool,
    },
    /// Assemble review context for a pull request from mandatory file seeds
    #[command(name = "review-context")]
    ReviewContext {
        /// GitHub PR number
        pr_number: u64,
        /// Emit the ReviewContextDocument as JSON instead of formatted text
        #[arg(long)]
        json: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .without_time()
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Status                             => commands::status::run(),
        Commands::Ingest { path, github, typescript } => commands::ingest::run(&path, github, typescript),
        Commands::Query { file }        => commands::query::run(&file),
        Commands::Explain { file }      => commands::explain::run(&file),
        Commands::CoChanges { file, min_count } => commands::cochanges::run(&file, min_count),
        Commands::Timeline { file }             => commands::timeline::run(&file),
        Commands::HotFiles { limit }            => commands::hotfiles::run(limit),
        Commands::WhenIntroduced { file }       => commands::whenintroduced::run(&file),
        Commands::Context { file, json }        => commands::context::run(&file, json),
        Commands::Feedback { file }             => commands::feedback::run(&file),
        Commands::Investigate { anchors, json }     => commands::investigate::run(&anchors, json),
        Commands::Search { anchors, json }          => commands::search::run(&anchors, json),
        Commands::Structural { file, reverse }          => commands::structural::run(&file, reverse),
        Commands::ReviewContext { pr_number, json }      => commands::review_context::run(pr_number, json),
    }
}
