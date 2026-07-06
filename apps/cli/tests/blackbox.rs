/// Black-box integration tests for the `atlas` binary.
///
/// These tests build nothing themselves — Cargo pre-builds the binary before
/// running integration tests, so `CARGO_BIN_EXE_atlas` always points to a
/// fresh build.  Every test creates its own isolated temp directory for the
/// fixture git repo and its own temp file for the SQLite database, so tests
/// are fully parallel-safe and leave no residue.

use std::process::{Command, Output};
use tempfile::TempDir;

// ── Binary path ──────────────────────────────────────────────────────────────

fn atlas_bin() -> &'static str {
    env!("CARGO_BIN_EXE_atlas")
}

// ── Helpers ──────────────────────────────────────────────────────────────────

struct Fixture {
    _repo_dir: TempDir,
    _db_dir:   TempDir,
    pub repo:  String,
    pub db:    String,
    pub hash_a: String,
    pub hash_b: String,
    pub hash_c: String,
}

impl Fixture {
    fn create() -> Self {
        let repo_dir = tempfile::tempdir().expect("repo tempdir");
        let db_dir   = tempfile::tempdir().expect("db tempdir");
        let repo     = repo_dir.path().to_str().unwrap().to_string();
        let db       = db_dir.path().join("atlas.db").to_str().unwrap().to_string();

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

        git(&["init", "-b", "main"]);
        git(&["config", "user.email", "fixture@test.com"]);
        git(&["config", "user.name", "Fixture"]);

        // Use explicit timestamps so same-second execution doesn't collapse ordering.
        let commit = |repo: &str, msg: &str, date: &str| {
            let out = Command::new("git")
                .args(["commit", "-m", msg])
                .current_dir(repo)
                .env("GIT_AUTHOR_DATE",    date)
                .env("GIT_COMMITTER_DATE", date)
                .output()
                .expect("git commit");
            assert!(out.status.success(), "git commit failed: {}", String::from_utf8_lossy(&out.stderr));
        };

        // Commit A — creates auth.ts  (2024-01-01)
        std::fs::write(format!("{repo}/auth.ts"), "export {}").unwrap();
        git(&["add", "auth.ts"]);
        commit(&repo, "Add authentication module", "2024-01-01T10:00:00+0000");
        let hash_a = head_hash(&repo);

        // Commit B — modifies auth.ts, creates user.ts  (2024-01-02)
        std::fs::write(format!("{repo}/auth.ts"), "export function auth() {}").unwrap();
        std::fs::write(format!("{repo}/user.ts"), "export {}").unwrap();
        git(&["add", "auth.ts", "user.ts"]);
        commit(&repo, "Add user model, extend auth", "2024-01-02T10:00:00+0000");
        let hash_b = head_hash(&repo);

        // Commit C — modifies user.ts  (2024-01-03)
        std::fs::write(format!("{repo}/user.ts"), "export function getUser() {}").unwrap();
        git(&["add", "user.ts"]);
        commit(&repo, "Add getUser function", "2024-01-03T10:00:00+0000");
        let hash_c = head_hash(&repo);

        Fixture { _repo_dir: repo_dir, _db_dir: db_dir, repo, db, hash_a, hash_b, hash_c }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(atlas_bin())
            .args(args)
            .current_dir(&self.repo)
            .env("ATLAS_DB", &self.db)
            .output()
            .expect("atlas binary")
    }

    fn run_ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "atlas {:?} failed:\nstdout: {}\nstderr: {}",
            args,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
        String::from_utf8(out.stdout).unwrap()
    }
}

