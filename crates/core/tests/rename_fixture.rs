/// v0.4 Historical Identity — synthetic rename fixtures
///
/// This file exists to establish the expected Atlas behavior BEFORE any schema design.
/// It covers two fixture families:
///
/// ## Rename chain fixture (A–F): continuity across multiple moves
///
///   A — create auth.rs
///   B — modify auth.rs (extend with new functions)
///   C — rename auth.rs → security/auth.rs, no content change (~100% similarity)
///   D — modify security/auth.rs (small extension)
///   E — rename+modify security/auth.rs → crates/auth/src/lib.rs (~75% similarity)
///   F — modify crates/auth/src/lib.rs
///
/// ## Path-reuse fixture (X–Z): temporal ambiguity at the same path
///
///   X — create service.rs                 (belongs to artifact S1)
///   Y — rename service.rs → legacy/service.rs   (S1 moves, path is now free)
///   Z — create new service.rs             (belongs to artifact S2, different from S1)
///
///   The schema must represent that `(repo, "service.rs")` resolves to TWO distinct
///   identities depending on which point in time is queried. A simple path→identity
///   table without temporal bounds cannot satisfy this.
///
/// Tests prefixed `baseline_` compile and pass now. They document the current
/// path-scoped behavior and make the gap visible.
///
/// Tests prefixed `v04_` are #[ignore]. They document the acceptance criterion.
/// When v0.4 is complete, remove the `todo!()` bodies and activate the assertions.

use atlas_core::{build_context, ingest_git, ingest_rename_evidence, rebuild_file_identities};
use atlas_storage::Store;
use std::process::Command;
use tempfile::TempDir;

// ── Fixture builder ───────────────────────────────────────────────────────────

struct RenameFixture {
    _dir:       TempDir,
    pub path:   String,
    pub hash_a: String,
    pub hash_b: String,
    #[allow(dead_code)] // used in v0.4 #[ignore] test assertions
    pub hash_c: String,
    pub hash_d: String,
    pub hash_e: String,
    pub hash_f: String,
}

fn create_rename_fixture() -> RenameFixture {
    let dir  = tempfile::tempdir().expect("tempdir");
    let repo = dir.path().to_str().unwrap().to_string();

    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(&repo)
            .output()
            .expect("git");
        assert!(
            out.status.success(),
            "git {:?} failed:\n{}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    };

    let commit = |path: &str, msg: &str, date: &str| {
        let out = Command::new("git")
            .args(["commit", "-m", msg])
            .current_dir(path)
            .env("GIT_AUTHOR_DATE",    date)
            .env("GIT_COMMITTER_DATE", date)
            .output()
            .expect("git commit");
        assert!(
            out.status.success(),
            "commit '{}' failed:\n{}",
            msg,
            String::from_utf8_lossy(&out.stderr)
        );
    };

    let head = |path: &str| -> String {
        let out = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(path)
            .output()
            .expect("rev-parse");
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    };

    git(&["init", "-b", "main"]);
    git(&["config", "user.email", "fixture@test.com"]);
    git(&["config", "user.name", "Fixture"]);

    // ── Commit A: create src/auth.rs ──────────────────────────────────────────
    std::fs::write(format!("{repo}/auth.rs"), AUTH_V1).unwrap();
    git(&["add", "auth.rs"]);
    commit(&repo, "Add authentication module", "2024-01-01T10:00:00+0000");
    let hash_a = head(&repo);

    // ── Commit B: extend src/auth.rs ──────────────────────────────────────────
    std::fs::write(format!("{repo}/auth.rs"), AUTH_V2).unwrap();
    git(&["add", "auth.rs"]);
    commit(&repo, "Extend auth: add refresh_token and is_admin", "2024-01-02T10:00:00+0000");
    let hash_b = head(&repo);

    // ── Commit C: rename src/auth.rs → src/security/auth.rs (no content change)
    std::fs::create_dir_all(format!("{repo}/security")).unwrap();
    git(&["mv", "auth.rs", "security/auth.rs"]);
    commit(&repo, "Move auth module into security package", "2024-01-03T10:00:00+0000");
    let hash_c = head(&repo);

    // ── Commit D: extend src/security/auth.rs (small change) ─────────────────
    std::fs::write(format!("{repo}/security/auth.rs"), AUTH_V3).unwrap();
    git(&["add", "security/auth.rs"]);
    commit(&repo, "Add get_role helper to auth module", "2024-01-04T10:00:00+0000");
    let hash_d = head(&repo);

    // ── Commit E: rename+modify into crates/auth/src/lib.rs (~75% similarity) ─
    // The move is real (git mv), plus ~25% of function bodies are rewritten.
    std::fs::create_dir_all(format!("{repo}/crates/auth/src")).unwrap();
    git(&["mv", "security/auth.rs", "crates/auth/src/lib.rs"]);
    std::fs::write(format!("{repo}/crates/auth/src/lib.rs"), AUTH_V4).unwrap();
    git(&["add", "crates/auth/src/lib.rs"]);
    commit(&repo, "Promote auth to standalone crate, harden validation", "2024-01-05T10:00:00+0000");
    let hash_e = head(&repo);

    // ── Commit F: extend crates/auth/src/lib.rs ───────────────────────────────
    std::fs::write(format!("{repo}/crates/auth/src/lib.rs"), AUTH_V5).unwrap();
    git(&["add", "crates/auth/src/lib.rs"]);
    commit(&repo, "Add has_permission to auth crate", "2024-01-06T10:00:00+0000");
    let hash_f = head(&repo);

    RenameFixture { _dir: dir, path: repo, hash_a, hash_b, hash_c, hash_d, hash_e, hash_f }
}

// ── File versions ─────────────────────────────────────────────────────────────

// Commit A: initial auth module (~19 lines)
const AUTH_V1: &str = "\
pub fn authenticate(username: &str, password: &str) -> bool {
    !username.is_empty() && !password.is_empty()
}

