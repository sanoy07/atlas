use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

mod ai;
mod commands;

#[derive(Parser)]
#[command(
    name = "atlas",
    version,
    about = "Local developer knowledge engine — evidence first, AI second",
    long_about = "Atlas ingests a repository into a local SQLite evidence graph \
(structure, history, issues/PRs). Commands query that graph deterministically. \
Optional local Ollama only reasons over sealed evidence packets — it never \
becomes the source of truth.\n\n\
Philosophy: AI proposes, Atlas verifies, you implement.",
    after_help = "\
The command list above is alphabetical. This is what you actually reach for:\n\
\n\
START HERE\n  \
atlas init                        set up this repository (creates DB, first ingest)\n  \
atlas status                      health, evidence freshness, what to run next\n\
\n\
UNDERSTAND — what is this repository?\n  \
atlas map                         modules, coupling, hot files, coverage\n  \
atlas modules                     inventory of module directories\n  \
atlas capabilities                infrastructure capabilities + product surfaces\n\
\n\
LOCATE — where is X?\n  \
atlas code-search ListingAsset    definition-ranked structural search\n  \
atlas callers tryEnqueue          who calls this symbol\n  \
atlas implementations IStorageProvider   what implements this interface\n\
\n\
INVESTIGATE — why is it like this? what breaks if I change it?\n  \
atlas investigate \"orders timeout under concurrency\"\n  \
atlas impact src/modules/orders/order.service.ts   blast radius\n  \
atlas focus src/modules/orders    neighborhood\n  \
atlas explain <file>              full history: commits, PRs, issues\n  \
atlas co-changes <file>           files that change together\n\
\n\
CONVENTIONS — what patterns does this repository follow?\n  \
atlas conventions src/modules     repeated structural patterns across peers\n  \
atlas structural <file> --reverse peer observations (what peers do that this file doesn't)\n  \
atlas anomalies                   deviations from observed patterns\n\
\n\
Evidence is a snapshot from the last ingest — `atlas status` reports when it has\n\
fallen behind HEAD. Re-run `atlas ingest .` after significant changes.\n\
\n\
Env: ATLAS_DB  ATLAS_OLLAMA_MODEL  ATLAS_OLLAMA_SYNTHESIS_MODEL  ATLAS_OLLAMA_NUM_CTX\n\
Docs: README.md · docs/atlas-philosophy.md"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Health check, Ollama models, and daily workflow tips
    Status,
    /// Alias for `status` (doctor-style health report)
    Doctor,
    /// Local Ollama tool loop over Atlas (read-only: investigate, ripgrep, web)
    ///
    /// Shells to agent/atlas_agent.py. Requires python3 + Ollama with a
    /// tool-capable model (default qwen3:4b). Repo facts come from Atlas;
    /// the model only selects tools and synthesizes.
    Agent {
        /// Natural-language question
        #[arg(required = true)]
        question: Vec<String>,
        /// Repository root (default: discover git root)
        #[arg(long)]
        repo: Option<String>,
        /// Max tool-loop steps (default 10)
        #[arg(long, default_value_t = 10)]
        max_steps: u32,
        /// Print model thinking traces
        #[arg(long)]
        show_thinking: bool,
        /// Skip agent loop; atlas investigate --no-ai only
        #[arg(long)]
        fast: bool,
        /// Disable web_search / web_fetch for this run
        #[arg(long)]
        no_web: bool,
    },
    /// Set up Atlas in this repository: create the database and run the first ingest
    ///
    /// Anchors the database at the Git root, adds it to .gitignore, and
    /// auto-detects which structural extractors apply. Safe to re-run.
    Init {
        #[arg(long, help = "Also fetch GitHub PRs and issues")]
        github: bool,
        /// Leave .gitignore untouched
        #[arg(long)]
        no_gitignore: bool,
    },
    /// Ingest a Git repository into the local evidence database
    Ingest {
        #[arg(default_value = ".")]
        path: String,
        #[arg(long, help = "Also fetch GitHub PRs and issues")]
        github: bool,
        #[arg(long, help = "Also extract TypeScript structural edges (IMPORTS, CALLS_STATIC, REFERENCES_MODEL)")]
        typescript: bool,
        /// Ingest commits reachable from any local ref (branches + tags).
        /// Default is HEAD-only.  The chosen scope is recorded in the DB
        /// so downstream queries know what portion of history is present.
        #[arg(long)]
        all_refs: bool,
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
        /// Override the repository path (defaults to git root of current directory)
        #[arg(long)]
        repo: Option<String>,
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
    /// Investigate from anchors, a question, --issue, or --file
    ///
    /// Deterministic retrieval builds an evidence packet. Optional local AI
    /// proposes hypotheses; Atlas verifies them (C4). Use --no-ai for facts only.
    Investigate {
        /// Anchor terms, or a single quoted natural-language question.
        /// Optional when --issue or --file is set.
        #[arg(required = false)]
        anchors: Vec<String>,
        #[arg(long)]
        json: bool,
        /// Skip AI; show deterministic evidence only (legacy raw layout for anchor mode)
        #[arg(long)]
        raw: bool,
        /// Override the repository path (defaults to git root of current directory)
        #[arg(long)]
        repo: Option<String>,
        /// Investigate from a stored GitHub issue number (requires prior --github ingest)
        #[arg(long)]
        issue: Option<i64>,
        /// Seed investigation around this repository-relative file path
        #[arg(long)]
        file: Option<String>,
        /// Never call local AI (deterministic evidence packet / legacy raw only)
        #[arg(long)]
        no_ai: bool,
        /// Max local-AI investigation rounds (1–3, default 3)
        #[arg(long, default_value_t = 3)]
        rounds: u32,
    },
    /// Show observed structural relationships for a file (IMPORTS, CALLS_STATIC, CALLS_INSTANCE, REFERENCES_MODEL)
    Structural {
        file: String,
        /// Also show which files import this file (reverse edges)
        #[arg(long)]
        reverse: bool,
    },
    /// Who calls this symbol (or file)? OBSERVED reverse edges from structural_edges.
    ///
    /// Prefer this over grepping for multi-hop flow questions. Production callers
    /// are listed before tests. Dynamic DI is not observed.
    Callers {
        /// Symbol (tryEnqueue), Class.method, or file path
        subject: String,
        /// Emphasize callees of the subject (outgoing calls)
        #[arg(long)]
        callees: bool,
        #[arg(long, default_value_t = 80)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// Heuristic implementors of an interface / type (import + naming DERIVED).
    Implementations {
        /// Interface name (IStorageProvider) or path (storage.interface.ts)
        subject: String,
        #[arg(long, default_value_t = 40)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// Infrastructure capabilities and product surfaces (import fan-in DERIVED).
    ///
    /// Answers "who uses storage / messaging / cache?" without domain hardcoding.
    Capabilities {
        #[arg(long)]
        json: bool,
    },
    /// Definition-ranked structural code search (not full-text; not LSP).
    ///
    /// Ranks DEFINITION / WIRING / CALL_SITE / REFERENCE / TEST from structural_edges.
    #[command(name = "code-search")]
    CodeSearch {
        /// Symbol or path fragment
        query: String,
        #[arg(long, default_value_t = 40)]
        limit: usize,
        #[arg(long)]
        json: bool,
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
    /// Campaign engine: determine the next evidence-backed improvement
    Campaign {
        #[command(subcommand)]
        subcommand: CampaignCommand,
    },
    /// Run investigation quality benchmarks against a corpus of TOML cases
    Eval {
        /// Path to a directory containing *.toml benchmark files
        #[arg(default_value = "eval/investigations")]
        corpus_dir: String,
        /// Print each case's anchors and intermediate progress
        #[arg(long)]
        verbose: bool,
    },
    /// Generate an implementation plan for a GitHub issue using Atlas codebase evidence
    Plan {
        /// GitHub issue number
        issue_number: i64,
        /// Override the repository path (defaults to git root of current directory)
        #[arg(long)]
        repo: Option<String>,
    },
    /// Browse and compare stored investigation results
    Investigations {
        #[command(subcommand)]
        subcommand: InvestigationsCommand,
    },
    /// Manage projects (groups of repositories that form one product)
    Project {
        #[command(subcommand)]
        subcommand: ProjectCommand,
    },
    /// Show the on-disk repository tree (spatial view only — no evidence attached)
    Tree {
        /// Maximum depth below the repository root.  Omit for unlimited.
        /// 0 shows only the root; 1 shows the root plus its immediate children.
        #[arg(long)]
        depth: Option<u32>,
        /// Emit the RepositoryTree as JSON instead of formatted text
        #[arg(long)]
        json: bool,
    },
    /// Attach existing Atlas evidence to a file or directory path
    Inspect {
        /// Repository-relative path (file or directory).  Defaults to the repo root.
        #[arg(default_value = ".")]
        path: String,
        /// Emit the InspectionDocument as JSON instead of formatted text
        #[arg(long)]
        json: bool,
    },
    /// Report repeated structural patterns across a directory's peers
    /// (aggregation over the `files` table; no LLM, no new evidence).
    Conventions {
        /// Repository-relative directory path (e.g. `src/modules` or `src/modules/blockchain`)
        path: String,
        /// Emit the PeerStructureReport as JSON instead of formatted text
        #[arg(long)]
        json: bool,
    },
    /// Module→module coupling matrix from structural_edges.
    /// Modules = immediate child directories of the subject (default: src/modules).
    Coupling {
        /// Repository-relative directory whose immediate children are the modules.
        #[arg(default_value = "src/modules")]
        path: String,
        /// Emit the ModuleCouplingReport as JSON instead of formatted text
        #[arg(long)]
        json: bool,
    },
    /// Per-author commit counts (with first/last touch) for a subject.
    ///
    /// Subject can be the repo root (omit), a directory (any depth), or a
    /// file path.  Files with a materialised FileIdentity aggregate across
    /// the full rename chain; historical file paths redirect with a note.
    ///
    /// Rows are OBSERVED COMMITS by (author_name, author_email) — no alias
    /// merging, no ownership scoring, no time-decay weighting.  A commit
    /// that touched N files in the subtree counts as 1, not N.
    ///
    /// Repo-scoped: only commits ingested against the current repo appear.
    Authors {
        /// Repository-relative path (file, directory, or omit for repo root).
        #[arg(default_value = ".")]
        path: String,
        /// Force subject kind: auto | dir | file.  Use when the path does
        /// not exist on disk (e.g. querying a deleted file) or when the
        /// auto-detection heuristic misclassifies.
        #[arg(long)]
        kind: Option<String>,
        /// Emit the AuthorsReport as JSON instead of formatted text
        #[arg(long)]
        json: bool,
    },
    /// Drill down into one Atlas record.  Subject can be a commit hash,
    /// PR (#N or pr#N), issue (issue#N), file path, identity (id:N),
    /// document (doc:PATH or plain path), config artifact (config:PATH),
    /// or ingest run (run:N or run:latest).
    ///
    /// AUTO-DETECTION FALLBACK: when the subject doesn't match a commit
    /// hash, prefixed form, or existing document/config entry, it is
    /// treated as a file path.  This means `atlas show anything` will
    /// return a `file` subject even for paths Atlas has never seen — a
    /// deliberate choice so bare paths are always a valid query.  Use
    /// `--kind commit` (or another explicit kind) if you want strict
    /// resolution instead.
    Show {
        /// Subject to resolve — see `--kind` for explicit disambiguation.
        subject: String,
        /// Force the subject kind: auto | commit | pr | issue | file |
        /// identity | document | config | run.
        #[arg(long)]
        kind: Option<String>,
        /// Emit full section contents (no truncation, full body).
        #[arg(long)]
        full: bool,
        /// Max rows per section before truncation (default 10).
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Emit the ShowRecord as JSON instead of formatted text
        #[arg(long)]
        json: bool,
    },
    /// Inventory of module directories under a subject (default: src/modules).
    ///
    /// Reports deterministic file/commit/edge counts per immediate child
    /// directory.  Module names are path segments — not business-domain labels.
    Modules {
        /// Parent path whose immediate children are modules.
        #[arg(default_value = "src/modules")]
        path: String,
        /// Emit the ModulesReport as JSON
        #[arg(long)]
        json: bool,
    },
    /// Test ↔ module path linkage under documented deterministic rules.
    ///
    /// Links are path-based only.  No ownership or expertise claim is made.
    Tests {
        /// Modules subject used for discovery (default: src/modules).
        #[arg(long, default_value = "src/modules")]
        modules: String,
        /// Optional path filter (only tests under this path).
        path: Option<String>,
        /// Emit the TestModuleReport as JSON
        #[arg(long)]
        json: bool,
    },
    /// NPM package.json declarations linked to structural import observations.
    ///
    /// DECLARED ≠ OBSERVED ≠ runtime usage.  Observed means static
    /// UNRESOLVED:external structural edges only.
    Deps {
        /// Max packages to print in text mode (0 = all). JSON always emits all.
        #[arg(long, default_value_t = 40)]
        limit: usize,
        /// Emit the DependencyLinkageReport as JSON
        #[arg(long)]
        json: bool,
    },
    /// Cross-directory co-change cohorts under a subject (default: src/modules).
    ///
    /// Cohorts are connected components of directory pairs that co-change
    /// above an explicit threshold.  Not a business-domain claim.
    Cohorts {
        /// Parent path whose immediate children are cohort candidates.
        #[arg(default_value = "src/modules")]
        path: String,
        /// Minimum shared commits for a pair edge (default 2).
        #[arg(long)]
        threshold: Option<usize>,
        /// Emit the CohortsReport as JSON
        #[arg(long)]
        json: bool,
    },
    /// Deviations from observed structural patterns (not quality judgments).
    ///
    /// Reuses B1 peer-structure deviations, missing test associations, and
    /// declared-but-unobserved dependencies.  Language: "deviates from
    /// observed peer pattern" — never "bad architecture".
    Anomalies {
        /// Subject for peer/module analysis (default: src/modules).
        #[arg(default_value = "src/modules")]
        path: String,
        /// Emit the AnomaliesReport as JSON
        #[arg(long)]
        json: bool,
    },
    /// Configuration artifact inventory and per-path historical provenance.
    ///
    /// Without PATH: list ingested configuration_artifacts.
    /// With PATH: artifact content identity + commits that touched the path.
    /// Historical configuration content snapshots are NOT available.
    Config {
        /// Configuration file path (omit for inventory).
        path: Option<String>,
        /// Emit JSON (ConfigInventoryReport or ConfigArtifactReport)
        #[arg(long)]
        json: bool,
    },
    /// Repository orientation map (Section C1) — modules, coupling, hot files,
    /// config, coverage as evidence-backed claims.  Not an LLM essay.
    Map {
        #[arg(long)]
        json: bool,
    },
    /// Focus a module/path/file neighborhood (Section C2).
    Focus {
        /// Module name, directory, or file path
        subject: String,
        #[arg(long)]
        json: bool,
    },
    /// Blast-radius investigation neighbors (Section C3). Ranked guidance only —
    /// not change-safety or ownership.
    Impact {
        /// File or directory path
        subject: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum ProjectCommand {
    /// Create a project (idempotent — reusing the same name returns the existing project)
    Init {
        name: String,
        #[arg(long)]
        description: Option<String>,
    },
    /// Register a repository at a local path against an existing project
    Register {
        /// Project name
        project: String,
        /// Absolute or relative path to the repository
        path: String,
        /// Free-form descriptive label ("api", "notifier", …); not used for classification
        #[arg(long)]
        role: Option<String>,
        /// Override the repository name (defaults to the path's basename)
        #[arg(long)]
        name: Option<String>,
    },
    /// List projects, or list repositories inside one project when --project is given
    List {
        /// Restrict to repositories inside a project
        project: Option<String>,
    },
    /// Ingest every accessible repository in a project using the per-repo pipeline
    Ingest {
        project: String,
        #[arg(long)]
        typescript: bool,
        #[arg(long)]
        github: bool,
    },
    /// Print the project census (observed ProfileClaims for every repository)
    Census {
        project: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum CampaignCommand {
    /// Show the next campaign ready for implementation, or the closest candidates
    Next,
}

#[derive(Subcommand)]
enum InvestigationsCommand {
    /// List stored investigations for the current repository (newest first)
    List {
        #[arg(long, default_value_t = 20)]
        limit: i64,
    },
    /// Show a stored investigation by ID
    Show {
        id: i64,
        #[arg(long)]
        json: bool,
    },
    /// Diff two stored investigations — show what candidates changed
    Diff {
        id_a: i64,
        id_b: i64,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Default to WARN so stdout/stderr are clean for scripts/pipes.  Users
    // who want the previous DEBUG chatter can set `RUST_LOG=debug`.
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")))
        .without_time()
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Status | Commands::Doctor => commands::status::run(),
        Commands::Agent {
            question,
            repo,
            max_steps,
            show_thinking,
            fast,
            no_web,
        } => commands::agent::run(
            &question,
            repo.as_deref(),
            max_steps,
            show_thinking,
            fast,
            no_web,
        ),
        Commands::Init { github, no_gitignore } =>
            commands::init::run(github, no_gitignore),
        Commands::Ingest { path, github, typescript, all_refs } =>
            commands::ingest::run(&path, github, typescript, all_refs),
        Commands::Query { file }        => commands::query::run(&file),
        Commands::Explain { file }      => commands::explain::run(&file),
        Commands::CoChanges { file, min_count } => commands::cochanges::run(&file, min_count),
        Commands::Timeline { file }             => commands::timeline::run(&file),
        Commands::HotFiles { limit, repo }      => commands::hotfiles::run(limit, repo.as_deref()),
        Commands::WhenIntroduced { file }       => commands::whenintroduced::run(&file),
        Commands::Context { file, json }        => commands::context::run(&file, json),
        Commands::Feedback { file }             => commands::feedback::run(&file),
        Commands::Investigate { anchors, json, raw, repo, issue, file, no_ai, rounds } => {
            if anchors.is_empty() && issue.is_none() && file.is_none() {
                anyhow::bail!("investigate requires anchors/question, --issue N, or --file PATH");
            }
            commands::investigate::run(
                &anchors,
                json,
                raw,
                repo.as_deref(),
                issue,
                file.as_deref(),
                no_ai,
                rounds,
            )
        }
        Commands::Search { anchors, json }          => commands::search::run(&anchors, json),
        Commands::Structural { file, reverse }          => commands::structural::run(&file, reverse),
        Commands::Callers { subject, callees, limit, json } =>
            commands::callers::run(&subject, callees, limit, json),
        Commands::Implementations { subject, limit, json } =>
            commands::implementations::run(&subject, limit, json),
        Commands::Capabilities { json } => commands::capabilities::run(json),
        Commands::CodeSearch { query, limit, json } =>
            commands::code_search::run(&query, limit, json),
        Commands::ReviewContext { pr_number, json }      => commands::review_context::run(pr_number, json),
        Commands::Campaign { subcommand } => match subcommand {
            CampaignCommand::Next => commands::campaign::run_next(),
        },
        Commands::Plan { issue_number, repo } => commands::plan::run(issue_number, repo.as_deref()),
        Commands::Eval { corpus_dir, verbose } => commands::eval::run(&corpus_dir, verbose),
        Commands::Investigations { subcommand } => match subcommand {
            InvestigationsCommand::List { limit } =>
                commands::investigations::run_list(limit),
            InvestigationsCommand::Show { id, json } =>
                commands::investigations::run_show(id, json),
            InvestigationsCommand::Diff { id_a, id_b } =>
                commands::investigations::run_diff(id_a, id_b),
        },
        Commands::Project { subcommand } => match subcommand {
            ProjectCommand::Init { name, description } =>
                commands::project::run_init(&name, description.as_deref()),
            ProjectCommand::Register { project, path, role, name } =>
                commands::project::run_register(&project, &path, role.as_deref(), name.as_deref()),
            ProjectCommand::List { project } =>
                commands::project::run_list(project.as_deref()),
            ProjectCommand::Ingest { project, typescript, github } =>
                commands::project::run_ingest(&project, typescript, github),
            ProjectCommand::Census { project, json } =>
                commands::project::run_census(&project, json),
        },
        Commands::Tree { depth, json } => commands::tree::run(depth, json),
        Commands::Inspect { path, json } => commands::inspect::run(&path, json),
        Commands::Conventions { path, json } => commands::conventions::run(&path, json),
        Commands::Coupling { path, json } => commands::coupling::run(&path, json),
        Commands::Authors { path, kind, json } =>
            commands::authors::run(&path, kind.as_deref(), json),
        Commands::Show { subject, kind, full, limit, json } =>
            commands::show::run(&subject, kind.as_deref(), full, limit, json),
        Commands::Modules { path, json } => commands::modules::run(&path, json),
        Commands::Tests { modules, path, json } =>
            commands::tests_cmd::run(&modules, path.as_deref(), json),
        Commands::Deps { limit, json } => commands::deps::run(json, limit),
        Commands::Cohorts { path, threshold, json } =>
            commands::cohorts::run(&path, threshold, json),
        Commands::Anomalies { path, json } => commands::anomalies::run(&path, json),
        Commands::Config { path, json } =>
            commands::config_cmd::run(path.as_deref(), json),
        Commands::Map { json } => commands::map_cmd::run(json),
        Commands::Focus { subject, json } => commands::focus_cmd::run(&subject, json),
        Commands::Impact { subject, json } => commands::impact_cmd::run(&subject, json),
    }
}