fn head_hash(repo: &str) -> String {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .expect("rev-parse");
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn status_exits_zero() {
    let f = Fixture::create();
    f.run_ok(&["status"]);
}

#[test]
fn ingest_reports_three_commits() {
    let f = Fixture::create();
    let output = f.run_ok(&["ingest", "."]);
    assert!(
        output.contains("3 commits"),
        "expected '3 commits' in output:\n{output}"
    );
}

#[test]
fn explain_auth_ts_contains_commit_a_and_b() {
    let f = Fixture::create();
    f.run_ok(&["ingest", "."]);

    let output = f.run_ok(&["explain", "auth.ts"]);

    let short_a = &f.hash_a[..7];
    let short_b = &f.hash_b[..7];
    let short_c = &f.hash_c[..7];

    assert!(
        output.contains(short_a),
        "commit A ({short_a}) missing from explain auth.ts:\n{output}"
    );
    assert!(
        output.contains(short_b),
        "commit B ({short_b}) missing from explain auth.ts:\n{output}"
    );
    assert!(
        !output.contains(short_c),
        "commit C ({short_c}) must NOT appear in explain auth.ts:\n{output}"
    );
}

#[test]
fn explain_user_ts_contains_commit_b_and_c() {
    let f = Fixture::create();
    f.run_ok(&["ingest", "."]);

    let output = f.run_ok(&["explain", "user.ts"]);

    let short_b = &f.hash_b[..7];
    let short_c = &f.hash_c[..7];
    let short_a = &f.hash_a[..7];

    assert!(
        output.contains(short_b),
        "commit B ({short_b}) missing from explain user.ts:\n{output}"
    );
    assert!(
        output.contains(short_c),
        "commit C ({short_c}) missing from explain user.ts:\n{output}"
    );
    assert!(
        !output.contains(short_a),
        "commit A ({short_a}) must NOT appear in explain user.ts:\n{output}"
    );
}

#[test]
fn query_auth_ts_contains_correct_commits() {
    let f = Fixture::create();
    f.run_ok(&["ingest", "."]);

    let output = f.run_ok(&["query", "auth.ts"]);

    assert!(
        output.contains(&f.hash_a[..7]),
        "commit A missing from query auth.ts:\n{output}"
    );
    assert!(
        output.contains(&f.hash_b[..7]),
        "commit B missing from query auth.ts:\n{output}"
    );
}

#[test]
fn double_ingest_does_not_duplicate_commits() {
    let f = Fixture::create();
    f.run_ok(&["ingest", "."]);
    f.run_ok(&["ingest", "."]);

    let output = f.run_ok(&["explain", "auth.ts"]);
    assert!(
        output.contains("Touch history (2 commits)"),
        "expected exactly 2 commits for auth.ts after double ingest:\n{output}"
    );
}

#[test]
fn co_changes_shows_coupled_file() {
    let f = Fixture::create();
    f.run_ok(&["ingest", "."]);

    // Commit B touches both auth.ts and user.ts, so they co-change.
    let output = f.run_ok(&["co-changes", "auth.ts"]);
    assert!(
        output.contains("user.ts"),
        "expected user.ts as a co-changed file for auth.ts:\n{output}"
    );
}

#[test]
fn co_changes_no_data_message() {
    let f = Fixture::create();
    let output = f.run_ok(&["co-changes", "auth.ts"]);
    assert!(
        output.contains("No co-change data"),
        "expected 'No co-change data' before ingestion:\n{output}"
    );
}

#[test]
fn co_changes_min_count_filters_single_occurrence() {
    let f = Fixture::create();
    f.run_ok(&["ingest", "."]);

    // user.ts co-changes with auth.ts exactly once (commit B).
    // With --min-count 2, it should be filtered out.
    let output = f.run_ok(&["co-changes", "auth.ts", "--min-count", "2"]);
    assert!(
        !output.contains("user.ts"),
        "user.ts should be filtered out by --min-count 2:\n{output}"
    );
    assert!(
        output.contains("No co-changed files") || output.contains("0 co-changed"),
        "expected empty result message:\n{output}"
    );
}

#[test]
fn hot_files_shows_most_modified_files() {
    let f = Fixture::create();
    f.run_ok(&["ingest", "."]);

    let output = f.run_ok(&["hot-files"]);
    // auth.ts and user.ts each appear in 2 commits.
    assert!(output.contains("auth.ts"), "auth.ts missing from hot-files:\n{output}");
    assert!(output.contains("user.ts"), "user.ts missing from hot-files:\n{output}");
    assert!(output.contains("2×"), "expected '2×' frequency count:\n{output}");
}

#[test]
fn hot_files_no_data_message() {
    let f = Fixture::create();
    let output = f.run_ok(&["hot-files"]);
    assert!(
        output.contains("No file history"),
        "expected 'No file history' before ingestion:\n{output}"
    );
}

#[test]
fn when_introduced_shows_first_commit() {
    let f = Fixture::create();
    f.run_ok(&["ingest", "."]);

    let output = f.run_ok(&["when-introduced", "auth.ts"]);

    // auth.ts was first introduced in commit A ("Add authentication module").
    assert!(
        output.contains("Add authentication module"),
        "expected commit A message in when-introduced output:\n{output}"
    );
    assert!(
        output.contains(&f.hash_a[..7]),
        "expected commit A short hash:\n{output}"
    );
    // Must NOT mention commit B's message.
    assert!(
        !output.contains("Add user model"),
        "commit B must not appear in when-introduced auth.ts:\n{output}"
    );
}

#[test]
fn when_introduced_unknown_file() {
    let f = Fixture::create();
    f.run_ok(&["ingest", "."]);

    let output = f.run_ok(&["when-introduced", "ghost.ts"]);
    assert!(
        output.contains("not found in history"),
        "expected 'not found in history':\n{output}"
    );
}

#[test]
fn timeline_shows_all_relevant_commits() {
    let f = Fixture::create();
    f.run_ok(&["ingest", "."]);

    let output = f.run_ok(&["timeline", "auth.ts"]);

    // Both commits that touched auth.ts must appear.
    assert!(
        output.contains(&f.hash_a[..7]),
        "commit A missing from timeline:\n{output}"
    );
    assert!(
        output.contains(&f.hash_b[..7]),
        "commit B missing from timeline:\n{output}"
    );
    // Commit C only touches user.ts — must NOT appear for auth.ts.
    assert!(
        !output.contains(&f.hash_c[..7]),
        "commit C must not appear in auth.ts timeline:\n{output}"
    );
    // The section label confirms oldest-first intent.
    assert!(
        output.contains("oldest → newest"),
        "expected oldest→newest section label:\n{output}"
    );
}

#[test]
fn explain_unknown_file_shows_no_data_message() {
    let f = Fixture::create();
    f.run_ok(&["ingest", "."]);

    let output = f.run_ok(&["explain", "nonexistent.ts"]);
    // explain prints "No data found" when nothing is in the DB for that file
    assert!(
        output.contains("No data found"),
        "expected 'No data found' message for unknown file:\n{output}"
    );
}

#[test]
fn ingest_nonexistent_path_exits_nonzero() {
    let f = Fixture::create();
    let out = f.run(&["ingest", "/this/path/does/not/exist"]);
    assert!(
        !out.status.success(),
        "expected non-zero exit for bad repo path"
    );
}
