/// Black-box integration tests for the `atlas` binary.
///
/// These tests build nothing themselves — Cargo pre-builds the binary before
/// running integration tests, so `CARGO_BIN_EXE_atlas` always points to a
/// fresh build.  Every test creates its own isolated temp directory for the
/// fixture git repo and its own temp file for the SQLite database, so tests
/// are fully parallel-safe and leave no residue.
///
/// Two fixtures:
/// - `Fixture` — a minimal 3-commit git repo with TypeScript files (auth.ts, user.ts)
/// - `TsFixture` — extends Fixture with TypeScript source that has real structural edges

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

    /// Extend the repo with TypeScript files that have real structural edges, then
    /// commit and re-ingest with `--typescript`.
    fn add_typescript_sources(&self) {
        let git = |args: &[&str]| {
            let out = Command::new("git")
                .args(args)
                .current_dir(&self.repo)
                .output()
                .expect("git");
            assert!(out.status.success(), "git {:?} failed", args);
        };

        std::fs::create_dir_all(format!("{}/src/models", self.repo)).unwrap();
        std::fs::create_dir_all(format!("{}/src/services", self.repo)).unwrap();

        std::fs::write(
            format!("{}/src/models/user.model.ts", self.repo),
            "import { model } from \"mongoose\";\nexport const User = model(\"User\", {});\n",
        ).unwrap();

        std::fs::write(
            format!("{}/src/services/user.service.ts", self.repo),
            "import { User } from \"../models/user.model.js\";\n\
             export class UserService {\n\
               static async getUser(id: string) { return User.findById(id); }\n\
             }\n",
        ).unwrap();

        git(&["add", "src/"]);
        let out = Command::new("git")
            .args(["commit", "-m", "Add user model and service"])
            .current_dir(&self.repo)
            .env("GIT_AUTHOR_DATE",    "2024-01-04T10:00:00+0000")
            .env("GIT_COMMITTER_DATE", "2024-01-04T10:00:00+0000")
            .output()
            .expect("git commit");
        assert!(out.status.success(), "git commit TS sources failed");
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

#[test]
fn context_identity_shows_first_and_last_commit() {
    let f = Fixture::create();
    f.run_ok(&["ingest", "."]);

    let output = f.run_ok(&["context", "auth.ts"]);

    // Identity section must report the correct introduction commit
    assert!(
        output.contains("Add authentication module"),
        "expected intro commit message in IDENTITY:\n{output}"
    );
    assert!(
        output.contains(&f.hash_a[..7]),
        "expected commit A hash in IDENTITY:\n{output}"
    );
    // Last changed should mention commit B (auth.ts modified in commit B too)
    assert!(
        output.contains(&f.hash_b[..7]),
        "expected commit B hash somewhere in context:\n{output}"
    );
}

#[test]
fn context_coverage_shows_not_ingested_without_github() {
    let f = Fixture::create();
    f.run_ok(&["ingest", "."]);

    let output = f.run_ok(&["context", "auth.ts"]);

    assert!(
        output.contains("COVERAGE"),
        "expected COVERAGE section:\n{output}"
    );
    // Git history is present but only path-scoped (no rename tracking)
    assert!(
        output.contains("△ path-scoped"),
        "expected git history reported as path-scoped:\n{output}"
    );
    assert!(
        output.contains("Rename tracking") && output.contains("✗ not ingested"),
        "expected rename tracking listed as not ingested:\n{output}"
    );
    // Without GitHub ingestion, PRs and issues should be not ingested
    assert!(
        output.matches("✗ not ingested").count() >= 3,
        "expected at least 3 'not ingested' entries (rename, PRs, issues):\n{output}"
    );
}

#[test]
fn context_coupling_shows_co_changed_files() {
    let f = Fixture::create();
    f.run_ok(&["ingest", "."]);

    let output = f.run_ok(&["context", "auth.ts"]);

    assert!(
        output.contains("HISTORICAL COUPLING"),
        "expected HISTORICAL COUPLING section:\n{output}"
    );
    assert!(
        output.contains("user.ts"),
        "expected user.ts in coupling (changed with auth.ts in commit B):\n{output}"
    );
}

#[test]
fn context_evidence_section_present() {
    let f = Fixture::create();
    f.run_ok(&["ingest", "."]);

    let output = f.run_ok(&["context", "auth.ts"]);

    assert!(
        output.contains("EVIDENCE"),
        "expected EVIDENCE section:\n{output}"
    );
    assert!(
        output.contains("deterministic fact"),
        "expected deterministic facts count:\n{output}"
    );
    assert!(
        output.contains("no inferred claims"),
        "expected no inferred claims statement:\n{output}"
    );
}

#[test]
fn context_significance_shows_rank() {
    let f = Fixture::create();
    f.run_ok(&["ingest", "."]);

    let output = f.run_ok(&["context", "auth.ts"]);

    assert!(
        output.contains("CURRENT SIGNIFICANCE"),
        "expected CURRENT SIGNIFICANCE section:\n{output}"
    );
    assert!(
        output.contains("Ranked #"),
        "expected rank in significance:\n{output}"
    );
}

#[test]
fn context_unknown_file_returns_zero_touches() {
    let f = Fixture::create();
    f.run_ok(&["ingest", "."]);

    let output = f.run_ok(&["context", "ghost.ts"]);

    // No error, but touch count should be zero and no commits shown
    assert!(
        output.contains("Total touches: 0"),
        "expected 0 touches for unknown file:\n{output}"
    );
}

// ── search tests ─────────────────────────────────────────────────────────────

#[test]
fn search_finds_file_path_in_observed_section() {
    let f = Fixture::create();
    f.run_ok(&["ingest", "."]);

    // "auth" appears in auth.ts (file path) and in commit messages.
    let output = f.run_ok(&["search", "auth"]);

    assert!(
        output.contains("OBSERVED"),
        "expected OBSERVED section for file-path match:\n{output}"
    );
    assert!(
        output.contains("auth.ts"),
        "expected auth.ts in OBSERVED section:\n{output}"
    );
}

#[test]
fn search_finds_commit_message_in_historical_section() {
    let f = Fixture::create();
    f.run_ok(&["ingest", "."]);

    // "getUser" appears only in commit C message — no file is named getUser.
    let output = f.run_ok(&["search", "getUser"]);

    assert!(
        output.contains("HISTORICAL"),
        "expected HISTORICAL section for commit-message match:\n{output}"
    );
    assert!(
        output.contains("getUser"),
        "expected getUser snippet in HISTORICAL section:\n{output}"
    );
    // No file is named getUser so OBSERVED should be absent.
    assert!(
        !output.contains("OBSERVED"),
        "OBSERVED section must not appear when there is no file-path match:\n{output}"
    );
}

#[test]
fn search_no_match_shows_no_matches_message() {
    let f = Fixture::create();
    f.run_ok(&["ingest", "."]);

    let output = f.run_ok(&["search", "xyzzy_not_in_corpus"]);

    assert!(
        output.contains("No matches found"),
        "expected 'No matches found' for term absent from corpus:\n{output}"
    );
}

#[test]
fn search_json_flag_emits_valid_json_with_schema_version() {
    let f = Fixture::create();
    f.run_ok(&["ingest", "."]);

    let output = f.run_ok(&["search", "auth", "--json"]);

    let parsed: serde_json::Value =
        serde_json::from_str(&output).expect("--json output must be valid JSON");

    assert_eq!(
        parsed["schema_version"].as_u64().unwrap(),
        1,
        "expected schema_version = 1"
    );
    assert!(parsed["anchors"].is_array(),  "expected 'anchors' array");
    assert!(parsed["matches"].is_array(),  "expected 'matches' array");
    assert!(parsed["coverage"].is_object(), "expected 'coverage' object");

    let anchors = parsed["anchors"].as_array().unwrap();
    assert!(
        anchors.iter().any(|a| a.as_str() == Some("auth")),
        "anchors must contain the queried term"
    );
}

#[test]
fn search_coverage_section_shows_searched_sources() {
    let f = Fixture::create();
    f.run_ok(&["ingest", "."]);

    let output = f.run_ok(&["search", "auth"]);

    assert!(
        output.contains("COVERAGE"),
        "expected COVERAGE section:\n{output}"
    );
    assert!(
        output.contains("✓ searched"),
        "expected at least one '✓ searched' entry:\n{output}"
    );
}

#[test]
fn context_json_flag_emits_valid_json_with_expected_keys() {
    let f = Fixture::create();
    f.run_ok(&["ingest", "."]);

    let output = f.run_ok(&["context", "auth.ts", "--json"]);

    let parsed: serde_json::Value =
        serde_json::from_str(&output).expect("--json output must be valid JSON");

    assert!(parsed["subject"].is_string(),   "expected 'subject' key");
    assert!(parsed["identity"].is_object(),  "expected 'identity' object");
    assert!(parsed["coverage"].is_object(),  "expected 'coverage' object");
    assert!(parsed["evidence"].is_object(),  "expected 'evidence' object");

    assert_eq!(
        parsed["subject"].as_str().unwrap(),
        "auth.ts",
        "subject must match the requested file"
    );
    assert_eq!(
        parsed["identity"]["touch_count"].as_i64().unwrap(),
        2,
        "touch_count must be 2 for auth.ts in the 3-commit fixture"
    );
}

// ── investigate tests ─────────────────────────────────────────────────────────

#[test]
fn investigate_finds_file_path_candidates() {
    let f = Fixture::create();
    f.run_ok(&["ingest", "."]);

    // "auth" matches auth.ts (file path).  auth.ts has no test/migration/schema
    // path pattern so it must appear in the core implementation neighborhood.
    let output = f.run_ok(&["investigate", "auth", "--raw"]);

    assert!(
        output.contains("CORE IMPLEMENTATION NEIGHBORHOOD"),
        "expected CORE IMPLEMENTATION NEIGHBORHOOD section:\n{output}"
    );
    assert!(
        output.contains("auth.ts"),
        "expected auth.ts as a candidate:\n{output}"
    );
    assert!(
        output.contains("anchor match"),
        "expected provenance label 'anchor match':\n{output}"
    );
}

#[test]
fn investigate_shows_structural_neighbors_after_typescript_ingest() {
    let f = Fixture::create();
    f.add_typescript_sources();
    f.run_ok(&["ingest", ".", "--typescript"]);

    // "user" matches user.ts AND src/models/user.model.ts AND src/services/user.service.ts.
    // user.service.ts imports user.model.ts — structural neighbor.
    let output = f.run_ok(&["investigate", "user", "--raw"]);

    assert!(
        output.contains("OBSERVED STRUCTURE") || output.contains("CANDIDATE ARTIFACTS"),
        "expected structural output:\n{output}"
    );
    assert!(
        output.contains("user.model.ts") || output.contains("user.service.ts"),
        "expected TypeScript files in investigation:\n{output}"
    );
}

#[test]
fn investigate_json_flag_emits_valid_json_with_schema_version() {
    let f = Fixture::create();
    f.run_ok(&["ingest", "."]);

    let output = f.run_ok(&["investigate", "auth", "--json"]);

    let parsed: serde_json::Value =
        serde_json::from_str(&output).expect("--json output must be valid JSON");

    assert_eq!(parsed["schema_version"].as_u64().unwrap(), 4, "schema_version must be 4 (v0.7 related_decisions)");
    assert!(parsed["anchors"].is_array(),                "expected 'anchors' array");
    assert!(parsed["effective_anchors"].is_array(),      "expected 'effective_anchors' array");
    assert!(parsed["concept_expansions"].is_array(),     "expected 'concept_expansions' array");
    assert!(parsed["core_candidates"].is_array(),        "expected 'core_candidates' array");
    assert!(parsed["supporting_artifacts"].is_array(),   "expected 'supporting_artifacts' array");
    assert!(parsed["observed_structure"].is_array());
    assert!(parsed["documentary"].is_array());
    assert!(parsed["historical"].is_array());
    assert!(parsed["unresolved"].is_array());
    assert!(parsed["related_decisions"].is_array(),      "expected 'related_decisions' array");
    assert!(parsed["coverage"].is_object());

    // auth.ts has no test/migration/schema path markers → must be in core_candidates
    let core = parsed["core_candidates"].as_array().unwrap();
    assert!(
        core.iter().any(|c| c["file"].as_str() == Some("auth.ts")),
        "auth.ts must appear in core_candidates: {core:?}"
    );
    // Every core candidate must carry a role field
    for c in core {
        assert!(c["role"].is_string(), "each core candidate must have a role field: {c}");
    }

    // effective_anchors must contain the original anchor
    let effective = parsed["effective_anchors"].as_array().unwrap();
    assert!(
        effective.iter().any(|a| a.as_str() == Some("auth")),
        "effective_anchors must contain the original anchor 'auth': {effective:?}"
    );
}

#[test]
fn investigate_coverage_shows_analyzed_sources() {
    let f = Fixture::create();
    f.run_ok(&["ingest", "."]);

    let output = f.run_ok(&["investigate", "auth"]);

    assert!(
        output.contains("COVERAGE"),
        "expected COVERAGE section:\n{output}"
    );
    assert!(
        output.contains("Git history"),
        "expected Git history in coverage:\n{output}"
    );
    assert!(
        output.contains("Dynamic dispatch"),
        "expected dynamic dispatch listed as not analyzed:\n{output}"
    );
}

// ── repo autodiscovery tests ──────────────────────────────────────────────────

#[test]
fn query_from_subdirectory_autodiscovers_repo_root() {
    let f = Fixture::create();
    f.run_ok(&["ingest", "."]);

    // Create a nested subdirectory inside the fixture repo.
    let subdir = format!("{}/src/modules/core", f.repo);
    std::fs::create_dir_all(&subdir).unwrap();

    // Run atlas from the subdirectory — must auto-discover the parent .git root
    // and return the same co-change result as running from the root.
    let out = Command::new(atlas_bin())
        .args(["co-changes", "auth.ts"])
        .current_dir(&subdir)
        .env("ATLAS_DB", &f.db)
        .output()
        .expect("atlas binary");

    assert!(
        out.status.success(),
        "atlas from subdirectory failed:\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("user.ts"),
        "expected user.ts in co-changes when run from subdirectory:\n{stdout}"
    );
}

#[test]
fn query_outside_git_repo_fails_with_clear_message() {
    let tmp    = tempfile::tempdir().expect("tempdir");
    let db_dir = tempfile::tempdir().expect("db tempdir");
    let db     = db_dir.path().join("atlas.db").to_str().unwrap().to_string();

    // tmp has no .git — atlas must fail, not silently return empty results.
    let out = Command::new(atlas_bin())
        .args(["co-changes", "auth.ts"])
        .current_dir(tmp.path())
        .env("ATLAS_DB", &db)
        .output()
        .expect("atlas binary");

    assert!(
        !out.status.success(),
        "expected non-zero exit when run outside a git repository"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("not inside a Git repository"),
        "expected clear 'not inside a Git repository' message in stderr:\n{stderr}"
    );
}

#[test]
fn investigate_incoming_expansion_surfaces_service_for_model_seed() {
    let f = Fixture::create();
    f.add_typescript_sources();
    f.run_ok(&["ingest", ".", "--typescript"]);

    // "model" matches only src/models/user.model.ts (path contains "model").
    // user.service.ts is NOT a seed (its path doesn't contain "model").
    // user.service.ts has an outgoing REFERENCES_MODEL edge to user.model.ts,
    // so user.model.ts has an incoming REFERENCES_MODEL edge from user.service.ts.
    // Phase 2 incoming expansion must surface user.service.ts as a structural neighbor.
    let output = f.run_ok(&["investigate", "model", "--raw"]);

    assert!(
        output.contains("user.service.ts"),
        "expected user.service.ts via incoming REFERENCES_MODEL expansion:\n{output}"
    );
}

#[test]
fn structural_import_gaps_surface_common_peer_imports() {
    // Scenario: three *.service.ts files share a directory.
    // Two of them import src/errors/createError.ts.
    // The third (invoice.service.ts) does not.
    // IMPORT GAPS must appear on the third file and name createError.ts.
    let f = Fixture::create();

    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(&f.repo)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {:?} failed", args);
    };

    std::fs::create_dir_all(format!("{}/src/services", f.repo)).unwrap();
    std::fs::create_dir_all(format!("{}/src/errors", f.repo)).unwrap();

    // The shared error utility that peers import (must exist on disk for
    // the TypeScript parser to resolve the relative import path).
    std::fs::write(
        format!("{}/src/errors/createError.ts", f.repo),
        "export function createError(code: string, msg: string) {}\n",
    ).unwrap();

    // Peer A — imports createError.
    std::fs::write(
        format!("{}/src/services/order.service.ts", f.repo),
        "import { createError } from \"../errors/createError.js\";\n\
         export class OrderService {}\n",
    ).unwrap();

    // Peer B — imports createError.
    std::fs::write(
        format!("{}/src/services/payment.service.ts", f.repo),
        "import { createError } from \"../errors/createError.js\";\n\
         export class PaymentService {}\n",
    ).unwrap();

    // Target — imports mongoose but NOT createError (the gap we expect to surface).
    std::fs::write(
        format!("{}/src/services/invoice.service.ts", f.repo),
        "import mongoose from \"mongoose\";\nexport class InvoiceService {}\n",
    ).unwrap();

    git(&["add", "src/"]);
    let out = Command::new("git")
        .args(["commit", "-m", "Add services for import-gap test"])
        .current_dir(&f.repo)
        .env("GIT_AUTHOR_DATE",    "2024-01-05T10:00:00+0000")
        .env("GIT_COMMITTER_DATE", "2024-01-05T10:00:00+0000")
        .output()
        .expect("git commit");
    assert!(out.status.success(), "commit failed");

    f.run_ok(&["ingest", ".", "--typescript"]);

    let output = f.run_ok(&["structural", "src/services/invoice.service.ts"]);

    assert!(
        output.contains("PEER OBSERVATIONS"),
        "expected PEER OBSERVATIONS section for invoice.service.ts:\n{output}"
    );
    assert!(
        output.contains("Imports present in peers but absent here:"),
        "expected import gaps subsection:\n{output}"
    );
    assert!(
        output.contains("src/errors/createError.ts"),
        "expected createError.ts listed as a gap:\n{output}"
    );
    assert!(
        output.contains("2 of 2 peers"),
        "expected '2 of 2 peers' annotation on the gap:\n{output}"
    );
}

