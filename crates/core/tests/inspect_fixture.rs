//! Step 3 acceptance tests for `atlas_core::inspect`.
//!
//! Covers: file/directory kind detection, prefix aggregation of commits,
//! subtree isolation (no sibling leakage), structural-edge partitioning
//! into internal/depends_on/used_by, documents inside subject, Module
//! ProfileClaim relevance, and nonexistent-path graceful behaviour.

use atlas_core::inspect;
use atlas_ir::{
    ArtifactRole, InspectionSubjectKind, ProfileClaimKind, StructuralEdge, StructuralEdgeKind,
    StructuralEvidence, TreeNodeKind,
};
use atlas_storage::Store;
use chrono::{DateTime, Utc};
use std::path::Path;
use tempfile::TempDir;

// ── Fixture helpers ─────────────────────────────────────────────────────────

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

fn mkdir(root: &Path, rel: &str) {
    std::fs::create_dir_all(root.join(rel)).unwrap();
}

fn commit(store: &Store, repo_path: &str, hash: &str, msg: &str, ts: i64, files: &[&str]) {
    use atlas_ir::Commit;
    let c = Commit {
        hash:          hash.into(),
        short_hash:    hash[..7.min(hash.len())].into(),
        message:       msg.into(),
        author_name:   "T".into(),
        author_email:  "t@x".into(),
        timestamp:     DateTime::<Utc>::from_timestamp(ts, 0).unwrap(),
        files_changed: files.iter().map(|f| f.to_string()).collect(),

            parents:       vec![],    };
    store.insert_commit(&c, repo_path).unwrap();
}

fn ins_edge(store: &Store, repo_path: &str, source: &str, target: &str, kind: StructuralEdgeKind) {
    let edge = StructuralEdge {
        source_file:   source.into(),
        source_symbol: None,
        target_file:   target.into(),
        target_symbol: None,
        kind,
        evidence:      StructuralEvidence {
            source_file: source.into(),
            line:        Some(1),
            snippet:     "import".into(),
            extractor:   "test".into(),
        },
    };
    store.insert_structural_edge(&edge, repo_path).unwrap();
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn inspect_file_populates_identity_and_role() {
    let (dir, repo, store) = temp_repo();
    write(dir.path(), "src/auth/service.ts", "export {}");
    commit(&store, &repo, "aaaa1111", "add service", 1_700_000_000, &["src/auth/service.ts"]);

    let doc = inspect("src/auth/service.ts", &repo, &store).unwrap();

    assert_eq!(doc.kind, InspectionSubjectKind::File);
    assert_eq!(doc.relative_path, "src/auth/service.ts");
    assert!(doc.exists_on_disk);
    assert_eq!(doc.role, Some(ArtifactRole::ProductionSource));
    assert!(doc.identity.is_some(), "file subject must carry identity");
    assert!(doc.children.is_empty(), "file subject has no children");
    assert_eq!(doc.touch_count, 1);
    assert!(doc.structural_internal.is_empty(),
        "internal edges are meaningless for a file subject");
}

#[test]
fn inspect_directory_lists_immediate_children_sorted() {
    let (dir, repo, store) = temp_repo();
    write(dir.path(), "src/z_last.rs",  "");
    write(dir.path(), "src/a_first.rs", "");
    mkdir(dir.path(), "src/alpha_dir");

    let doc = inspect("src", &repo, &store).unwrap();

    assert_eq!(doc.kind, InspectionSubjectKind::Directory);
    let names: Vec<&str> = doc.children.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["a_first.rs", "alpha_dir", "z_last.rs"]);
    // Directory kind is preserved on the child.
    let alpha = doc.children.iter().find(|c| c.name == "alpha_dir").unwrap();
    assert_eq!(alpha.kind, TreeNodeKind::Directory);
}

