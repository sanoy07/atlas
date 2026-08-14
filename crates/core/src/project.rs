//! Project layer — composition over existing per-repository pipeline functions.
//!
//! This module introduces no new concepts.  Every type it uses
//! (`ProjectRecord`, `RepositoryRecord`, `ProjectCensus`, `ProfileClaim`) already
//! exists in the IR; every storage call already exists on `Store`; every
//! ingestion function already exists in `atlas_core`.  What this module does is
//! compose them so that a *set* of repositories can be operated on as one
//! project — the dormant scaffolding wired into a working pipeline.
//!
//! Cross-repository traversal is NOT implemented here.  That primitive is
//! deferred until the Phase 2 benchmark demonstrates it is earned.

use anyhow::{Context, Result};
use atlas_ir::{
    AccessState, ExistenceSource, IngestionState, ProjectCensus, ProjectRecord,
    RepositoryCensusEntry, RepositoryRecord,
};
use atlas_storage::Store;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::repo_inspector;

/// Schema version for `ProjectCensus` JSON.  Bump only on incompatible shape
/// changes.  Kept alongside `InvestigationDocument`'s versioning discipline.
const CENSUS_SCHEMA_VERSION: u32 = 1;

// ── Project creation and listing ─────────────────────────────────────────────

/// Create a new project, or return the existing one if it already exists with
/// this name.  Idempotent so callers can re-run `init` without losing state.
pub fn create_project(name: &str, description: Option<&str>, store: &Store) -> Result<ProjectRecord> {
    if let Some(existing) = store.get_project_by_name(name)? {
        return Ok(existing);
    }
    let id = store.create_project(name, description)?;
    Ok(ProjectRecord {
        id,
        name:        name.to_string(),
        description: description.map(|s| s.to_string()),
    })
}

/// Look up a project by name.  Returns None if no project with this name exists.
pub fn get_project(name: &str, store: &Store) -> Result<Option<ProjectRecord>> {
    store.get_project_by_name(name)
}

pub fn list_projects(store: &Store) -> Result<Vec<ProjectRecord>> {
    store.list_projects()
}

// ── Repository registration ──────────────────────────────────────────────────

