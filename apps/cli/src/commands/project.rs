//! CLI presentation for the project command family.
//!
//! Delegates all logic to `atlas_core::project`.  This file contains only:
//!   - argument plumbing
//!   - human/JSON output formatting
//!
//! No business logic; no schema knowledge; no ranking; no interpretation.

use anyhow::Result;
use atlas_core::IngestOptions;
use atlas_ir::{AccessState, ExistenceSource, IngestionState};
use atlas_storage::Store;

fn open_store() -> Result<Store> {
    let db_path = super::resolve_db_path();
    Ok(Store::open(&db_path)?)
}

// ── init ─────────────────────────────────────────────────────────────────────

pub fn run_init(name: &str, description: Option<&str>) -> Result<()> {
    let store = open_store()?;
    let project = atlas_core::create_project(name, description, &store)?;
    println!("Project '{}' ready (id {}).", project.name, project.id);
    if let Some(desc) = &project.description {
        println!("  description: {}", desc);
    }
    Ok(())
}

// ── register ─────────────────────────────────────────────────────────────────

pub fn run_register(project: &str, path: &str, role: Option<&str>, name: Option<&str>) -> Result<()> {
    let store = open_store()?;
    let repo = atlas_core::register_repository_at_path(project, path, role, name, &store)?;

    println!("Registered '{}' in project '{}'.", repo.name, project);
    if let Some(role) = &repo.role_label {
        println!("  role:      {}", role);
    }
    if let Some(path) = &repo.local_path {
        println!("  path:      {}", path);
    }
    println!("  existence: {}", existence_label(&repo.existence_source));
    println!("  access:    {}", access_label(&repo.access_state));
    println!("  ingestion: {}", ingestion_label(&repo.ingestion_state));
    Ok(())
}

// ── list ─────────────────────────────────────────────────────────────────────

pub fn run_list(project: Option<&str>) -> Result<()> {
    let store = open_store()?;

    match project {
        None => {
            let projects = atlas_core::list_projects(&store)?;
            if projects.is_empty() {
                println!("No projects registered. Run `atlas project init <name>` first.");
                return Ok(());
            }
            println!("{:<24}  {}", "PROJECT", "DESCRIPTION");
            println!("{}", "-".repeat(60));
            for p in projects {
                println!("{:<24}  {}", p.name, p.description.unwrap_or_default());
            }
        }
        Some(name) => {
            let repos = atlas_core::list_repositories(name, &store)?;
            if repos.is_empty() {
                println!("Project '{}' has no repositories registered yet.", name);
                println!("Run `atlas project register {} <path>` to add one.", name);
                return Ok(());
            }
            println!("{:<24}  {:<14}  {:<14}  {:<14}  {}",
                     "REPOSITORY", "EXISTENCE", "ACCESS", "INGESTION", "PATH");
            println!("{}", "-".repeat(110));
            for r in repos {
                println!("{:<24}  {:<14}  {:<14}  {:<14}  {}",
                    r.name,
                    existence_label(&r.existence_source),
                    access_label(&r.access_state),
                    ingestion_label(&r.ingestion_state),
                    r.local_path.unwrap_or_else(|| "-".to_string()));
            }
        }
    }
    Ok(())
}

// ── ingest ───────────────────────────────────────────────────────────────────

pub fn run_ingest(project: &str, typescript: bool, github: bool) -> Result<()> {
    let store = open_store()?;
    let opts = IngestOptions { github, typescript };

    println!("Ingesting project '{}' …", project);
    let summaries = atlas_core::ingest_project(project, opts, &store)?;

    if summaries.is_empty() {
        println!("No repositories registered under '{}'.", project);
        return Ok(());
    }

    for s in &summaries {
        if s.skipped {
            println!("  [skip] {}  (not accessible)", s.repository.name);
            continue;
        }
        println!(
            "  [done] {}  commits={} renames={} identities={} prs={} edges={} docs={}",
            s.repository.name,
            s.commits, s.rename_records, s.identities, s.prs, s.structural_edges, s.documents,
        );
    }

    let done = summaries.iter().filter(|s| !s.skipped).count();
    let skipped = summaries.iter().filter(|s| s.skipped).count();
    println!("Ingested {} repositories ({} skipped).", done, skipped);
    Ok(())
}

// ── census ───────────────────────────────────────────────────────────────────

pub fn run_census(project: &str, json: bool) -> Result<()> {
    let store = open_store()?;
    let census = atlas_core::build_project_census(project, &store)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&census)?);
        return Ok(());
    }

    println!("Project census: {}", census.project.name);
    if let Some(desc) = &census.project.description {
        println!("  {}", desc);
    }
    println!();

    if census.entries.is_empty() {
        println!("No repositories registered.");
        return Ok(());
    }

    for entry in &census.entries {
        println!("── {} ({})", entry.repository.name,
                 access_label(&entry.repository.access_state));
        if let Some(path) = &entry.repository.local_path {
            println!("   path: {}", path);
        }
        if entry.claims.is_empty() {
            println!("   (no observed claims)");
            println!();
            continue;
        }

        // Group claims by kind for readability, but keep the underlying
        // evidence for each — no summarisation, just grouping.
        use std::collections::BTreeMap;
        let mut by_kind: BTreeMap<&str, Vec<&atlas_ir::ProfileClaim>> = BTreeMap::new();
        for c in &entry.claims {
            by_kind.entry(c.kind.as_str()).or_default().push(c);
        }
        for (kind, claims) in by_kind {
            let values: Vec<&str> = claims.iter().map(|c| c.value.as_str()).collect();
            println!("   {:<18} {}", kind, values.join(", "));
        }
        println!();
    }

    Ok(())
}

// ── Label helpers ────────────────────────────────────────────────────────────

fn existence_label(s: &ExistenceSource) -> &'static str {
    match s {
        ExistenceSource::LocalObserved => "local-observed",
        ExistenceSource::UserConfirmed => "user-confirmed",
    }
}

fn access_label(s: &AccessState) -> &'static str {
    match s {
        AccessState::Accessible    => "accessible",
        AccessState::NotAccessible => "not-accessible",
    }
}

fn ingestion_label(s: &IngestionState) -> &'static str {
    match s {
        IngestionState::Ingested      => "ingested",
        IngestionState::NotIngested   => "not-ingested",
        IngestionState::NotApplicable => "n/a",
    }
}