pub fn logout(username: &str) {
    let _ = username;
}

pub fn validate_session(token: &str) -> bool {
    token.len() > 8
}

pub fn get_user_id(username: &str) -> u64 {
    username.len() as u64
}

pub fn is_locked(username: &str) -> bool {
    username.starts_with('_')
}
";

// Commit B: adds refresh_token and is_admin (~26 lines)
const AUTH_V2: &str = "\
pub fn authenticate(username: &str, password: &str) -> bool {
    !username.is_empty() && !password.is_empty()
}

pub fn logout(username: &str) {
    let _ = username;
}

pub fn validate_session(token: &str) -> bool {
    token.len() > 8
}

pub fn get_user_id(username: &str) -> u64 {
    username.len() as u64
}

pub fn is_locked(username: &str) -> bool {
    username.starts_with('_')
}

pub fn refresh_token(token: &str) -> String {
    format!(\"refreshed:{token}\")
}

pub fn is_admin(username: &str) -> bool {
    username == \"admin\"
}
";

// Commit D: same as V2 plus get_role (~31 lines)
// (Commit C is a pure rename of V2; D extends it)
const AUTH_V3: &str = "\
pub fn authenticate(username: &str, password: &str) -> bool {
    !username.is_empty() && !password.is_empty()
}

pub fn logout(username: &str) {
    let _ = username;
}

pub fn validate_session(token: &str) -> bool {
    token.len() > 8
}

pub fn get_user_id(username: &str) -> u64 {
    username.len() as u64
}

pub fn is_locked(username: &str) -> bool {
    username.starts_with('_')
}

pub fn refresh_token(token: &str) -> String {
    format!(\"refreshed:{token}\")
}

pub fn is_admin(username: &str) -> bool {
    username == \"admin\"
}

pub fn get_role(username: &str) -> &'static str {
    if username == \"admin\" { \"admin\" } else { \"user\" }
}
";

// Commit E: move + rewrite ~6 lines (~75% similarity with V3)
// Changed: authenticate body, logout body, validate_session body,
//          refresh_token body, is_admin body, get_role body.
// Unchanged: all 6 function signatures + get_user_id + is_locked (~25 lines).
const AUTH_V4: &str = "\
pub fn authenticate(username: &str, password: &str) -> bool {
    username.len() >= 3 && password.len() >= 8
}

pub fn logout(username: &str) {
    drop(username);
}

pub fn validate_session(token: &str) -> bool {
    token.starts_with(\"sess_\")
}

pub fn get_user_id(username: &str) -> u64 {
    username.len() as u64
}

pub fn is_locked(username: &str) -> bool {
    username.starts_with('_')
}

pub fn refresh_token(token: &str) -> String {
    format!(\"new_{token}\")
}

pub fn is_admin(username: &str) -> bool {
    username == \"root\" || username == \"admin\"
}

pub fn get_role(username: &str) -> &'static str {
    match username {
        \"admin\" | \"root\" => \"admin\",
        _ => \"user\",
    }
}
";

// Commit F: adds has_permission (~37 lines)
const AUTH_V5: &str = "\
pub fn authenticate(username: &str, password: &str) -> bool {
    username.len() >= 3 && password.len() >= 8
}

pub fn logout(username: &str) {
    drop(username);
}