#[test]
fn structural_import_gaps_absent_for_complete_peer() {
    // A well-implemented service (one that already has the common imports)
    // must produce no IMPORT GAPS section.
    let f = Fixture::create();

    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(&f.repo)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {:?} failed", args);
    };

    std::fs::create_dir_all(format!("{}/src/services", f.repo)).unwrap();
    std::fs::create_dir_all(format!("{}/src/errors", f.repo)).unwrap();

    std::fs::write(
        format!("{}/src/errors/createError.ts", f.repo),
        "export function createError(code: string, msg: string) {}\n",
    ).unwrap();

    // Both peers import createError — the complete service must produce no gaps.
    for name in &["order", "payment"] {
        std::fs::write(
            format!("{}/src/services/{name}.service.ts", f.repo),
            "import { createError } from \"../errors/createError.js\";\n\
             export class Service {}\n",
        ).unwrap();
    }

    git(&["add", "src/"]);
    let out = Command::new("git")
        .args(["commit", "-m", "Add complete services"])
        .current_dir(&f.repo)
        .env("GIT_AUTHOR_DATE",    "2024-01-05T10:00:00+0000")
        .env("GIT_COMMITTER_DATE", "2024-01-05T10:00:00+0000")
        .output()
        .expect("git commit");
    assert!(out.status.success(), "commit failed");

    f.run_ok(&["ingest", ".", "--typescript"]);

    // order.service.ts already imports createError — no gap expected.
    let output = f.run_ok(&["structural", "src/services/order.service.ts"]);

    assert!(
        !output.contains("PEER OBSERVATIONS"),
        "PEER OBSERVATIONS must not appear for a complete service:\n{output}"
    );
}