#[test]
fn inspect_directory_aggregates_commits_from_subtree() {
    let (dir, repo, store) = temp_repo();
    write(dir.path(), "src/a/x.rs", "");
    write(dir.path(), "src/b/y.rs", "");
    commit(&store, &repo, "c1c1c1c1", "touched x", 1, &["src/a/x.rs"]);
    commit(&store, &repo, "c2c2c2c2", "touched y", 2, &["src/b/y.rs"]);

    let doc = inspect("src", &repo, &store).unwrap();

    assert_eq!(doc.touch_count, 2, "both commits under src/ must be aggregated");
    let hashes: Vec<&str> = doc.recent_activity.iter().map(|c| c.short_hash.as_str()).collect();
    // Sorted newest first.
    assert_eq!(hashes, vec!["c2c2c2c", "c1c1c1c"]);
}

#[test]
fn inspect_directory_does_not_leak_sibling_commits() {
    let (dir, repo, store) = temp_repo();
    write(dir.path(), "src/a/x.rs", "");
    write(dir.path(), "other/y.rs", "");
    commit(&store, &repo, "aaaa1111", "src commit",   1, &["src/a/x.rs"]);
    commit(&store, &repo, "bbbb2222", "other commit", 2, &["other/y.rs"]);

    let doc = inspect("src/a", &repo, &store).unwrap();

    assert_eq!(doc.touch_count, 1, "only src/a/x.rs commit should count");
    assert_eq!(doc.recent_activity.len(), 1);
    assert_eq!(doc.recent_activity[0].short_hash, "aaaa111");
}

#[test]
fn inspect_directory_partitions_internal_vs_boundary_edges() {
    let (dir, repo, store) = temp_repo();
    write(dir.path(), "src/mod/a.rs", "");
    write(dir.path(), "src/mod/b.rs", "");
    write(dir.path(), "src/other.rs", "");
    // inside → inside
    ins_edge(&store, &repo, "src/mod/a.rs", "src/mod/b.rs", StructuralEdgeKind::Imports);
    // inside → outside
    ins_edge(&store, &repo, "src/mod/a.rs", "src/other.rs", StructuralEdgeKind::Imports);
    // outside → inside
    ins_edge(&store, &repo, "src/other.rs", "src/mod/b.rs", StructuralEdgeKind::CallsStatic);

    let doc = inspect("src/mod", &repo, &store).unwrap();

    assert_eq!(doc.structural_internal.len(), 1, "one internal edge (a → b)");
    assert_eq!(doc.structural_internal[0].source_file, "src/mod/a.rs");
    assert_eq!(doc.structural_internal[0].target_file, "src/mod/b.rs");

    assert_eq!(doc.structural_depends_on.len(), 1, "one boundary out edge (a → src/other.rs)");
    assert_eq!(doc.structural_depends_on[0].target_file, "src/other.rs");

    assert_eq!(doc.structural_used_by.len(), 1, "one boundary in edge (src/other.rs → b)");
    assert_eq!(doc.structural_used_by[0].source_file, "src/other.rs");

    // No edge appears in more than one list.
    let all_ids: Vec<String> = doc.structural_internal.iter()
        .chain(doc.structural_depends_on.iter())
        .chain(doc.structural_used_by.iter())
        .map(|e| format!("{}->{}", e.source_file, e.target_file))
        .collect();
    let mut deduped = all_ids.clone();
    deduped.sort();
    deduped.dedup();
    assert_eq!(all_ids.len(), deduped.len(), "each edge must appear in exactly one partition");
}

#[test]
fn inspect_file_subject_uses_outgoing_and_incoming_edges() {
    let (dir, repo, store) = temp_repo();
    write(dir.path(), "src/a.rs", "");
    write(dir.path(), "src/b.rs", "");
    write(dir.path(), "src/c.rs", "");
    ins_edge(&store, &repo, "src/a.rs", "src/b.rs", StructuralEdgeKind::Imports);
    ins_edge(&store, &repo, "src/c.rs", "src/a.rs", StructuralEdgeKind::CallsStatic);

    let doc = inspect("src/a.rs", &repo, &store).unwrap();

    assert!(doc.structural_internal.is_empty(),
        "file subject: internal is always empty");
    assert_eq!(doc.structural_depends_on.len(), 1);
    assert_eq!(doc.structural_depends_on[0].target_file, "src/b.rs");
    assert_eq!(doc.structural_used_by.len(), 1);
    assert_eq!(doc.structural_used_by[0].source_file, "src/c.rs");
}