/// Register a repository at a local path against an existing project.
///
/// Determines `existence_source`, `access_state`, and initial `ingestion_state`
/// from what is directly observable on disk.  The repository name is derived
/// from the final path segment when not supplied; callers should pass an
/// explicit name whenever two repos might share a directory basename.
///
/// This is idempotent when the same `(project_id, name)` already exists;
/// duplicate registration returns the existing record unchanged.
pub fn register_repository_at_path(
    project_name: &str,
    repo_path:    &str,
    role_label:   Option<&str>,
    override_name: Option<&str>,
    store:        &Store,
) -> Result<RepositoryRecord> {
    let project = store
        .get_project_by_name(project_name)?
        .with_context(|| format!("project '{}' not found — run `atlas project init {}` first", project_name, project_name))?;

    let canonical = std::fs::canonicalize(repo_path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| repo_path.to_string());

    // Determine observed state.
    let path = Path::new(&canonical);
    let is_git = path.join(".git").exists();
    let (existence_source, access_state, ingestion_state) = if is_git {
        (ExistenceSource::LocalObserved, AccessState::Accessible, IngestionState::NotIngested)
    } else {
        // Registration of a not-yet-cloned repository is a valid observation —
        // we simply record it as inaccessible rather than refusing.
        (ExistenceSource::UserConfirmed, AccessState::NotAccessible, IngestionState::NotApplicable)
    };

    // Derive name.
    let derived_name = override_name
        .map(|s| s.to_string())
        .or_else(|| path.file_name().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_else(|| canonical.clone());

    // If a repo with this project_id + name already exists, return it unchanged.
    for existing in store.list_repositories(project.id)? {
        if existing.name == derived_name {
            return Ok(existing);
        }
    }

    let local_path_opt: Option<&str> = if matches!(access_state, AccessState::Accessible) {
        Some(canonical.as_str())
    } else {
        None
    };

    let id = store.register_repository(
        project.id,
        &derived_name,
        role_label,
        local_path_opt,
        None,
        &existence_source,
        &access_state,
        &ingestion_state,
    )?;

    Ok(RepositoryRecord {
        id,
        project_id: project.id,
        name:       derived_name,
        role_label: role_label.map(|s| s.to_string()),
        local_path: local_path_opt.map(|s| s.to_string()),
        remote_url: None,
        existence_source,
        access_state,
        ingestion_state,
    })
}

pub fn list_repositories(project_name: &str, store: &Store) -> Result<Vec<RepositoryRecord>> {
    let project = store
        .get_project_by_name(project_name)?
        .with_context(|| format!("project '{}' not found", project_name))?;
    store.list_repositories(project.id)
}

// ── Project ingestion — composition of existing per-repo pipeline ────────────

/// Options controlling which per-repository ingest stages run.  Mirrors the
/// existing `atlas ingest` flags so behaviour is predictable.
#[derive(Debug, Clone, Copy, Default)]
pub struct IngestOptions {
    pub github:     bool,
    pub typescript: bool,
}

/// Result of one repository's ingestion within a project run.
#[derive(Debug, Clone)]
pub struct RepositoryIngestSummary {
    pub repository:      RepositoryRecord,
    pub commits:         usize,
    pub rename_records:  usize,
    pub identities:      usize,
    pub prs:             usize,
    pub structural_edges: usize,
    pub documents:       usize,
    /// Skipped: repository was NotAccessible.
    pub skipped:         bool,
}

/// Run the existing per-repository ingest pipeline for every accessible
/// repository in this project.  Updates `ingestion_state` to `Ingested` on
/// success.  Errors on a single repository do not abort the whole project —
/// they are propagated as the function's `Result` only when *no* repository
/// could be ingested.
pub fn ingest_project(
    project_name: &str,
    opts:         IngestOptions,
    store:        &Store,
) -> Result<Vec<RepositoryIngestSummary>> {
    let project = store
        .get_project_by_name(project_name)?
        .with_context(|| format!("project '{}' not found", project_name))?;
    let repos = store.list_repositories(project.id)?;

    let mut summaries = Vec::with_capacity(repos.len());
    for repo in repos {
        let Some(local_path) = repo.local_path.clone() else {
            summaries.push(RepositoryIngestSummary {
                repository:       repo,
                commits:          0,
                rename_records:   0,
                identities:       0,
                prs:              0,
                structural_edges: 0,
                documents:        0,
                skipped:          true,
            });
            continue;
        };

        // Record ingest_runs so `atlas status` can show last ingest after
        // project-level fan-out (previously only `atlas ingest` wrote runs).
        let git_head = std::process::Command::new("git")
            .args(["-C", &local_path, "rev-parse", "HEAD"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
        let git_branch = std::process::Command::new("git")
            .args(["-C", &local_path, "rev-parse", "--abbrev-ref", "HEAD"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
        let run_id = store
            .start_ingest_run(
                &local_path,
                env!("CARGO_PKG_VERSION"),
                git_head.as_deref(),
                git_branch.as_deref(),
                "project_ingest",
            )
            .unwrap_or(-1);

        let mut stage_results: Vec<serde_json::Value> = Vec::new();
        let mut push_stage = |name: &str, status: &str, detail: String| {
            stage_results.push(serde_json::json!({
                "stage": name,
                "status": status,
                "detail": detail,
            }));
        };

        let commits = match crate::ingest_git(&local_path, store) {
            Ok(n) => {
                push_stage("git history", "ok", format!("{n} commits"));
                n
            }
            Err(e) => {
                push_stage("git history", "failed", e.to_string());
                0
            }
        };
        let rename_records = match crate::ingest_rename_evidence(&local_path, store) {
            Ok(n) => {
                push_stage("rename evidence", "ok", format!("{n} rename records"));
                n
            }
            Err(e) => {
                push_stage("rename evidence", "failed", e.to_string());
                0
            }
        };
        let identities = match crate::rebuild_file_identities(&local_path, store) {
            Ok(n) => {
                push_stage("file identities", "ok", format!("{n} identities"));
                n
            }
            Err(e) => {
                push_stage("file identities", "failed", e.to_string());
                0
            }
        };

        let mut prs = 0;
        if opts.github {
            match crate::ingest_github(&local_path, store) {
                Ok(n) => {
                    prs = n;
                    push_stage("github", "ok", format!("{n} PRs"));
                }
                Err(e) => push_stage("github", "failed", e.to_string()),
            }
        } else {
            push_stage("github", "skipped", "not requested".into());
        }

        let mut structural_edges = 0;
        if opts.typescript {
            match crate::ingest_typescript(&local_path, store) {
                Ok(n) => {
                    structural_edges += n;
                    push_stage("typescript", "ok", format!("{n} edges"));
                }
                Err(e) => push_stage("typescript", "failed", e.to_string()),
            }
        }
        if crate::repo_has_c_files(&local_path) {
            match crate::ingest_c(&local_path, store) {
                Ok(n) => {
                    structural_edges += n;
                    push_stage("c structural", "ok", format!("{n} edges"));
                }
                Err(e) => push_stage("c structural", "failed", e.to_string()),
            }
        }
        if crate::repo_has_rust_files(&local_path) {
            match crate::ingest_rust(&local_path, store) {
                Ok(n) => {
                    structural_edges += n;
                    push_stage("rust structural", "ok", format!("{n} edges"));
                }
                Err(e) => push_stage("rust structural", "failed", e.to_string()),
            }
        }
        if crate::repo_has_python_files(&local_path) {
            match crate::ingest_python(&local_path, store) {
                Ok(n) => {
                    structural_edges += n;
                    push_stage("python structural", "ok", format!("{n} edges"));
                }
                Err(e) => push_stage("python structural", "failed", e.to_string()),
            }
        }

        let documents = match crate::ingest_documents(&local_path, store) {
            Ok(n) => {
                push_stage("documents", "ok", format!("{n} docs"));
                n
            }
            Err(e) => {
                push_stage("documents", "failed", e.to_string());
                0
            }
        };
        let _ = crate::build_lexicon(&local_path, store);
        push_stage("lexicon", "ok", "built".into());

        let stages_json = serde_json::to_string(&stage_results).unwrap_or_else(|_| "[]".into());
        let any_failed = stage_results
            .iter()
            .any(|s| s.get("status").and_then(|v| v.as_str()) == Some("failed"));
        let exit_status = if any_failed { "partial" } else { "ok" };
        if run_id >= 0 {
            let _ = store.finish_ingest_run(run_id, exit_status, &stages_json, "[]");
        }

        store.update_ingestion_state(repo.id, &IngestionState::Ingested)?;

        summaries.push(RepositoryIngestSummary {
            repository: RepositoryRecord { ingestion_state: IngestionState::Ingested, ..repo },
            commits,
            rename_records,
            identities,
            prs,
            structural_edges,
            documents,
            skipped: false,
        });
    }

    Ok(summaries)
}

// ── Project census — a projection over existing evidence ─────────────────────

/// Build a `ProjectCensus` for a project.  For every accessible repository the
/// inspector runs, its claims are persisted (replacing any prior claims), and
/// the resulting per-repository entries are collected.  Repositories that are
/// not accessible are included with an empty claim list — the fact of their
/// registration is itself an observation.
pub fn build_project_census(project_name: &str, store: &Store) -> Result<ProjectCensus> {
    let project = store
        .get_project_by_name(project_name)?
        .with_context(|| format!("project '{}' not found", project_name))?;
    let repos = store.list_repositories(project.id)?;

    let now = SystemTime::now().duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let mut entries = Vec::with_capacity(repos.len());
    for repo in repos {
        let (claims, inspected_at) = match &repo.local_path {
            Some(path) => {
                let observed = repo_inspector::inspect_repository(path)?;
                store.replace_profile_claims(repo.id, &observed, now)?;
                (observed, Some(now))
            }
            None => {
                // Not accessible — surface prior claims if any (there won't be
                // any for a freshly-registered UserConfirmed repo).
                let (prior, ts) = store.load_profile_claims(repo.id)?;
                (prior, ts)
            }
        };
        entries.push(RepositoryCensusEntry {
            repository: repo,
            claims,
            inspected_at,
        });
    }

    Ok(ProjectCensus {
        schema_version: CENSUS_SCHEMA_VERSION,
        project,
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn temp_store() -> (TempDir, Store) {
        let dir = TempDir::new().unwrap();
        let db = dir.path().join("atlas.db");
        let store = Store::open(db.to_str().unwrap()).unwrap();
        (dir, store)
    }

    #[test]
    fn create_project_is_idempotent() {
        let (_dir, store) = temp_store();
        let a = create_project("rwatp", Some("real world asset tokenization platform"), &store).unwrap();
        let b = create_project("rwatp", Some("different description ignored"), &store).unwrap();
        assert_eq!(a.id, b.id);
        assert_eq!(list_projects(&store).unwrap().len(), 1);
    }

    #[test]
    fn register_repository_records_observed_state() {
        let (_dir, store) = temp_store();
        create_project("p", None, &store).unwrap();

        // Non-git directory → UserConfirmed / NotAccessible
        let non_git_dir = TempDir::new().unwrap();
        std::fs::write(non_git_dir.path().join("README.md"), "hi").unwrap();
        let repo = register_repository_at_path(
            "p",
            non_git_dir.path().to_str().unwrap(),
            Some("api"),
            Some("api"),
            &store,
        ).unwrap();
        assert_eq!(repo.existence_source, ExistenceSource::UserConfirmed);
        assert_eq!(repo.access_state,     AccessState::NotAccessible);
        assert_eq!(repo.ingestion_state,  IngestionState::NotApplicable);
        assert!(repo.local_path.is_none());
    }

    #[test]
    fn register_repository_is_idempotent() {
        let (_dir, store) = temp_store();
        create_project("p", None, &store).unwrap();
        let d = TempDir::new().unwrap();
        std::fs::write(d.path().join("README.md"), "hi").unwrap();
        let a = register_repository_at_path("p", d.path().to_str().unwrap(), None, Some("same"), &store).unwrap();
        let b = register_repository_at_path("p", d.path().to_str().unwrap(), None, Some("same"), &store).unwrap();
        assert_eq!(a.id, b.id);
        assert_eq!(list_repositories("p", &store).unwrap().len(), 1);
    }

    #[test]
    fn census_returns_empty_entries_when_no_repos() {
        let (_dir, store) = temp_store();
        create_project("empty", None, &store).unwrap();
        let census = build_project_census("empty", &store).unwrap();
        assert_eq!(census.project.name, "empty");
        assert!(census.entries.is_empty());
        assert_eq!(census.schema_version, CENSUS_SCHEMA_VERSION);
    }
}