#[test]
fn structural_call_gaps_surface_common_peer_static_calls() {
    // Two peer *.service.ts files both make a static call to StaticHelper.process().
    // The third service imports something else but never calls StaticHelper.
    // PEER OBSERVATIONS must surface the missing static call.
    let f = Fixture::create();

    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(&f.repo)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {:?} failed", args);
    };

    std::fs::create_dir_all(format!("{}/src/services", f.repo)).unwrap();
    std::fs::create_dir_all(format!("{}/src/utils", f.repo)).unwrap();

    std::fs::write(
        format!("{}/src/utils/staticHelper.ts", f.repo),
        "export class StaticHelper { static process(x: string) {} }\n",
    ).unwrap();

    // Peers A and B both call StaticHelper.process().
    for name in &["alpha", "beta"] {
        std::fs::write(
            format!("{}/src/services/{name}.service.ts", f.repo),
            "import { StaticHelper } from \"../utils/staticHelper.js\";\n\
             StaticHelper.process(\"arg\");\n\
             export class Service {}\n",
        ).unwrap();
    }

    // Target imports an external but never calls StaticHelper.
    std::fs::write(
        format!("{}/src/services/gamma.service.ts", f.repo),
        "import mongoose from \"mongoose\";\nexport class GammaService {}\n",
    ).unwrap();

    git(&["add", "src/"]);
    let out = Command::new("git")
        .args(["commit", "-m", "Add services for call-gap test"])
        .current_dir(&f.repo)
        .env("GIT_AUTHOR_DATE",    "2024-01-05T10:00:00+0000")
        .env("GIT_COMMITTER_DATE", "2024-01-05T10:00:00+0000")
        .output()
        .expect("git commit");
    assert!(out.status.success(), "commit failed");

    f.run_ok(&["ingest", ".", "--typescript"]);

    let output = f.run_ok(&["structural", "src/services/gamma.service.ts"]);

    assert!(
        output.contains("PEER OBSERVATIONS"),
        "expected PEER OBSERVATIONS section:\n{output}"
    );
    assert!(
        output.contains("Static calls present in peers but absent here:"),
        "expected static calls subsection:\n{output}"
    );
    assert!(
        output.contains("StaticHelper.process"),
        "expected StaticHelper.process listed as a call gap:\n{output}"
    );
    assert!(
        output.contains("2 of 2 peers"),
        "expected '2 of 2 peers' annotation:\n{output}"
    );
}