#[test]
fn inspect_docs_under_subtree_only() {
    let (dir, repo, store) = temp_repo();
    write(dir.path(), "docs/adr/0001.md",     "---\ntitle: ADR One\n---\n");
    write(dir.path(), "docs/decisions/d.md",  "---\ntitle: Decision\n---\n");
    write(dir.path(), "src/foo.rs",           "");
    // Ingest the docs the same way `atlas ingest` would.
    atlas_core::ingest_documents(&repo, &store).unwrap();

    let doc = inspect("docs", &repo, &store).unwrap();
    let paths: Vec<&str> = doc.documents.iter().map(|d| d.file_path.as_str()).collect();
    assert!(paths.iter().any(|p| p == &"docs/adr/0001.md"),    "ADR must appear");
    assert!(paths.iter().any(|p| p == &"docs/decisions/d.md"), "decision must appear");
    assert!(paths.iter().all(|p| p.starts_with("docs/")),      "no docs outside subject");

    let src_doc = inspect("src", &repo, &store).unwrap();
    assert!(src_doc.documents.is_empty(),
        "src/ subtree contains no ingested docs — literal containment only");
}

#[test]
fn inspect_nonexistent_path_returns_document_with_exists_false() {
    let (_dir, repo, store) = temp_repo();
    let doc = inspect("does/not/exist", &repo, &store).unwrap();

    assert!(!doc.exists_on_disk);
    assert_eq!(doc.kind, InspectionSubjectKind::Directory,
        "nonexistent paths default to Directory for aggregation-as-prefix");
    assert_eq!(doc.touch_count, 0);
    assert!(doc.children.is_empty());
    assert!(doc.recent_activity.is_empty());
    // Not an error.
}

#[test]
fn inspect_module_matches_profile_claim() {
    let (dir, repo, store) = temp_repo();
    // Minimal package.json so inspect_repository produces claims.
    write(dir.path(), "package.json", r#"{
        "name": "app",
        "dependencies": { "express": "^4.0.0" }
    }"#);
    mkdir(dir.path(), "src/identity");
    write(dir.path(), "src/identity/service.ts", "export {}");

    let doc = inspect("src/identity", &repo, &store).unwrap();
    // Ambient claims present.
    assert!(doc.profile_claims.iter().any(|c|
        matches!(c.kind, ProfileClaimKind::Runtime) && c.value == "Node.js"),
        "ambient Runtime claim must always be present when detectable");
    // Module claim scoped to the subject.
    assert!(doc.profile_claims.iter().any(|c|
        matches!(c.kind, ProfileClaimKind::Module) && c.value == "identity"),
        "Module: identity claim must appear for src/identity subject");
}

#[test]
fn inspect_repo_path_isolation() {
    let (dir_a, repo_a, store) = temp_repo();
    let dir_b = TempDir::new().unwrap();
    let repo_b = dir_b.path().to_string_lossy().into_owned();

    write(dir_a.path(), "src/x.rs", "");
    write(dir_b.path(), "src/y.rs", "");
    commit(&store, &repo_a, "aaaa1111", "A commit", 1, &["src/x.rs"]);
    commit(&store, &repo_b, "bbbb2222", "B commit", 2, &["src/y.rs"]);

    let doc_a = inspect("src", &repo_a, &store).unwrap();
    assert_eq!(doc_a.touch_count, 1);
    assert_eq!(doc_a.recent_activity[0].short_hash, "aaaa111");

    let doc_b = inspect("src", &repo_b, &store).unwrap();
    assert_eq!(doc_b.touch_count, 1);
    assert_eq!(doc_b.recent_activity[0].short_hash, "bbbb222");
}
