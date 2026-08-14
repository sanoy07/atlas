//! Step 1 acceptance tests for extended document ingestion.
//!
//! Verifies the four sources handled by `ingest_documents`:
//!   1. `docs/decisions/*.md`  → doc_type = "decision"   (top-level, unchanged)
//!   2. `docs/adr/*.md`        → doc_type = "adr"        (top-level, unchanged)
//!   3. root `README.md`       → doc_type = "readme"     (new)
//!   4. any other `*.md` under `docs/` recursively → doc_type = "doc" (new)
//!
//! Files under `docs/decisions/` and `docs/adr/` MUST NOT be reclassified as
//! generic docs by the recursive pass — precedence is explicit.

use atlas_core::ingest_documents;
use atlas_storage::Store;
use std::path::Path;
use tempfile::TempDir;

// ── Fixture helpers ──────────────────────────────────────────────────────────

fn temp_repo() -> (TempDir, String, Store) {
    let dir = TempDir::new().expect("tempdir");
    let repo = dir.path().to_string_lossy().into_owned();
    let db = dir.path().join("atlas.db");
    let store = Store::open(db.to_str().unwrap()).expect("store");
    (dir, repo, store)
}

fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

fn list_documents(store: &Store, repo_path: &str) -> Vec<(String, String, String)> {
    store.list_documents(repo_path).unwrap()
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn ingests_root_readme() {
    let (dir, repo, store) = temp_repo();
    write(dir.path(), "README.md", "# Atlas\n\nA deterministic engine.\n");

    let count = ingest_documents(&repo, &store).unwrap();
    assert_eq!(count, 1, "one README should be ingested");

    let docs = list_documents(&store, &repo);
    assert_eq!(docs.len(), 1);
    let (path, doc_type, _title) = &docs[0];
    assert_eq!(path, "README.md");
    assert_eq!(doc_type, "readme");
}

#[test]
fn walks_docs_directory_recursively() {
    let (dir, repo, store) = temp_repo();
    write(dir.path(), "docs/guides/setup.md",         "# Setup\n");
    write(dir.path(), "docs/architecture/overview.md", "# Overview\n");
    write(dir.path(), "docs/notes.md",                 "loose note\n");

    let count = ingest_documents(&repo, &store).unwrap();
    assert_eq!(count, 3, "three generic docs should be ingested");

    let docs = list_documents(&store, &repo);
    let by_path: std::collections::HashMap<_, _> =
        docs.iter().map(|(p, t, _)| (p.as_str(), t.as_str())).collect();

    assert_eq!(by_path.get("docs/architecture/overview.md"), Some(&"doc"));
    assert_eq!(by_path.get("docs/guides/setup.md"),          Some(&"doc"));
    assert_eq!(by_path.get("docs/notes.md"),                 Some(&"doc"));
}

#[test]
fn preserves_decision_and_adr_types() {
    let (dir, repo, store) = temp_repo();

    write(dir.path(), "docs/decisions/2026-01-01-repository-awareness.md",
          "---\ntitle: Repository Awareness\ndate: 2026-01-01\n---\n\nBody.\n");
    write(dir.path(), "docs/adr/0001-use-sqlite.md",
          "---\ntitle: Use SQLite\n---\n\nBody.\n");
    write(dir.path(), "docs/guides/setup.md",      "# Setup\n");
    write(dir.path(), "docs/architecture/overview.md", "# Overview\n");
    write(dir.path(), "README.md",                  "# Atlas\n");

    let count = ingest_documents(&repo, &store).unwrap();
    assert_eq!(count, 5, "one decision + one adr + two docs + one readme");

    let docs = list_documents(&store, &repo);
    let by_path: std::collections::HashMap<_, _> =
        docs.iter().map(|(p, t, _)| (p.as_str(), t.as_str())).collect();

    // Existing behaviour preserved.
    assert_eq!(
        by_path.get("docs/decisions/2026-01-01-repository-awareness.md"),
        Some(&"decision"),
        "top-level file under docs/decisions/ must keep doc_type = decision"
    );
    assert_eq!(
        by_path.get("docs/adr/0001-use-sqlite.md"),
        Some(&"adr"),
        "top-level file under docs/adr/ must keep doc_type = adr"
    );
    // New behaviour.
    assert_eq!(by_path.get("docs/guides/setup.md"),         Some(&"doc"));
    assert_eq!(by_path.get("docs/architecture/overview.md"), Some(&"doc"));
    assert_eq!(by_path.get("README.md"),                     Some(&"readme"));

    // No duplicates: each unique file_path appears exactly once.
    let mut paths: Vec<&str> = docs.iter().map(|(p, _, _)| p.as_str()).collect();
    let unique = paths.iter().copied().collect::<std::collections::HashSet<_>>();
    paths.sort();
    assert_eq!(paths.len(), unique.len(), "no file_path may be inserted twice");

    // Decisions/adrs are NOT reclassified as generic docs by the recursive pass.
    let decision_row = docs.iter()
        .find(|(p, _, _)| p == "docs/decisions/2026-01-01-repository-awareness.md")
        .expect("decision row present");
    assert_eq!(decision_row.1, "decision");
    let adr_row = docs.iter()
        .find(|(p, _, _)| p == "docs/adr/0001-use-sqlite.md")
        .expect("adr row present");
    assert_eq!(adr_row.1, "adr");
}

#[test]
fn empty_repo_returns_zero() {
    let (_dir, repo, store) = temp_repo();
    let count = ingest_documents(&repo, &store).unwrap();
    assert_eq!(count, 0);
    assert!(list_documents(&store, &repo).is_empty());
}

#[test]
fn readme_without_docs_dir_returns_one() {
    let (dir, repo, store) = temp_repo();
    write(dir.path(), "README.md", "# Atlas\n");
    let count = ingest_documents(&repo, &store).unwrap();
    assert_eq!(count, 1);
    let docs = list_documents(&store, &repo);
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].1, "readme");
}