pub fn validate_session(token: &str) -> bool {
    token.starts_with(\"sess_\")
}

pub fn get_user_id(username: &str) -> u64 {
    username.len() as u64
}

pub fn is_locked(username: &str) -> bool {
    username.starts_with('_')
}

pub fn refresh_token(token: &str) -> String {
    format!(\"new_{token}\")
}

pub fn is_admin(username: &str) -> bool {
    username == \"root\" || username == \"admin\"
}

pub fn get_role(username: &str) -> &'static str {
    match username {
        \"admin\" | \"root\" => \"admin\",
        _ => \"user\",
    }
}

pub fn has_permission(username: &str, action: &str) -> bool {
    is_admin(username) || action == \"read\"
}
";

// ── Baseline tests (pass now) ─────────────────────────────────────────────────
//
// These tests document the current path-scoped behavior and make the gap visible.
// Each assertion answers: "what does Atlas see when queried at this exact path?"

#[test]
fn baseline_ingest_produces_six_commits() {
    let f     = create_rename_fixture();
    let store = Store::open(":memory:").unwrap();

    let count = ingest_git(&f.path, &store).unwrap();
    assert_eq!(count, 6, "fixture must produce exactly 6 commits");
}

#[test]
fn baseline_current_path_sees_only_recent_history() {
    // crates/auth/src/lib.rs was created (via rename) in commit E.
    // Current Atlas only sees E and F for this path — the pre-rename history
    // from commits A–D (which existed under src/auth.rs / src/security/auth.rs)
    // is invisible from the current path.
    let f     = create_rename_fixture();
    let store = Store::open(":memory:").unwrap();
    ingest_git(&f.path, &store).unwrap();

    let commits = store.commits_for_file("crates/auth/src/lib.rs", &f.path).unwrap();
    let hashes: Vec<&str> = commits.iter().map(|c| c.short_hash.as_str()).collect();

    // Commits A and B are definitely NOT visible from the current path —
    // this is the identity gap: the artifact's original history is lost.
    let short_a = &f.hash_a[..7];
    let short_b = &f.hash_b[..7];
    assert!(
        !hashes.contains(&short_a),
        "commit A must NOT be visible from crates/auth/src/lib.rs (path-scoped gap):\n{hashes:?}"
    );
    assert!(
        !hashes.contains(&short_b),
        "commit B must NOT be visible from crates/auth/src/lib.rs (path-scoped gap):\n{hashes:?}"
    );
}

#[test]
fn baseline_original_path_sees_only_early_history() {
    // src/auth.rs only exists in commits A and B.
    // After commit C renames it, the old path sees no further changes.
    let f     = create_rename_fixture();
    let store = Store::open(":memory:").unwrap();
    ingest_git(&f.path, &store).unwrap();

    let commits = store.commits_for_file("auth.rs", &f.path).unwrap();
    let hashes: Vec<&str> = commits.iter().map(|c| c.short_hash.as_str()).collect();

    // Commits D, E, F are definitely not visible from auth.rs.
    for (label, hash) in [("D", &f.hash_d), ("E", &f.hash_e), ("F", &f.hash_f)] {
        let short = &hash[..7];
        assert!(
            !hashes.contains(&short),
            "commit {label} must NOT be visible from auth.rs (path-scoped gap):\n{hashes:?}"
        );
    }
}

#[test]
fn baseline_history_is_fragmented_across_paths() {
    // The same engineering artifact has history fragmented across three paths.
    // No single path shows all 6 commits; the union of path-scoped queries
    // cannot easily reconstruct the artifact's true history.
    let f     = create_rename_fixture();
    let store = Store::open(":memory:").unwrap();
    ingest_git(&f.path, &store).unwrap();

    let path1 = store.commits_for_file("auth.rs",                  &f.path).unwrap();
    let path2 = store.commits_for_file("security/auth.rs",         &f.path).unwrap();
    let path3 = store.commits_for_file("crates/auth/src/lib.rs",   &f.path).unwrap();

    // No single path-scoped view has all 6 commits.
    assert!(
        path1.len() < 6,
        "auth.rs should not see all 6 commits in path-scoped mode (saw {})",
        path1.len()
    );
    assert!(
        path3.len() < 6,
        "crates/auth/src/lib.rs should not see all 6 commits in path-scoped mode (saw {})",
        path3.len()
    );

    // Commits A+B exist under path1, E+F exist under path3 — they cannot overlap.
    let short_a = &f.hash_a[..7];
    let short_f = &f.hash_f[..7];
    let path1_has_a = path1.iter().any(|c| c.short_hash == short_a);
    let path3_has_f = path3.iter().any(|c| c.short_hash == short_f);
    let _path2_len  = path2.len(); // retained for diagnostic value

    assert!(path1_has_a, "auth.rs must contain commit A");
    assert!(path3_has_f, "crates/auth/src/lib.rs must contain commit F");
}

#[test]
fn baseline_context_current_path_missing_origin_commit() {
    // build_context for the current path returns correct facts for what it can see,
    // but first_commit is not commit A — it's the rename/move commit, not the origin.
    // This is the epistemic gap: "introduced" is wrong because it's path-scoped.
    let f     = create_rename_fixture();
    let store = Store::open(":memory:").unwrap();
    ingest_git(&f.path, &store).unwrap();

    let doc = build_context("crates/auth/src/lib.rs", &f.path, &store).unwrap();

    // The first commit visible from the current path is NOT commit A (the true origin).
    // It's at best commit E (when this path first appeared).
    let first_hash = doc.identity.first_commit
        .as_ref()
        .map(|c| c.short_hash.as_str())
        .unwrap_or("");

    assert_ne!(
        first_hash, &f.hash_a[..7],
        "path-scoped query must NOT return commit A as first_commit for crates/auth/src/lib.rs"
    );

    // Coverage correctly reports the limitation.
    assert_eq!(
        doc.coverage.git_history,
        atlas_ir::CoverageStatus::PathScoped,
        "git_history coverage must be PathScoped to honestly report the gap"
    );
    assert_eq!(
        doc.coverage.rename_tracking,
        atlas_ir::CoverageStatus::NotIngested,
        "rename_tracking must be NotIngested until v0.4"
    );
}

// ── Evidence pipeline integration tests ─────────────────────────────────────
//
// These tests prove the full pipeline: git → parse → store → query.
// They pass now and must keep passing throughout v0.4 development.

#[test]
fn evidence_pipeline_captures_both_rename_commits() {
    // The A-F fixture has two rename events:
    //   Commit C: auth.rs → security/auth.rs  (pure rename, 100% similarity)
    //   Commit E: security/auth.rs → crates/auth/src/lib.rs  (~75% similarity)
    // Both must appear in rename_evidence after ingest_rename_evidence.
    let f     = create_rename_fixture();
    let store = Store::open(":memory:").unwrap();
    ingest_git(&f.path, &store).unwrap();

    let count = ingest_rename_evidence(&f.path, &store).unwrap();
    assert!(count >= 2, "expected at least 2 rename records from A-F fixture, got {count}");

    let all = store.all_rename_evidence(&f.path).unwrap();

    let commit_c = all.iter().find(|r| r.new_path == "security/auth.rs");
    assert!(commit_c.is_some(), "rename C (auth.rs → security/auth.rs) missing from evidence");
    let c = commit_c.unwrap();
    assert_eq!(c.old_path,         "auth.rs");
    assert_eq!(c.similarity_score, 100, "pure rename must be 100% similarity");
    assert_eq!(c.detection_source, "git-rename");

    let commit_e = all.iter().find(|r| r.new_path == "crates/auth/src/lib.rs");
    assert!(commit_e.is_some(), "rename E (security/auth.rs → crates/auth/src/lib.rs) missing");
    let e = commit_e.unwrap();
    assert_eq!(e.old_path, "security/auth.rs");
    assert!(
        e.similarity_score >= 60 && e.similarity_score <= 85,
        "rename+modify commit E should have similarity 60–85% (got {})", e.similarity_score
    );
}

#[test]
fn evidence_pipeline_is_idempotent() {
    let f     = create_rename_fixture();
    let store = Store::open(":memory:").unwrap();
    ingest_git(&f.path, &store).unwrap();
    ingest_rename_evidence(&f.path, &store).unwrap();
    ingest_rename_evidence(&f.path, &store).unwrap();

    let all     = store.all_rename_evidence(&f.path).unwrap();
    let c_count = all.iter().filter(|r| r.new_path == "security/auth.rs").count();
    assert_eq!(c_count, 1, "duplicate ingest must not duplicate rename_evidence rows");
}

#[test]
fn evidence_pipeline_path_query_links_chain() {
    // Querying security/auth.rs returns BOTH the incoming rename (C) and the
    // outgoing rename (E) — the intermediate path is connected on both sides.
    let f     = create_rename_fixture();
    let store = Store::open(":memory:").unwrap();
    ingest_git(&f.path, &store).unwrap();
    ingest_rename_evidence(&f.path, &store).unwrap();

    let rows = store.rename_evidence_for_path("security/auth.rs", &f.path).unwrap();
    assert_eq!(rows.len(), 2,
        "security/auth.rs must appear in exactly 2 rename records (one in, one out)");
}

#[test]
fn evidence_pipeline_path_reuse_captures_rename_in_y() {
    // In the path-reuse fixture, commit Y renames service.rs → legacy/service.rs.
    // This rename must appear in rename_evidence.
    let f     = create_path_reuse_fixture();
    let store = Store::open(":memory:").unwrap();
    ingest_git(&f.path, &store).unwrap();
    ingest_rename_evidence(&f.path, &store).unwrap();

    let all = store.all_rename_evidence(&f.path).unwrap();
    let rename_y = all.iter().find(|r| r.new_path == "legacy/service.rs");
    assert!(rename_y.is_some(), "rename Y (service.rs → legacy/service.rs) must be in evidence");
    let r = rename_y.unwrap();
    assert_eq!(r.old_path,         "service.rs");
    assert_eq!(r.similarity_score, 100, "pure git mv must be 100% similarity");
}

// ── Phase 3 identity materialization tests ────────────────────────────────────
//
// These tests prove the full resolution checkpoint:
//   P1 → F42,  P2 → F42,  P3 → F42   (rename chain)
//   service.rs@X → S1,  service.rs@Z → S2  (path reuse)

fn ingest_all(store: &Store, repo_path: &str) {
    ingest_git(repo_path, store).unwrap();
    ingest_rename_evidence(repo_path, store).unwrap();
    rebuild_file_identities(repo_path, store).unwrap();
}

#[test]
fn identity_chain_all_paths_map_to_same_identity() {
    let f     = create_rename_fixture();
    let store = Store::open(":memory:").unwrap();
    ingest_all(&store, &f.path);

    let id1 = store.resolve_path_to_identity("auth.rs",                &f.path).unwrap();
    let id2 = store.resolve_path_to_identity("security/auth.rs",       &f.path).unwrap();
    let id3 = store.resolve_path_to_identity("crates/auth/src/lib.rs", &f.path).unwrap();

    assert!(id1.is_some(), "auth.rs must resolve to an identity");
    assert!(id2.is_some(), "security/auth.rs must resolve to an identity");
    assert!(id3.is_some(), "crates/auth/src/lib.rs must resolve to an identity");
    assert_eq!(id1, id2, "auth.rs and security/auth.rs must share the same identity");
    assert_eq!(id2, id3, "security/auth.rs and crates/auth/src/lib.rs must share the same identity");
}

#[test]
fn identity_chain_path_history_ordered_correctly() {
    let f     = create_rename_fixture();
    let store = Store::open(":memory:").unwrap();
    ingest_all(&store, &f.path);

    let id = store.resolve_path_to_identity("crates/auth/src/lib.rs", &f.path)
        .unwrap()
        .expect("crates/auth/src/lib.rs must have an identity");

    let history = store.path_history_for_identity(id, &f.path).unwrap();
    assert_eq!(history.len(), 3, "F42 must have exactly 3 path observations (auth.rs → security/auth.rs → crates/auth/src/lib.rs)");

    assert_eq!(history[0].path, "auth.rs",                  "first path must be auth.rs");
    assert_eq!(history[1].path, "security/auth.rs",         "second path must be security/auth.rs");
    assert_eq!(history[2].path, "crates/auth/src/lib.rs",   "third path must be crates/auth/src/lib.rs (current)");

    // First two paths are superseded; the current path is not
    assert!(history[0].superseded_by_commit.is_some(), "auth.rs must be superseded");
    assert!(history[1].superseded_by_commit.is_some(), "security/auth.rs must be superseded");
    assert!(history[2].superseded_by_commit.is_none(),  "crates/auth/src/lib.rs must be current");
}

#[test]
fn identity_chain_current_path_resolves_to_chain_identity() {
    let f     = create_rename_fixture();
    let store = Store::open(":memory:").unwrap();
    ingest_all(&store, &f.path);

    let current = store.resolve_current_path("crates/auth/src/lib.rs", &f.path).unwrap();
    assert!(current.is_some(), "crates/auth/src/lib.rs must be current for some identity");

    // Historical paths are not current (superseded_by_commit IS NOT NULL)
    let auth_current = store.resolve_current_path("auth.rs", &f.path).unwrap();
    assert!(auth_current.is_none(), "auth.rs is a historical path — must not be current");
}

#[test]
fn identity_chain_resolve_path_at_commit_returns_correct_identity() {
    let f     = create_rename_fixture();
    let store = Store::open(":memory:").unwrap();
    ingest_all(&store, &f.path);

    let id_at_a = store.resolve_path_at("auth.rs", &f.hash_a, &f.path).unwrap();
    let id_at_e = store.resolve_path_at("crates/auth/src/lib.rs", &f.hash_e, &f.path).unwrap();

    assert!(id_at_a.is_some(), "auth.rs must be resolvable at commit A");
    assert!(id_at_e.is_some(), "crates/auth/src/lib.rs must be resolvable at commit E");
    assert_eq!(id_at_a, id_at_e, "both resolutions must return the same identity (F42)");
}

#[test]
fn identity_chain_rebuild_is_idempotent() {
    let f     = create_rename_fixture();
    let store = Store::open(":memory:").unwrap();
    ingest_git(&f.path, &store).unwrap();
    ingest_rename_evidence(&f.path, &store).unwrap();
    rebuild_file_identities(&f.path, &store).unwrap();
    rebuild_file_identities(&f.path, &store).unwrap();

    let id1 = store.resolve_path_to_identity("auth.rs", &f.path).unwrap();
    let id3 = store.resolve_path_to_identity("crates/auth/src/lib.rs", &f.path).unwrap();
    assert_eq!(id1, id3, "double rebuild must produce the same identity chain");
}

#[test]
fn path_reuse_two_distinct_identities_materialized() {
    let f     = create_path_reuse_fixture();
    let store = Store::open(":memory:").unwrap();
    ingest_all(&store, &f.path);

    let ids = store.identities_for_path("service.rs", &f.path).unwrap();
    assert_eq!(ids.len(), 2,
        "service.rs must resolve to 2 identities (S1 pre-rename, S2 post-reuse): got {:?}", ids);
}

#[test]
fn path_reuse_resolve_current_returns_s2() {
    let f     = create_path_reuse_fixture();
    let store = Store::open(":memory:").unwrap();
    ingest_all(&store, &f.path);

    // S1's service.rs is superseded (renamed to legacy/service.rs).
    // S2's service.rs is current (created after the rename, no further rename).
    let current = store.resolve_current_path("service.rs", &f.path)
        .unwrap()
        .expect("service.rs must have a current occupant (S2)");

    let history = store.path_history_for_identity(current, &f.path).unwrap();
    // S2 has only one path observation: service.rs (not superseded)
    assert_eq!(history.len(), 1,
        "current identity (S2) must have exactly 1 path observation");
    assert!(history[0].superseded_by_commit.is_none(),
        "S2's service.rs must not be superseded");
}

#[test]
fn path_reuse_resolve_at_x_returns_s1_and_at_z_returns_s2() {
    let f     = create_path_reuse_fixture();
    let store = Store::open(":memory:").unwrap();
    ingest_all(&store, &f.path);

    let id_at_x = store.resolve_path_at("service.rs", &f.hash_x, &f.path).unwrap();
    let id_at_z = store.resolve_path_at("service.rs", &f.hash_z, &f.path).unwrap();

    assert!(id_at_x.is_some(), "service.rs must be resolvable at commit X");
    assert!(id_at_z.is_some(), "service.rs must be resolvable at commit Z");
    assert_ne!(id_at_x, id_at_z,
        "service.rs at commit X (S1) and at commit Z (S2) must be different identities");
}

// ── Phase 4: identity-scoped commit queries ───────────────────────────────────

#[test]
fn path_reuse_commits_for_identity_boundary() {
    // Regression guard: proves commits_for_identity uses temporal bounds, not a
    // naive path join.  A naive "JOIN ON commit_files.file_path = path" would
    // assign commit Z to S1 (both X and Z touch "service.rs") and X to S2.
    // The temporal window prevents this: S1's window is [X_ts, Y_ts) which
    // excludes any commit at or after Y_ts.
    let f     = create_path_reuse_fixture();
    let store = Store::open(":memory:").unwrap();
    ingest_all(&store, &f.path);

    let s1 = store.resolve_path_at("service.rs", &f.hash_x, &f.path).unwrap()
        .expect("S1 must exist at commit X");
    let s2 = store.resolve_path_at("service.rs", &f.hash_z, &f.path).unwrap()
        .expect("S2 must exist at commit Z");

    let s1_commits = store.commits_for_identity(s1, &f.path).unwrap();
    let s2_commits = store.commits_for_identity(s2, &f.path).unwrap();
    let s1_hashes: Vec<&str> = s1_commits.iter().map(|c| c.hash.as_str()).collect();
    let s2_hashes: Vec<&str> = s2_commits.iter().map(|c| c.hash.as_str()).collect();

    assert!(s1_hashes.contains(&f.hash_x.as_str()), "S1 must contain commit X");
    assert!(!s1_hashes.contains(&f.hash_z.as_str()),
        "S1 must NOT contain commit Z — naive path join would incorrectly include it");
    assert!(s2_hashes.contains(&f.hash_z.as_str()), "S2 must contain commit Z");
    assert!(!s2_hashes.contains(&f.hash_x.as_str()),
        "S2 must NOT contain commit X — naive path join would incorrectly include it");
}

// ── v0.4 acceptance criteria (#[ignore]) ─────────────────────────────────────
//
// These tests document what v0.4 MUST achieve. They will not pass until:
//   1. Rename evidence is ingested via --diff-filter=R or --name-status
//   2. FileIdentity + FilePathObservation + IdentityEvidence tables exist
//   3. path→FileIdentity resolution handles both current and historical paths
//   4. Identity-scoped query variants exist alongside path-scoped ones
//
// To activate: remove `todo!()`, uncomment the assertions, run with --include-ignored.

#[test]
fn v04_all_three_paths_resolve_to_same_identity() {
    let f     = create_rename_fixture();
    let store = Store::open(":memory:").unwrap();
    ingest_all(&store, &f.path);

    let id1 = store.resolve_path_to_identity("auth.rs",                &f.path).unwrap();
    let id2 = store.resolve_path_to_identity("security/auth.rs",       &f.path).unwrap();
    let id3 = store.resolve_path_to_identity("crates/auth/src/lib.rs", &f.path).unwrap();
    assert_eq!(id1, id2, "original and intermediate path must share identity");
    assert_eq!(id2, id3, "intermediate and current path must share identity");
}

#[test]
fn v04_identity_scoped_query_returns_all_six_commits() {
    let f     = create_rename_fixture();
    let store = Store::open(":memory:").unwrap();
    ingest_all(&store, &f.path);

    let id = store.resolve_path_to_identity("crates/auth/src/lib.rs", &f.path).unwrap()
        .expect("crates/auth/src/lib.rs must resolve to exactly one identity");
    let commits = store.commits_for_identity(id, &f.path).unwrap();
    assert_eq!(commits.len(), 6, "identity-scoped query must return all 6 commits");

    let hashes: Vec<&str> = commits.iter().map(|c| c.short_hash.as_str()).collect();
    for (label, hash) in [
        ("A", &f.hash_a), ("B", &f.hash_b), ("C", &f.hash_c),
        ("D", &f.hash_d), ("E", &f.hash_e), ("F", &f.hash_f),
    ] {
        assert!(hashes.contains(&&hash[..7]), "commit {label} missing from identity query");
    }
}

#[test]
fn v04_context_current_path_shows_full_history() {
    let f     = create_rename_fixture();
    let store = Store::open(":memory:").unwrap();
    ingest_all(&store, &f.path);

    let doc = build_context("crates/auth/src/lib.rs", &f.path, &store).unwrap();
    assert_eq!(doc.identity.touch_count, 6, "touch_count must span full artifact history");
    assert_eq!(
        doc.identity.first_commit.as_ref().unwrap().short_hash,
        &f.hash_a[..7],
        "first_commit must be commit A (true origin)"
    );
    assert_eq!(
        doc.coverage.rename_tracking,
        atlas_ir::CoverageStatus::Available,
        "rename_tracking must be Available once ingested"
    );
}

#[test]
fn v04_historical_path_shows_navigation_hint() {
    let f     = create_rename_fixture();
    let store = Store::open(":memory:").unwrap();
    ingest_all(&store, &f.path);

    let doc = build_context("auth.rs", &f.path, &store).unwrap();
    assert_eq!(
        doc.identity.current_path.as_deref(),
        Some("crates/auth/src/lib.rs"),
        "historical path query must expose current canonical path"
    );
    assert!(
        doc.identity.is_historical_path,
        "must flag that the queried path is historical, not current"
    );
}

#[test]
fn v04_rename_evidence_carries_similarity_score() {
    let f     = create_rename_fixture();
    let store = Store::open(":memory:").unwrap();
    ingest_all(&store, &f.path);

    let id = store.resolve_path_to_identity("crates/auth/src/lib.rs", &f.path).unwrap()
        .expect("crates/auth/src/lib.rs must resolve to exactly one identity");
    let evidence = store.identity_evidence_for(id, &f.path).unwrap();

    // Commit E was a rename+modify (~75% similarity): security/auth.rs → crates/auth/src/lib.rs
    let rename_e = evidence.iter().find(|e| e.source_commit_hash.starts_with(&f.hash_e[..7]));
    assert!(rename_e.is_some(), "IdentityEvidence must exist for rename commit E");
    let r = rename_e.unwrap();
    // similarity_score range matches evidence_pipeline_captures_both_rename_commits
    assert!(r.similarity >= 60 && r.similarity <= 85,
        "similarity for commit E should be in 60–85% range (got {})", r.similarity);
    assert_eq!(r.detection_source, "git-rename",
        "detection_source must identify the evidence mechanism, not assert identity certainty");
}

// ─────────────────────────────────────────────────────────────────────────────
// PATH-REUSE FIXTURE
// ─────────────────────────────────────────────────────────────────────────────
//
// A simple path→identity_id table cannot represent two distinct artifacts that
// occupied the same path at different points in time.  This fixture forces the
// schema to be temporally aware.
//
//   X  create service.rs                        → artifact S1
//   Y  rename service.rs → legacy/service.rs    → S1 moves, path freed
//   Z  create service.rs (new file, blank slate) → artifact S2
//
// The correct resolution semantics:
//   resolve_current_path("service.rs")          → S2
//   identities_for_path("service.rs")           → [S1, S2]
//   resolve_path_at("service.rs", commit X)     → S1
//   resolve_path_at("service.rs", commit Z)     → S2

struct PathReuseFixture {
    _dir:       TempDir,
    pub path:   String,
    pub hash_x: String,
    pub hash_y: String,
    pub hash_z: String,
}

fn create_path_reuse_fixture() -> PathReuseFixture {
    let dir  = tempfile::tempdir().expect("tempdir");
    let repo = dir.path().to_str().unwrap().to_string();

    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(&repo)
            .output()
            .expect("git");
        assert!(
            out.status.success(),
            "git {:?} failed:\n{}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    };

    let commit = |path: &str, msg: &str, date: &str| {
        let out = Command::new("git")
            .args(["commit", "-m", msg])
            .current_dir(path)
            .env("GIT_AUTHOR_DATE",    date)
            .env("GIT_COMMITTER_DATE", date)
            .output()
            .expect("git commit");
        assert!(out.status.success(), "commit failed:\n{}", String::from_utf8_lossy(&out.stderr));
    };

    let head = |path: &str| -> String {
        let out = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(path)
            .output()
            .expect("rev-parse");
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    };

    git(&["init", "-b", "main"]);
    git(&["config", "user.email", "fixture@test.com"]);
    git(&["config", "user.name", "Fixture"]);

    // Commit X: create service.rs — this is artifact S1
    std::fs::write(format!("{repo}/service.rs"), "pub fn serve() {}\n").unwrap();
    git(&["add", "service.rs"]);
    commit(&repo, "Add service module", "2024-02-01T10:00:00+0000");
    let hash_x = head(&repo);

    // Commit Y: rename service.rs → legacy/service.rs — S1 moves, path is now free
    std::fs::create_dir_all(format!("{repo}/legacy")).unwrap();
    git(&["mv", "service.rs", "legacy/service.rs"]);
    commit(&repo, "Retire service.rs to legacy package", "2024-02-02T10:00:00+0000");
    let hash_y = head(&repo);

    // Commit Z: create NEW service.rs — this is a completely different artifact S2
    std::fs::write(format!("{repo}/service.rs"), "pub fn handle_request() {}\n").unwrap();
    git(&["add", "service.rs"]);
    commit(&repo, "Add new service module (v2 design)", "2024-02-03T10:00:00+0000");
    let hash_z = head(&repo);

    PathReuseFixture { _dir: dir, path: repo, hash_x, hash_y, hash_z }
}

// ── Path-reuse baseline tests (pass now) ─────────────────────────────────────

#[test]
fn baseline_path_reuse_ingest_produces_three_commits() {
    let f     = create_path_reuse_fixture();
    let store = Store::open(":memory:").unwrap();
    let count = ingest_git(&f.path, &store).unwrap();
    assert_eq!(count, 3, "path-reuse fixture must have exactly 3 commits");
}

#[test]
fn baseline_path_reuse_current_path_sees_both_artifacts() {
    // Current path-scoped behavior: "service.rs" appears in both commit X (S1's
    // creation) and commit Z (S2's creation).  Atlas conflates S1 and S2 into
    // a single path history.  This is the temporal ambiguity — the path was
    // reused and Atlas has no way to distinguish the two artifacts.
    let f     = create_path_reuse_fixture();
    let store = Store::open(":memory:").unwrap();
    ingest_git(&f.path, &store).unwrap();

    let commits = store.commits_for_file("service.rs", &f.path).unwrap();
    let hashes: Vec<&str> = commits.iter().map(|c| c.short_hash.as_str()).collect();

    // Both X (S1 creation) and Z (S2 creation) appear — path-scoped conflation.
    assert!(
        hashes.contains(&&f.hash_x[..7]),
        "commit X (S1 creation) must appear under 'service.rs' in path-scoped mode"
    );
    assert!(
        hashes.contains(&&f.hash_z[..7]),
        "commit Z (S2 creation) must appear under 'service.rs' in path-scoped mode"
    );
    // Commit Y renamed service.rs away — it does NOT touch service.rs afterwards.
    assert!(
        !hashes.contains(&&f.hash_y[..7]),
        "commit Y (rename-away) must NOT appear under 'service.rs'"
    );
}

#[test]
fn baseline_path_reuse_schema_cannot_distinguish_s1_from_s2() {
    // This is the key schema constraint proof: with only a path→identity mapping
    // (no temporal bounds), a schema could store only ONE identity for "service.rs".
    // The baseline proves that the query returns commits from BOTH artifacts.
    // v0.4 must store TWO distinct identities for this path, with non-overlapping
    // valid-time ranges.
    let f     = create_path_reuse_fixture();
    let store = Store::open(":memory:").unwrap();
    ingest_git(&f.path, &store).unwrap();

    // Under current path-scoped model: touch_count for "service.rs" is 2
    // (commits X and Z), even though these are unrelated engineering artifacts.
    let touch = store.touch_count("service.rs", &f.path).unwrap();
    assert_eq!(
        touch, 2,
        "path-scoped touch_count conflates S1+S2 into a single count of 2"
    );

    // Correct answer that v0.4 must produce:
    //   touch_count(S1) = 1  (only commit X)
    //   touch_count(S2) = 1  (only commit Z)
    // Both sum to 2, but they must NOT be presented as a single artifact's history.
}

// ── v0.4 path-reuse acceptance criteria (#[ignore]) ──────────────────────────

#[test]
fn v04_path_reuse_two_distinct_identities() {
    let f     = create_path_reuse_fixture();
    let store = Store::open(":memory:").unwrap();
    ingest_all(&store, &f.path);

    let ids = store.identities_for_path("service.rs", &f.path).unwrap();
    assert_eq!(ids.len(), 2, "service.rs must resolve to two distinct identities (S1 and S2)");

    // ids ordered by file_identity_id ASC: S1 created first (rename edge X→Y processed first)
    let s1_commits = store.commits_for_identity(ids[0], &f.path).unwrap();
    let s2_commits = store.commits_for_identity(ids[1], &f.path).unwrap();
    let all_for_s1: Vec<&str> = s1_commits.iter().map(|c| c.short_hash.as_str()).collect();
    let all_for_s2: Vec<&str> = s2_commits.iter().map(|c| c.short_hash.as_str()).collect();
    assert!(all_for_s1.contains(&&f.hash_x[..7]) ^ all_for_s2.contains(&&f.hash_x[..7]),
        "commit X must belong to exactly ONE of the two identities");
    assert!(all_for_s1.contains(&&f.hash_z[..7]) ^ all_for_s2.contains(&&f.hash_z[..7]),
        "commit Z must belong to exactly ONE of the two identities");
}

#[test]
fn v04_path_reuse_resolve_at_returns_correct_identity() {
    let f     = create_path_reuse_fixture();
    let store = Store::open(":memory:").unwrap();
    ingest_all(&store, &f.path);

    let s1 = store.resolve_path_at("service.rs", &f.hash_x, &f.path).unwrap()
        .expect("service.rs at commit X must resolve to S1");
    let s2 = store.resolve_path_at("service.rs", &f.hash_z, &f.path).unwrap()
        .expect("service.rs at commit Z must resolve to S2");
    assert_ne!(s1, s2, "service.rs at commit X and at commit Z must be different identities");

    let s1_commits = store.commits_for_identity(s1, &f.path).unwrap();
    let s2_commits = store.commits_for_identity(s2, &f.path).unwrap();
    let s1_hashes: Vec<&str> = s1_commits.iter().map(|c| c.short_hash.as_str()).collect();
    let s2_hashes: Vec<&str> = s2_commits.iter().map(|c| c.short_hash.as_str()).collect();
    assert!(s1_hashes.contains(&&f.hash_x[..7]), "S1 must contain commit X");
    assert!(!s1_hashes.contains(&&f.hash_z[..7]), "S1 must NOT contain commit Z");
    assert!(s2_hashes.contains(&&f.hash_z[..7]), "S2 must contain commit Z");
    assert!(!s2_hashes.contains(&&f.hash_x[..7]), "S2 must NOT contain commit X");
}

#[test]
fn v04_path_reuse_resolve_current_returns_s2() {
    let f     = create_path_reuse_fixture();
    let store = Store::open(":memory:").unwrap();
    ingest_all(&store, &f.path);

    let current = store.resolve_current_path("service.rs", &f.path).unwrap()
        .expect("service.rs must have a current occupant (S2)");
    let s2_commits = store.commits_for_identity(current, &f.path).unwrap();
    let hashes: Vec<&str> = s2_commits.iter().map(|c| c.short_hash.as_str()).collect();
    assert!(hashes.contains(&&f.hash_z[..7]), "current identity must include commit Z");
    assert!(!hashes.contains(&&f.hash_x[..7]),
        "current identity must NOT include commit X — that belongs to S1");
}

// ── v0.4.1 Governing invariant regression tests ───────────────────────────────
//
// Invariant: once Atlas resolves a path to a FileIdentity, every identity-level
// analytical operation must operate over that identity's temporal history unless
// the operation is explicitly documented as path-scoped.
//
// These tests prove that current-path and historical-path queries produce
// equivalent identity-level analytics.

#[test]
fn identity_invariant_touch_count_equivalent_across_paths() {
    // touch_count from the current path must equal touch_count from any historical path.
    let f     = create_rename_fixture();
    let store = Store::open(":memory:").unwrap();
    ingest_all(&store, &f.path);

    let doc_current  = build_context("crates/auth/src/lib.rs", &f.path, &store).unwrap();
    let doc_mid      = build_context("security/auth.rs",       &f.path, &store).unwrap();
    let doc_original = build_context("auth.rs",                &f.path, &store).unwrap();

    assert_eq!(doc_current.identity.touch_count, 6);
    assert_eq!(doc_mid.identity.touch_count,  doc_current.identity.touch_count,
        "intermediate path must yield same touch_count as current path");
    assert_eq!(doc_original.identity.touch_count, doc_current.identity.touch_count,
        "original path must yield same touch_count as current path");
}

#[test]
fn identity_invariant_significance_rank_coherent_across_paths() {
    // Significance rank for historical and current paths must be the same.
    // Both planes (rank and touch_count) must be identity-scoped — no mixing.
    let f     = create_rename_fixture();
    let store = Store::open(":memory:").unwrap();
    ingest_all(&store, &f.path);

    let doc_current  = build_context("crates/auth/src/lib.rs", &f.path, &store).unwrap();
    let doc_original = build_context("auth.rs",                &f.path, &store).unwrap();

    let sig_current  = doc_current.significance.as_ref()
        .expect("current path must have significance");
    let sig_original = doc_original.significance.as_ref()
        .expect("historical path must have significance");

    assert_eq!(sig_current.touch_count, 6, "significance touch_count must be identity-scoped");
    assert_eq!(sig_original.touch_count, sig_current.touch_count,
        "historical path significance must match current path — no plane mixing");
    assert_eq!(sig_original.rank, sig_current.rank,
        "historical path rank must equal current path rank — both use canonical path in hot list");
}

#[test]
fn identity_invariant_co_changes_equivalent_across_paths() {
    // Co-changes for the current path and all historical paths must be identical.
    // They all resolve to the same FileIdentity, so the coupling graph must be the same.
    let f     = create_rename_fixture();
    let store = Store::open(":memory:").unwrap();
    ingest_all(&store, &f.path);

    let doc_current  = build_context("crates/auth/src/lib.rs", &f.path, &store).unwrap();
    let doc_original = build_context("auth.rs",                &f.path, &store).unwrap();

    let paths_current:  Vec<&str> = doc_current.coupling.iter().map(|e| e.file_path.as_str()).collect();
    let paths_original: Vec<&str> = doc_original.coupling.iter().map(|e| e.file_path.as_str()).collect();

    assert_eq!(paths_current, paths_original,
        "coupling list must be identical for current and historical paths of the same identity");
}

#[test]
fn identity_invariant_navigation_hint_present_on_historical_paths() {
    // Historical-path queries must carry a navigation hint pointing to the current canonical path.
    // Current-path queries must NOT carry a navigation hint.
    let f     = create_rename_fixture();
    let store = Store::open(":memory:").unwrap();
    ingest_all(&store, &f.path);

    // Current path: no hint
    let doc_current = build_context("crates/auth/src/lib.rs", &f.path, &store).unwrap();
    assert!(!doc_current.identity.is_historical_path,
        "current path must NOT be flagged as historical");
    assert!(doc_current.identity.current_path.is_none(),
        "current path must NOT carry a redirect hint");

    // First historical path
    let doc_a = build_context("auth.rs", &f.path, &store).unwrap();
    assert!(doc_a.identity.is_historical_path, "auth.rs must be flagged as historical");
    assert_eq!(doc_a.identity.current_path.as_deref(), Some("crates/auth/src/lib.rs"),
        "auth.rs redirect must point to canonical current path");

    // Intermediate historical path
    let doc_b = build_context("security/auth.rs", &f.path, &store).unwrap();
    assert!(doc_b.identity.is_historical_path, "security/auth.rs must be flagged as historical");
    assert_eq!(doc_b.identity.current_path.as_deref(), Some("crates/auth/src/lib.rs"),
        "security/auth.rs redirect must point to canonical current path");
}
