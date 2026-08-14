//! Regression tests for FileIdentity propagation into hot-files, inspect,
//! and investigate.
//!
//! Uses a real git repo built with `git init` + `git mv` (so rename evidence
//! and file identity chains materialise the same way they do on RWATP).
//!
//! Invariant under test:
//!   No Atlas read command silently loses longitudinal file history merely
//!   because a file changed path.

use atlas_core::{
    ingest_git, ingest_rename_evidence, inspect, investigate, rebuild_file_identities,
};
use atlas_storage::Store;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn sh(cwd: &str, args: &[&str]) {
    sh_at(cwd, args, "2026-01-01T00:00:00Z")
}

fn sh_at(cwd: &str, args: &[&str], iso_ts: &str) {
    let out = Command::new(args[0]).args(&args[1..])
        .current_dir(cwd)
        .env("GIT_AUTHOR_NAME", "T").env("GIT_AUTHOR_EMAIL", "t@x")
        .env("GIT_COMMITTER_NAME", "T").env("GIT_COMMITTER_EMAIL", "t@x")
        .env("GIT_COMMITTER_DATE", iso_ts)
        .env("GIT_AUTHOR_DATE",    iso_ts)
        .output().expect("cmd");
    assert!(out.status.success(),
        "{:?} failed: stdout={} stderr={}",
        args, String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
}

fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// Build a fixture with one file renamed via `git mv`.
/// Layout after fixture:
///   old_path: src/legacy.ts (renamed away)
///   new_path: src/service.ts (current)
/// Both touched by 2 commits in total (before + rename+modify).
struct RenameFixture {
    _dir: TempDir,
    repo: String,
    store: Store,
    old_path: &'static str,
    new_path: &'static str,
}

fn make_rename_fixture() -> RenameFixture {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().to_string_lossy().into_owned();

    sh(&repo, &["git", "init", "-q", "-b", "main"]);
    sh(&repo, &["git", "config", "user.email", "t@x"]);
    sh(&repo, &["git", "config", "user.name",  "T"]);
    // Ensure git detects renames even under conservative defaults.
    sh(&repo, &["git", "config", "diff.renames", "true"]);

    // Commit 1: create old path with enough content that git's rename
    // detector has real signal to work with (a single-line file is easy
    // to misclassify as a delete+add).
    let content_v1 = "\
export const answer = 42;
export function compute() {
    return answer;
}
export const helpers = {
    add: (a: number, b: number) => a + b,
    sub: (a: number, b: number) => a - b,
};
";
    write(dir.path(), "src/legacy.ts", content_v1);
    sh_at(&repo, &["git", "add", "-A"], "2026-01-01T00:00:00Z");
    sh_at(&repo, &["git", "commit", "-q", "-m", "initial: add legacy"],
          "2026-01-01T00:00:00Z");

    // Commit 2: rename + minor modify (small enough to stay above -M50).
    let content_v2 = "\
export const answer = 43;
export function compute() {
    return answer;
}
export const helpers = {
    add: (a: number, b: number) => a + b,
    sub: (a: number, b: number) => a - b,
};
";
    std::fs::remove_file(dir.path().join("src/legacy.ts")).unwrap();
    write(dir.path(), "src/service.ts", content_v2);
    // Distinct timestamp so populate_identity_commits's temporal window
    // `[c_intro, c_super)` cleanly includes the create commit.
    sh_at(&repo, &["git", "add", "-A"], "2026-01-02T00:00:00Z");
    sh_at(&repo, &["git", "commit", "-q", "-m", "rename legacy → service"],
          "2026-01-02T00:00:00Z");

    let db = dir.path().join("atlas.db");
    let store = Store::open(db.to_str().unwrap()).unwrap();
    ingest_git(&repo, &store).unwrap();
    ingest_rename_evidence(&repo, &store).unwrap();
    rebuild_file_identities(&repo, &store).unwrap();

    RenameFixture {
        _dir: dir,
        repo,
        store,
        old_path: "src/legacy.ts",
        new_path: "src/service.ts",
    }
}

// ── hot-files ────────────────────────────────────────────────────────────────

#[test]
fn hot_files_identity_aware_collapses_renamed_file_to_one_row() {
    // Without identity-aware aggregation, `src/legacy.ts` and `src/service.ts`
    // would appear as two files with 1 touch each.  With identity aggregation
    // they must appear as ONE row keyed on the current path with 2 touches.
    let fx = make_rename_fixture();

    // Sanity: the fixture actually produced identity state.  Without these
    // the failure mode of "identity chain never materialised" is misleading.
    assert!(fx.store.has_materialized_identities(&fx.repo).unwrap(),
        "fixture precondition: rename must produce an identity chain");

    let rows = fx.store.hot_files_identity_aware(&fx.repo, 10).unwrap();

    let for_service: Vec<_> = rows.iter().filter(|r| r.file_path == fx.new_path).collect();
    let for_legacy:  Vec<_> = rows.iter().filter(|r| r.file_path == fx.old_path).collect();

    assert_eq!(for_service.len(), 1, "current path must appear exactly once");
    assert_eq!(for_service[0].touch_count, 2,
        "count must span the full identity (create + rename+modify), not just post-rename touches");
    assert!(for_legacy.is_empty(),
        "old path must NOT appear as its own row — it has been superseded");
}

#[test]
fn hot_files_path_scoped_does_not_aggregate() {
    // Path-scoped fallback (what a user sees when has_materialized_identities=false)
    // still returns two rows.  This test guards against accidental behaviour drift
    // in the path-scoped implementation.
    let fx = make_rename_fixture();
    let rows = fx.store.hot_files(&fx.repo, 10).unwrap();

    let for_service = rows.iter().find(|r| r.file_path == fx.new_path);
    let for_legacy  = rows.iter().find(|r| r.file_path == fx.old_path);
    assert!(for_service.is_some(), "path-scoped hot_files should list current path");
    assert!(for_legacy.is_some(),
        "path-scoped hot_files should still list the old path — that IS the path-scoped semantic");
}

// ── inspect: file subject redirect ────────────────────────────────────────────

#[test]
fn inspect_file_subject_redirects_historical_path_and_notes_redirect() {
    let fx = make_rename_fixture();

    // User queries the historical address.
    let doc = inspect(fx.old_path, &fx.repo, &fx.store).unwrap();

    let redirect = doc.historical_redirect.as_ref()
        .expect("inspect must record a historical_redirect for a renamed file");
    assert_eq!(redirect.original_subject, fx.old_path);
    assert_eq!(redirect.current_path,     fx.new_path);
    assert!(redirect.identity_id > 0);

    // The relative_path Atlas ACTUALLY inspected is the current path.
    assert_eq!(doc.relative_path, fx.new_path,
        "inspect must query under the current path, not silently return zero results for the old path");
    // touch_count now reflects the identity's full history (via build_context).
    assert!(doc.touch_count >= 2,
        "touch_count must reflect the identity's full history, got {}", doc.touch_count);
}

#[test]
fn inspect_current_path_does_not_trigger_redirect() {
    let fx = make_rename_fixture();
    let doc = inspect(fx.new_path, &fx.repo, &fx.store).unwrap();
    assert!(doc.historical_redirect.is_none(),
        "current path must not produce a redirect record");
}

#[test]
fn inspect_directory_subject_does_not_populate_redirect() {
    let fx = make_rename_fixture();
    let doc = inspect("src", &fx.repo, &fx.store).unwrap();
    assert!(doc.historical_redirect.is_none(),
        "directory subject must NEVER carry a historical_redirect \
         — Atlas has no directory identity");
}

// ── investigate: per-anchor redirect ──────────────────────────────────────────

#[test]
fn investigate_records_historical_anchor_redirect_and_preserves_original() {
    let fx = make_rename_fixture();

    let anchors = vec![fx.old_path];
    let doc = investigate(&anchors, &fx.repo, &fx.store).unwrap();

    // Original anchor preserved verbatim.
    assert_eq!(doc.anchors, vec![fx.old_path.to_string()],
        "original user anchor must be preserved");

    // Current path added to effective_anchors.
    assert!(doc.effective_anchors.iter().any(|a| a == fx.new_path),
        "current canonical path must appear in effective_anchors; got {:?}", doc.effective_anchors);

    // Redirect explicitly recorded.
    let redirects = &doc.anchor_redirects;
    assert_eq!(redirects.len(), 1, "one anchor was historical, one redirect expected");
    assert_eq!(redirects[0].original_anchor, fx.old_path);
    assert_eq!(redirects[0].current_path,    fx.new_path);
    assert!(redirects[0].identity_id > 0);
}

#[test]
fn investigate_non_path_anchor_produces_no_redirect() {
    let fx = make_rename_fixture();
    let anchors = vec!["order"];
    let doc = investigate(&anchors, &fx.repo, &fx.store).unwrap();
    assert!(doc.anchor_redirects.is_empty(),
        "a concept anchor must never produce a redirect");
}

#[test]
fn investigate_current_path_anchor_produces_no_redirect() {
    let fx = make_rename_fixture();
    let anchors = vec![fx.new_path];
    let doc = investigate(&anchors, &fx.repo, &fx.store).unwrap();
    assert!(doc.anchor_redirects.is_empty(),
        "a current-path anchor must not produce a redirect");
}
