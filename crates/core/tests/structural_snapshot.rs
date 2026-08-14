//! Regression tests for `structural_edges` snapshot semantics.
//!
//! Contract under test (see `structural_edges` schema comment in
//! `crates/storage/src/lib.rs`):
//!
//!   1. Each ingest run STARTS by clearing the table for this repo.
//!   2. Edges reflect the CURRENT working tree only.  When a file is
//!      renamed and then re-ingested, its edges disappear from the old
//!      path and reappear under the new path — not the moment of the
//!      rename, but on the next ingest.
//!   3. Historical structural queries ("what did file X import before
//!      commit Y renamed it?") are NOT answerable from this table.  That
//!      is a deliberate architectural choice — longitudinal identity
//!      lives in `file_identities`, the structural graph is snapshot-only.
//!
//! These tests exist so that any change breaking the invariant surfaces
//! immediately, not months later when a downstream query silently returns
//! stale rows.

use atlas_core::ingest_typescript;
use atlas_storage::Store;
use std::path::Path;
use tempfile::TempDir;

fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn temp_repo() -> (TempDir, String, Store) {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_string_lossy().into_owned();
    let db = dir.path().join("atlas.db");
    let store = Store::open(db.to_str().unwrap()).unwrap();
    (dir, repo, store)
}

#[test]
fn edges_reflect_the_current_working_tree_only() {
    let (dir, repo, store) = temp_repo();
    write(dir.path(), "src/helper.ts",  "export const H = 1;\n");
    write(dir.path(), "src/service.ts", "import { H } from './helper';\nexport const S = H;\n");

    ingest_typescript(&repo, &store).unwrap();

    let outgoing = store.structural_edges_for_file("src/service.ts", &repo).unwrap();
    assert!(!outgoing.is_empty(), "expected an edge from service.ts to helper.ts");
    assert!(outgoing.iter().any(|e| e.target_file == "src/helper.ts"),
        "target must be the current path of helper.ts");
}

#[test]
fn rename_without_reingest_leaves_stale_edges_under_old_path() {
    // This test PROVES the snapshot contract: until the next ingest, the
    // table still shows edges under the OLD path.  If a future change
    // silently moved edges on rename, this test would flag it.
    let (dir, repo, store) = temp_repo();
    write(dir.path(), "src/helper.ts",  "export const H = 1;\n");
    write(dir.path(), "src/service.ts", "import { H } from './helper';\nexport const S = H;\n");

    ingest_typescript(&repo, &store).unwrap();
    // Rename on disk WITHOUT re-ingesting.  Simulates the state between a
    // rename commit and the next `atlas ingest`.
    std::fs::rename(dir.path().join("src/service.ts"),
                    dir.path().join("src/renamed.ts")).unwrap();

    // Edges still live under `src/service.ts` — the snapshot has not been
    // refreshed.  This is CORRECT behaviour per the contract.
    let by_old = store.structural_edges_for_file("src/service.ts", &repo).unwrap();
    let by_new = store.structural_edges_for_file("src/renamed.ts", &repo).unwrap();
    assert!(!by_old.is_empty(), "old path must still carry edges before re-ingest");
    assert!(by_new.is_empty(),   "new path must have no edges until the next ingest");
}

#[test]
fn reingest_migrates_edges_to_new_path_and_drops_old() {
    let (dir, repo, store) = temp_repo();
    write(dir.path(), "src/helper.ts",  "export const H = 1;\n");
    write(dir.path(), "src/service.ts", "import { H } from './helper';\nexport const S = H;\n");

    ingest_typescript(&repo, &store).unwrap();
    // Rename and re-parse the source so the import target resolves correctly
    // in the new state, then re-ingest.
    std::fs::rename(dir.path().join("src/service.ts"),
                    dir.path().join("src/renamed.ts")).unwrap();
    write(dir.path(), "src/renamed.ts",
          "import { H } from './helper';\nexport const S = H;\n");

    ingest_typescript(&repo, &store).unwrap();

    let by_old = store.structural_edges_for_file("src/service.ts", &repo).unwrap();
    let by_new = store.structural_edges_for_file("src/renamed.ts", &repo).unwrap();

    assert!(by_old.is_empty(),
        "post-ingest, old path must have zero edges — the snapshot was refreshed");
    assert!(!by_new.is_empty(),
        "post-ingest, new path must carry the migrated edges");
    assert!(by_new.iter().any(|e| e.target_file == "src/helper.ts"),
        "target remains helper.ts (path unchanged)");
}

#[test]
fn edges_do_not_accumulate_across_ingest_runs() {
    // Two identical ingests must produce the same edge count.  If
    // `clear_structural_edges` were skipped, edges would accumulate.
    let (dir, repo, store) = temp_repo();
    write(dir.path(), "src/helper.ts",  "export const H = 1;\n");
    write(dir.path(), "src/service.ts", "import { H } from './helper';\n");

    ingest_typescript(&repo, &store).unwrap();
    let count_after_first = store.structural_edge_count(&repo).unwrap();

    ingest_typescript(&repo, &store).unwrap();
    let count_after_second = store.structural_edge_count(&repo).unwrap();

    assert_eq!(count_after_first, count_after_second,
        "identical ingests must not accumulate edges — the table is a snapshot");
}

#[test]
fn deleting_a_source_file_removes_its_edges_on_next_ingest() {
    // If a file disappears from the tree, its edges must disappear on the
    // next ingest.  This is the flip side of the snapshot semantic.
    let (dir, repo, store) = temp_repo();
    write(dir.path(), "src/helper.ts",  "export const H = 1;\n");
    write(dir.path(), "src/service.ts", "import { H } from './helper';\n");

    ingest_typescript(&repo, &store).unwrap();
    assert!(!store.structural_edges_for_file("src/service.ts", &repo).unwrap().is_empty());

    std::fs::remove_file(dir.path().join("src/service.ts")).unwrap();
    ingest_typescript(&repo, &store).unwrap();

    let after = store.structural_edges_for_file("src/service.ts", &repo).unwrap();
    assert!(after.is_empty(),
        "a deleted file must have no edges after the next ingest — snapshot is authoritative");
}