#[test]
fn structural_model_ref_gaps_surface_common_peer_models() {
    // Two peer *.service.ts files both reference the UserModel via findById().
    // The third service imports mongoose but never queries UserModel.
    // PEER OBSERVATIONS must surface the missing model reference.
    let f = Fixture::create();

    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(&f.repo)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {:?} failed", args);
    };

    std::fs::create_dir_all(format!("{}/src/services", f.repo)).unwrap();
    std::fs::create_dir_all(format!("{}/src/models", f.repo)).unwrap();

    std::fs::write(
        format!("{}/src/models/account.model.ts", f.repo),
        "import { model } from \"mongoose\";\nexport const Account = model(\"Account\", {});\n",
    ).unwrap();

    // Peers A and B both query the Account model.
    for name in &["billing", "payment"] {
        std::fs::write(
            format!("{}/src/services/{name}.service.ts", f.repo),
            "import { Account } from \"../models/account.model.js\";\n\
             Account.findById(\"id\");\n\
             export class Service {}\n",
        ).unwrap();
    }

    // Target imports mongoose directly but never queries Account.
    std::fs::write(
        format!("{}/src/services/shipping.service.ts", f.repo),
        "import mongoose from \"mongoose\";\nexport class ShippingService {}\n",
    ).unwrap();

    git(&["add", "src/"]);
    let out = Command::new("git")
        .args(["commit", "-m", "Add services for model-ref-gap test"])
        .current_dir(&f.repo)
        .env("GIT_AUTHOR_DATE",    "2024-01-05T10:00:00+0000")
        .env("GIT_COMMITTER_DATE", "2024-01-05T10:00:00+0000")
        .output()
        .expect("git commit");
    assert!(out.status.success(), "commit failed");

    f.run_ok(&["ingest", ".", "--typescript"]);

    let output = f.run_ok(&["structural", "src/services/shipping.service.ts"]);

    assert!(
        output.contains("PEER OBSERVATIONS"),
        "expected PEER OBSERVATIONS section:\n{output}"
    );
    assert!(
        output.contains("Model references present in peers but absent here:"),
        "expected model references subsection:\n{output}"
    );
    assert!(
        output.contains("src/models/account.model.ts"),
        "expected account.model.ts listed as a model reference gap:\n{output}"
    );
    assert!(
        output.contains("2 of 2 peers"),
        "expected '2 of 2 peers' annotation:\n{output}"
    );
}
